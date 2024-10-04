//! The database module.
//!
//! We do not need concurrency here as this is our actual source of truth for legally binging
//! commercial offers and contracts, hence causality is a must. A care must be taken that no threads
//! are spawned here and all locking methods are called in sync functions without sharing lock
//! guards with the async code.

use crate::{
    arguments::{ChainName, OLD_SEED},
    chain_wip::definitions::{ConnectedChain, PreparedChain, H256, H64},
    error::{DbError, OrderError},
    signer::{KeyStore, Signer},
    utils::PathDisplay,
};
use ahash::{HashMap, HashMapExt, HashSet, HashSetExt, RandomState};
use arguments::InsertOrder;
use codec::{DecodeAll, Encode};
use indexmap::{map::Entry, IndexMap};
use names::{Generator, Name};
use redb::{
    backends::{FileBackend, InMemoryBackend},
    AccessGuard, Database as Redb, ReadOnlyTable, ReadTransaction, ReadableTable, Table,
    TableHandle, Value, WriteTransaction,
};
use std::{
    array, borrow::Cow, ffi::OsStr, fs::File, io::ErrorKind, num::NonZeroU64 as StdNonZeroU64,
    sync::Arc,
};

pub mod definitions;

use definitions::{
    ChainHash, ChainName as DbChainName, ChainProperties, DaemonInfo, KeysTable, NonZeroU64,
    OrderId, OrderInfo, OrdersPerChainTable, OrdersTable, PaymentStatus, Public, RootKey,
    RootTable, RootValue, TableRead, TableTrait, TableTypes, TableWrite, Timestamp, Version,
};

pub const MODULE: &str = module_path!();
pub const DB_VERSION: Version = Version(0);

pub struct Database {
    db: Redb,
    instance: String,
}

impl Database {
    #[expect(clippy::too_many_lines, clippy::type_complexity)]
    pub fn new(
        path_option: Option<Cow<'static, OsStr>>,
        mut connected_chains: IndexMap<ChainName, ConnectedChain, RandomState>,
        key_store: KeyStore,
    ) -> Result<
        (
            Arc<Self>,
            Arc<Signer>,
            IndexMap<ChainName, PreparedChain, RandomState>,
        ),
        DbError,
    > {
        let builder = Redb::builder();

        let (mut database, is_new) = if let Some(path) = path_option {
            match File::create_new(&path) {
                Ok(file) => {
                    tracing::info!("Creating the database at {}.", PathDisplay(path));

                    FileBackend::new(file)
                        .and_then(|backend| builder.create_with_backend(backend))
                        .map(|db| (db, true))
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                    tracing::info!("Opening the database at {}.", PathDisplay(&path));

                    builder.open(path).map(|db| (db, false))
                }
                Err(error) => Err(error.into()),
            }
        } else {
            tracing::warn!(
                "The in-memory backend for the database is selected. All saved data will be deleted after the shutdown!"
            );

            builder
                .create_with_backend(InMemoryBackend::new())
                .map(|db| (db, true))
        }?;

        let tx = TxWrite::new(&database)?.0;
        let mut root = RootTable::open(&tx)?;
        let mut chain_hashes = HashSet::with_capacity(connected_chains.len());
        let public = key_store.public();
        let mut prepared_chains =
            IndexMap::with_capacity_and_hasher(connected_chains.len(), RandomState::new());

        let daemon_info;
        let signer;
        let instance = if is_new {
            signer = key_store.into_signer::<true>(HashMap::new());

            let mut new_db_chains = Vec::with_capacity(connected_chains.len());

            for (name, connected_chain) in connected_chains {
                process_new_chain(
                    &mut chain_hashes,
                    name,
                    &mut new_db_chains,
                    connected_chain,
                    &mut prepared_chains,
                );
            }

            let instance = Generator::with_naming(Name::Numbered).next().unwrap();

            daemon_info = DaemonInfo {
                chains: new_db_chains,
                public,
                old_publics_death_timestamps: vec![],
                instance: instance.clone(),
            };

            root.insert_slot(
                RootKey::DbVersion,
                RootValue(&Version::as_bytes(&DB_VERSION)),
            )?;

            instance
        } else {
            let Some(encoded_db_version) = root.get_slot(RootKey::DbVersion)? else {
                return Err(DbError::NoVersion);
            };

            let db_version = Version::from_bytes(encoded_db_version.value().0);

            if db_version != DB_VERSION {
                return Err(DbError::UnexpectedVersion(db_version));
            }

            let Some(encoded_daemon_info) = root.get_slot(RootKey::DaemonInfo)? else {
                return Err(DbError::NoDaemonInfo);
            };

            let DaemonInfo {
                chains: db_chains,
                public: db_public,
                old_publics_death_timestamps,
                instance,
            } = DaemonInfo::decode_all(&mut encoded_daemon_info.value().0)?;

            #[expect(clippy::arithmetic_side_effects)]
            let mut new_db_chains = IndexMap::with_capacity_and_hasher(
                db_chains.len() + connected_chains.len(),
                RandomState::new(),
            );
            let mut orders_per_chain = OrdersPerChainTable::open(&tx)?;

            for (name, properties) in db_chains {
                process_db_chain(
                    name,
                    properties,
                    &mut chain_hashes,
                    &mut new_db_chains,
                    &mut connected_chains,
                    &mut orders_per_chain,
                    &mut prepared_chains,
                )?;
            }

            for (name, connected_chain) in connected_chains {
                process_new_chain(
                    &mut chain_hashes,
                    name,
                    &mut new_db_chains,
                    connected_chain,
                    &mut prepared_chains,
                );
            }

            let tables = Tables::get(&tx, &chain_hashes)?;
            let (new_old_publics_death_timestamps, prepared_signer) = process_keys(
                &public,
                db_public,
                key_store,
                old_publics_death_timestamps,
                tables.keys,
            )?;

            signer = prepared_signer;

            let mut new_db_chains_vec = Vec::with_capacity(new_db_chains.len());

            new_db_chains_vec.extend(new_db_chains);

            daemon_info = DaemonInfo {
                chains: new_db_chains_vec,
                public,
                old_publics_death_timestamps: new_old_publics_death_timestamps,
                instance: instance.clone(),
            };

            instance
        };

        root.insert_slot(&RootKey::DaemonInfo, &RootValue(&daemon_info.encode()))?;

        drop(root);

        tx.commit()?;

        if database.compact()? {
            tracing::debug!("The database has been compacted.");
        } else {
            tracing::debug!("The database doesn't need to be compacted.");
        }

        Ok((
            Arc::new(Self {
                db: database,
                instance,
            }),
            Arc::new(signer),
            prepared_chains,
        ))
    }

    pub fn instance(&self) -> &str {
        &self.instance
    }

    pub fn read(&self) -> Result<TxRead, DbError> {
        self.db.begin_read().map_err(DbError::TxRead).map(TxRead)
    }

    pub fn write<T>(&self, f: impl FnOnce(&TxWrite) -> Result<T, DbError>) -> Result<T, DbError> {
        let tx = TxWrite::new(&self.db)?;

        let t = f(&tx)?;

        tx.0.commit()?;

        Ok(t)
    }
}

pub struct TxRead(ReadTransaction);

impl TxRead {
    pub fn orders(&self) -> Result<Option<OrdersRead>, DbError> {
        OrdersTable::open_ro(&self.0).map(|some| some.map(OrdersRead))
    }
}

pub struct OrdersRead(
    ReadOnlyTable<<OrdersTable as TableTypes>::Key, <OrdersTable as TableTypes>::Value>,
);

pub trait OrdersReadable {
    fn table(
        &self,
    ) -> &impl ReadableTable<<OrdersTable as TableTypes>::Key, <OrdersTable as TableTypes>::Value>;

    fn read(
        &self,
        key: impl AsRef<str>,
    ) -> Result<Option<<OrdersTable as TableTypes>::Value>, DbError> {
        self.table()
            .get_slot(OrderId(key.as_ref()))
            .map(|some| some.map(|ag| ag.value()))
    }

    fn active(
        &self,
    ) -> Result<
        impl Iterator<
            Item = Result<
                (
                    AccessGuard<'_, <OrdersTable as TableTypes>::Key>,
                    <OrdersTable as TableTypes>::Value,
                ),
                DbError,
            >,
        >,
        DbError,
    > {
        self.table().iter().map_err(DbError::Range).map(|range| {
            range.filter_map(|result| {
                result
                    .map(|(k, v)| {
                        let order = v.value();

                        (order.payment_status == PaymentStatus::Pending).then_some((k, order))
                    })
                    .map_err(DbError::RangeIter)
                    .transpose()
            })
        })
    }
}

impl OrdersReadable for OrdersRead {
    fn table(
        &self,
    ) -> &impl ReadableTable<<OrdersTable as TableTypes>::Key, <OrdersTable as TableTypes>::Value>
    {
        &self.0
    }
}

impl OrdersReadable for OrdersWrite<'_> {
    fn table(
        &self,
    ) -> &impl ReadableTable<<OrdersTable as TableTypes>::Key, <OrdersTable as TableTypes>::Value>
    {
        &self.0
    }
}

pub struct TxWrite(WriteTransaction);

impl TxWrite {
    fn new(db: &Redb) -> Result<Self, DbError> {
        db.begin_write().map(TxWrite).map_err(DbError::TxWrite)
    }

    pub fn orders(&self) -> Result<OrdersWrite<'_>, DbError> {
        OrdersTable::open(&self.0).map(OrdersWrite)
    }

    pub fn orders_per_chain(&self) -> Result<OrdersPerChainWrite<'_>, DbError> {
        OrdersPerChainTable::open(&self.0).map(OrdersPerChainWrite)
    }

    pub fn keys(&self) -> Result<KeysWrite<'_>, DbError> {
        KeysTable::open(&self.0).map(KeysWrite)
    }

    pub fn insert_order(
        mut orders: OrdersWrite<'_>,
        mut orders_per_chain: OrdersPerChainWrite<'_>,
        mut keys: KeysWrite<'_>,
        InsertOrder {
            chain,
            key,
            public,
            callback,
            currency,
            amount,
            account_lifetime,
            payment_account,
        }: InsertOrder<impl AsRef<str>>,
    ) -> Result<OrderInfo, DbError> {
        let key_as_ref = key.as_ref();
        let order_option = orders.read(key_as_ref)?;

        if let Some(order) = order_option {
            order.update(callback, chain, currency, amount, account_lifetime)
        } else {
            let increment = |option_ag: Option<AccessGuard<'_, NonZeroU64>>| {
                NonZeroU64(option_ag.map_or(StdNonZeroU64::MIN, |ag| {
                    ag.value()
                        .0
                        .checked_add(1)
                        .expect("database can't store more than `u64::MAX` orders")
                }))
            };
            let new = OrderInfo::new(
                amount,
                callback,
                payment_account,
                currency,
                account_lifetime,
                chain,
            )?;

            orders.0.insert_slot(OrderId(key_as_ref), &new)?;

            let mut n = increment(orders_per_chain.0.get_slot(chain)?);

            orders_per_chain.0.insert_slot(chain, n)?;

            n = increment(keys.0.get_slot(public)?);

            keys.0.insert_slot(public, n)?;

            Ok(new)
        }
    }
}

pub struct OrdersWrite<'a>(
    Table<'a, <OrdersTable as TableTypes>::Key, <OrdersTable as TableTypes>::Value>,
);

impl OrdersWrite<'_> {
    pub fn mark_paid(
        &mut self,
        key: impl AsRef<str>,
    ) -> Result<<OrdersTable as TableTypes>::Value, DbError> {
        let key_as_ref = key.as_ref();

        let Some(mut order) = self.read(key_as_ref)? else {
            return Err(OrderError::NotFound.into());
        };

        if order.payment_status != PaymentStatus::Pending {
            return Err(OrderError::Paid.into());
        }

        order.payment_status = PaymentStatus::Paid;

        self.0.insert_slot(OrderId(key_as_ref), &order)?;

        Ok(order)
    }
}

pub struct OrdersPerChainWrite<'a>(
    Table<'a, <OrdersPerChainTable as TableTypes>::Key, <OrdersPerChainTable as TableTypes>::Value>,
);

pub struct KeysWrite<'a>(
    Table<'a, <KeysTable as TableTypes>::Key, <KeysTable as TableTypes>::Value>,
);

struct Tables<'a> {
    keys: Option<Table<'a, <KeysTable as TableTypes>::Key, <KeysTable as TableTypes>::Value>>,
    orders: Option<Table<'a, <OrdersTable as TableTypes>::Key, <OrdersTable as TableTypes>::Value>>,
}

impl<'a> Tables<'a> {
    /// Collects all tables in the database & purges unknown ones (e.g. those left after a
    /// migration).
    fn get(
        tx: &'a WriteTransaction,
        chain_hashes: &HashSet<ChainHash>,
    ) -> Result<Tables<'a>, DbError> {
        let mut keys = None;
        let mut orders = None;

        for table in tx.list_tables().map_err(DbError::TableList)? {
            match table.name() {
                // These tables were opened before a calling of `Tables::get()`.
                RootTable::NAME | OrdersPerChainTable::NAME => {}
                KeysTable::NAME => keys = Some(KeysTable::open(tx)?),
                OrdersTable::NAME => orders = Some(OrdersTable::open(tx)?),
                other_name => {
                    if open_chain_tables(tx, chain_hashes, other_name)? {
                        tracing::debug!(
                            "Detected an unknown table {other_name:?}, it'll be purged."
                        );

                        tx.delete_table(table).map_err(DbError::DeleteTable)?;
                    }
                }
            }
        }

        Ok(Self { keys, orders })
    }
}

#[expect(clippy::unnecessary_wraps, unused)]
fn open_chain_tables(
    tx: &WriteTransaction,
    chain_hashes: &HashSet<ChainHash>,
    other_name: &str,
) -> Result<bool, DbError> {
    let Some(hash_start) = other_name.len().checked_sub(H64::HEX_LENGTH) else {
        return Ok(true);
    };

    let Some((stripped, maybe_hash)) = other_name.split_at_checked(hash_start) else {
        return Ok(true);
    };

    #[expect(clippy::match_single_binding)]
    match stripped {
        // TODO: Arms with chain tables.
        _ => Ok(true),
    }
}

trait NewDbChains {
    fn add(self, name: DbChainName, properties: ChainProperties);
}

impl NewDbChains for &mut IndexMap<DbChainName, ChainProperties, RandomState> {
    fn add(self, name: DbChainName, properties: ChainProperties) {
        self.insert(name, properties);
    }
}

impl NewDbChains for &mut Vec<(DbChainName, ChainProperties)> {
    fn add(self, name: DbChainName, properties: ChainProperties) {
        self.push((name, properties));
    }
}

fn strip_array<T, const N1: usize, const N2: usize>(array: [T; N1]) -> [T; N2] {
    let mut iterator = array.into_iter();

    array::from_fn(|_| {
        iterator.next().expect(
            "number of elements in the returned array should be equal or less than in the \
            given array",
        )
    })
}

fn process_new_chain(
    chain_hashes: &mut HashSet<ChainHash>,
    name: ChainName,
    new_db_chains: impl NewDbChains,
    chain: ConnectedChain,
    prepared_chains: &mut IndexMap<ChainName, PreparedChain, RandomState>,
) {
    let mut chain_hash = ChainHash(strip_array(chain.genesis.0.to_be_bytes()));
    let mut step = 1;

    loop {
        if chain_hashes.insert(chain_hash) {
            tracing::debug!(
                "The new {name:?} chain is assigned to the {:#} hash.",
                H64::from(chain_hash)
            );

            new_db_chains.add(
                (&name).into(),
                ChainProperties {
                    genesis: chain.genesis.into(),
                    hash: chain_hash,
                },
            );
            prepared_chains.insert(
                name,
                PreparedChain {
                    hash: chain_hash,
                    connected: chain,
                },
            );

            break;
        }

        tracing::debug!(
            "Failed to assign the new {name:?} chain to the {:#} hash. Probing the next slot...",
            H64::from(chain_hash)
        );

        chain_hash = H64(H64::from_be_bytes(chain_hash.0).0.overflowing_add(step).0).into();
        step = step
            .checked_add(1)
            .expect("database can't store more than `u64::MAX` chains");
    }
}

fn process_db_chain(
    db_name: DbChainName,
    properties: ChainProperties,
    chain_hashes: &mut HashSet<ChainHash>,
    new_db_chains: &mut IndexMap<DbChainName, ChainProperties, RandomState>,
    connected_chains: &mut IndexMap<ChainName, ConnectedChain, RandomState>,
    orders_per_chain: &mut Table<
        '_,
        <OrdersPerChainTable as TableTypes>::Key,
        <OrdersPerChainTable as TableTypes>::Value,
    >,
    prepared_chains: &mut IndexMap<ChainName, PreparedChain, RandomState>,
) -> Result<(), DbError> {
    if !chain_hashes.insert(properties.hash) {
        tracing::debug!(
            "Found the {db_name:?} chain with the hash duplicate {:#} in the database.",
            H64::from(properties.hash)
        );

        return Ok(());
    }

    let entry = match new_db_chains.entry(db_name) {
        Entry::Occupied(entry) => {
            tracing::debug!(
                "Found 2 chains with same name ({:?}) in the database.",
                entry.key()
            );

            return Ok(());
        }
        Entry::Vacant(entry) => entry,
    };

    tracing::debug!(name = ?entry.key(), properties = ?properties);

    if let Some((name, connected)) = connected_chains.swap_remove_entry(&*entry.key().0) {
        let given = connected.genesis.into();

        if given != properties.genesis {
            return Err(DbError::GenesisMismatch {
                chain: entry.into_key().0,
                expected: properties.genesis,
                given,
            });
        }

        tracing::debug!(
            "The {name:?} chain stored in the database is assigned to the {:?} hash.",
            H64::from(properties.hash),
        );

        prepared_chains.insert(
            name,
            PreparedChain {
                hash: properties.hash,
                connected,
            },
        );
    } else if orders_per_chain.get_slot(properties.hash)?.is_some() {
        tracing::warn!(
            "The {:?} chain exists in the database but isn't present in the config.",
            entry.key(),
        );
    } else {
        tracing::info!(
            "The {:?} chain exists in the database but isn't present in the config and has no \
            orders. It'll be deleted.",
            entry.key()
        );

        return Ok(());
    }

    entry.insert(properties);

    Ok(())
}

fn process_keys(
    public: &Public,
    db_public: Public,
    mut key_store: KeyStore,
    old_publics_death_timestamps: Vec<(Public, Timestamp)>,
    keys: Option<Table<'_, <KeysTable as TableTypes>::Key, <KeysTable as TableTypes>::Value>>,
) -> Result<(Vec<(Public, Timestamp)>, Signer), DbError> {
    // Since the current key can be changed, this array should've a capacity of one more element.
    #[expect(clippy::arithmetic_side_effects)]
    let mut new_old_publics_death_timestamps =
        Vec::with_capacity(old_publics_death_timestamps.len() + 1);
    let mut filtered_old_pairs = HashMap::with_capacity(key_store.old_pairs_len());
    let mut restored_key_timestamp = None;
    let print_about_duplicate = |old_public| {
        tracing::debug!(
            "Found a public key duplicate {:#} in the database.",
            H256::from(old_public)
        );
    };

    for (old_public, timestamp) in old_publics_death_timestamps {
        match key_store.remove(public) {
            Some((entropy, name)) => {
                tracing::info!(
                    "The public key {:#} in the database was matched with `{OLD_SEED}{name}`.",
                    H256::from(old_public)
                );

                filtered_old_pairs.insert(old_public, (entropy, name));
                new_old_publics_death_timestamps.push((old_public, timestamp));
            }
            None if old_public == *public => {
                if restored_key_timestamp.is_none() {
                    restored_key_timestamp = Some(timestamp);
                } else {
                    print_about_duplicate(old_public);
                }
            }
            None if filtered_old_pairs.contains_key(&old_public) => {
                print_about_duplicate(old_public);
            }
            None => {
                return Err(DbError::OldKeyNotFound {
                    key: old_public,
                    removed: timestamp,
                })
            }
        }
    }

    if *public != db_public {
        if let Some(timestamp) = restored_key_timestamp {
            tracing::info!(
                "The current key {:#} will be changed to {:#} removed on {}.",
                H256::from(db_public),
                H256::from(*public),
                timestamp,
            );
        } else {
            tracing::info!(
                "The current key {:#} will be changed to {:#}.",
                H256::from(db_public),
                H256::from(*public),
            );
        }

        let is_db_public_in_circulation = keys.map_or(Ok(false), |table| {
            table
                .get_slot(&db_public)
                .map(|slot_option| slot_option.is_some())
        })?;

        if is_db_public_in_circulation {
            key_store
                .remove(&db_public)
                .ok_or(DbError::CurrentKeyNotFound(db_public))?;
            new_old_publics_death_timestamps.push((db_public, Timestamp::now()?));
        } else {
            tracing::info!(
                "The current key has no accounts associated with it and hence will be deleted."
            );
        }
    }

    Ok((
        new_old_publics_death_timestamps,
        key_store.into_signer::<false>(filtered_old_pairs),
    ))
}

pub mod arguments {
    use super::definitions::{Account, Amount, ChainHash, CurrencyInfo, Public, Timestamp, Url};

    pub struct InsertOrder<K> {
        pub chain: ChainHash,
        pub key: K,
        pub public: Public,
        pub callback: Url,
        pub currency: CurrencyInfo,
        pub amount: Amount,
        pub account_lifetime: Timestamp,
        pub payment_account: Account,
    }
}
