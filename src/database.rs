use crate::{
    rpc::{ConnectedChain, Currency},
    server::{CurrencyInfo, CurrencyProperties, OrderQuery, OrderStatus, ServerInfo, ServerStatus},
    AccountId, AssetId, Balance, BlockNumber, Config, Nonce, Timestamp, Version,
};
use anyhow::{Context, Result};
use redb::{
    backends::{FileBackend, InMemoryBackend},
    Database, ReadOnlyTable, ReadableTable, Table, TableDefinition, TableHandle, TypeName, Value,
};
use serde::Deserialize;
use std::{collections::HashMap, fs::File, io::ErrorKind, sync::Arc};
use subxt::ext::{
    codec::{Compact, Decode, Encode},
    sp_core::{
        crypto::Ss58Codec,
        sr25519::{Pair, Public},
    },
};
use tokio::sync::{mpsc, oneshot, RwLock};

pub const MODULE: &str = module_path!();

#[derive(Debug)]
pub enum DbError {
    CurrencyKeyNotFound,
    DbEngineDown,
}

// Tables

const ROOT: TableDefinition<'_, &str, &[u8]> = TableDefinition::new("root");
const KEYS: TableDefinition<'_, PublicSlot, U256Slot> = TableDefinition::new("keys");
const CHAINS: TableDefinition<'_, ChainHash, BlockNumber> = TableDefinition::new("chains");
const INVOICES: TableDefinition<'_, InvoiceKey, Invoice> = TableDefinition::new("invoices");

const ACCOUNTS: &str = "accounts";

type ACCOUNTS_KEY = (Option<AssetId>, Account);
type ACCOUNTS_VALUE = InvoiceKey;

const TRANSACTIONS: &str = "transactions";

type TRANSACTIONS_KEY = BlockNumber;
type TRANSACTIONS_VALUE = (Account, Nonce, Transfer);

const HIT_LIST: &str = "hit_list";

type HIT_LIST_KEY = BlockNumber;
type HIT_LIST_VALUE = (Option<AssetId>, Account);

// `ROOT` keys

// The database version must be stored in a separate slot to be used by the not implemented yet
// database migration logic.
const DB_VERSION_KEY: &str = "db_version";
const DAEMON_INFO: &str = "daemon_info";

// Slots

type InvoiceKey = &'static [u8];
type U256Slot = [u64; 4];
type BlockHash = [u8; 32];
type ChainHash = [u8; 32];
type PublicSlot = [u8; 32];
type BalanceSlot = u128;
type Derivation = [u8; 32];
type Account = [u8; 32];

#[derive(Encode, Decode)]
#[codec(crate = subxt::ext::codec)]
enum ChainKind {
    Id(Vec<Compact<AssetId>>),
    MultiLocation(Vec<Compact<AssetId>>),
}

#[derive(Encode, Decode)]
#[codec(crate = subxt::ext::codec)]
struct DaemonInfo {
    chains: Vec<(String, ChainProperties)>,
    current_key: PublicSlot,
    old_keys_death_timestamps: Vec<(PublicSlot, Timestamp)>,
}

#[derive(Encode, Decode)]
#[codec(crate = subxt::ext::codec)]
struct ChainProperties {
    genesis: BlockHash,
    hash: ChainHash,
    kind: ChainKind,
}

#[derive(Encode, Decode)]
#[codec(crate = subxt::ext::codec)]
struct Transfer(Option<Compact<AssetId>>, #[codec(compact)] BalanceSlot);

#[derive(Encode, Decode, Debug)]
#[codec(crate = subxt::ext::codec)]
struct Invoice {
    derivation: (PublicSlot, Derivation),
    paid: bool,
    #[codec(compact)]
    timestamp: Timestamp,
    #[codec(compact)]
    price: BalanceSlot,
    callback: String,
    message: String,
    transactions: TransferTxs,
}

#[derive(Encode, Decode, Debug)]
#[codec(crate = subxt::ext::codec)]
enum TransferTxs {
    Asset {
        #[codec(compact)]
        id: AssetId,
        // transactions: TransferTxsAsset,
    },
    Native {
        recipient: Account,
        encoded: Vec<u8>,
        exact_amount: Option<Compact<BalanceSlot>>,
    },
}

// #[derive(Encode, Decode, Debug)]
// #[codec(crate = subxt::ext::codec)]
// struct TransferTxsAsset<T> {
//     recipient: Account,
//     encoded: Vec<u8>,
//     #[codec(compact)]
//     amount: BalanceSlot,
// }

#[derive(Encode, Decode, Debug)]
#[codec(crate = subxt::ext::codec)]
struct TransferTx {
    recipient: Account,
    exact_amount: Option<Compact<BalanceSlot>>,
}

impl Value for Invoice {
    type SelfType<'a> = Self;

    type AsBytes<'a> = Vec<u8>;

    fn fixed_width() -> Option<usize> {
        None
    }

    fn from_bytes<'a>(mut data: &[u8]) -> Self::SelfType<'_>
    where
        Self: 'a,
    {
        Self::decode(&mut data).unwrap()
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'a>) -> Self::AsBytes<'_> {
        value.encode()
    }

    fn type_name() -> TypeName {
        TypeName::new(stringify!(Invoice))
    }
}

pub struct ConfigWoChains {
    pub recipient: AccountId,
    pub debug: bool,
    pub remark: String,
    pub depth: Option<BlockNumber>,
    pub account_lifetime: BlockNumber,
    pub rpc: String,
}

enum StateAccessRequest {
    GetInvoiceStatus(GetInvoiceStatus),
    CreateInvoice(CreateInvoice),
    ServerStatus(oneshot::Sender<ServerStatus>),
}

struct GetInvoiceStatus {
    pub order: String,
    pub res: oneshot::Sender<OrderStatus>,
}

struct CreateInvoice {
    pub order_query: OrderQuery,
    pub res: oneshot::Sender<OrderStatus>,
}

//impl StateInterface {
/*
    Ok((
        OrderStatus {
            order,
            payment_status: if invoice.paid {
                PaymentStatus::Paid
            } else {
                PaymentStatus::Pending
            },
            message: String::new(),
            recipient: state.0.recipient.to_ss58check(),
            server_info: state.server_info(),
            order_info: OrderInfo {
                withdrawal_status: WithdrawalStatus::Waiting,
                amount: invoice.amount.format(6),
                currency: CurrencyInfo {
                    currency: "USDC".into(),
                    chain_name: "assethub-polkadot".into(),
                    kind: TokenKind::Asset,
                    decimals: 6,
                    rpc_url: state.rpc.clone(),
                    asset_id: Some(1337),
                },
                callback: invoice.callback.clone(),
                transactions: vec![],
                payment_account: invoice.paym_acc.to_ss58check(),
            },
        },
        OrderSuccess::Found,
    ))
} else {
    Ok((
        OrderStatus {
            order,
            payment_status: PaymentStatus::Unknown,
            message: String::new(),
            recipient: state.0.recipient.to_ss58check(),
            server_info: state.server_info(),
            order_info: OrderInfo {
                withdrawal_status: WithdrawalStatus::Waiting,
                amount: 0f64,
                currency: CurrencyInfo {
                    currency: "USDC".into(),
                    chain_name: "assethub-polkadot".into(),
                    kind: TokenKind::Asset,
                    decimals: 6,
                    rpc_url: state.rpc.clone(),
                    asset_id: Some(1337),
                },
                callback: String::new(),
                transactions: vec![],
                payment_account: String::new(),
            },
        },
        OrderSuccess::Found,
    ))
}*/

/*
 *
let pay_acc: AccountId = state
        .0
        .pair
        .derive(vec![DeriveJunction::hard(order.clone())].into_iter(), None)
        .unwrap()
        .0
        .public()
        .into();

 * */

/*(
    OrderStatus {
        order,
        payment_status: PaymentStatus::Pending,
        message: String::new(),
        recipient: state.0.recipient.to_ss58check(),
        server_info: state.server_info(),
        order_info: OrderInfo {
            withdrawal_status: WithdrawalStatus::Waiting,
            amount,
            currency: CurrencyInfo {
                currency: "USDC".into(),
                chain_name: "assethub-polkadot".into(),
                kind: TokenKind::Asset,
                decimals: 6,
                rpc_url: state.rpc.clone(),
                asset_id: Some(1337),
            },
            callback,
            transactions: vec![],
            payment_account: pay_acc.to_ss58check(),
        },
    },
    OrderSuccess::Created,
))*/

/*
        ServerStatus {
            description: state.server_info(),
            supported_currencies: state.currencies.clone(),
        }
*/

#[derive(Clone, Debug)]
pub struct State {
    pub tx: tokio::sync::mpsc::Sender<StateAccessRequest>,
}

#[derive(Deserialize, Debug)]
pub struct Invoicee {
    pub callback: String,
    pub amount: Balance,
    pub paid: bool,
    pub paym_acc: AccountId,
}

impl State {
    pub fn initialise(
        path_option: Option<String>,
        currencies: HashMap<String, CurrencyProperties>,
        current_pair: Pair,
        old_pairs: HashMap<String, Pair>,
        ConfigWoChains {
            recipient,
            debug,
            remark,
            depth,
            account_lifetime,
            rpc,
        }: ConfigWoChains,
    ) -> Result<Self> {
        let builder = Database::builder();
        let is_new;

        let database = if let Some(path) = path_option {
            tracing::info!("Creating/Opening the database at {path:?}.");

            match File::create_new(&path) {
                Ok(file) => {
                    is_new = true;

                    FileBackend::new(file).and_then(|backend| builder.create_with_backend(backend))
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                    is_new = false;

                    builder.create(path)
                }
                Err(error) => Err(error.into())
            }
        } else {
            tracing::warn!(
                "The in-memory backend for the database is selected. All saved data will be deleted after the shutdown!"
            );

            is_new = true;

            builder.create_with_backend(InMemoryBackend::new())
        }.context("failed to create/open the database")?;

        /*
            currencies: HashMap<String, CurrencyProperties>,
            recipient: AccountId,
            pair: Pair,
            depth: Option<Timestamp>,
            account_lifetime: Timestamp,
            debug: bool,
            remark: String,
            invoices: RwLock<HashMap<String, Invoicee>>,
            rpc: String,
        */
        let (tx, mut rx) = tokio::sync::mpsc::channel(1024);
        tokio::spawn(async move {
            while let Some(request) = rx.recv().await {
                //database;
            }
        });

        Ok(Self { tx })
    }

    pub async fn order_status(&self, order: &str) -> Result<OrderStatus, DbError> {
        let (res, mut rx) = oneshot::channel();
        self.tx
            .send(StateAccessRequest::GetInvoiceStatus(GetInvoiceStatus {
                order: order.to_string(),
                res,
            }))
            .await;
        rx.await.map_err(|_| DbError::DbEngineDown)
    }

    pub async fn server_status(&self) -> Result<ServerStatus, DbError> {
        let (res, mut rx) = oneshot::channel();
        self.tx.send(StateAccessRequest::ServerStatus(res)).await;
        rx.await.map_err(|_| DbError::DbEngineDown)
    }

    pub async fn create_order(&self, order_query: OrderQuery) -> Result<OrderStatus, DbError> {
        let (res, mut rx) = oneshot::channel();
        /*
                Invoicee {
                        callback: callback.clone(),
                        amount: Balance::parse(amount, 6),
                        paid: false,
                        paym_acc: pay_acc.clone(),
                    },
        */
        self.tx
            .send(StateAccessRequest::CreateInvoice(CreateInvoice {
                order_query,
                res,
            }))
            .await;
        rx.await.map_err(|_| DbError::DbEngineDown)
    }

    pub fn interface(&self) -> Self {
        State {
            tx: self.tx.clone(),
        }
    }
    /*
        pub fn server_info(&self) -> ServerInfo {
            ServerInfo {
                version: env!("CARGO_PKG_VERSION"),
                instance_id: String::new(),
                debug: self.debug,
                kalatori_remark: self.remark.clone(),
            }
        }

        pub fn currency_properties(&self, currency_name: &str) -> Result<&CurrencyProperties, DbError> {
            self.currencies
                .get(currency_name)
                .ok_or(DbError::CurrencyKeyNotFound)
        }

        pub fn currency_info(&self, currency_name: &str) -> Result<CurrencyInfo, DbError> {
            let currency = self.currency_properties(currency_name)?;
            Ok(CurrencyInfo {
                currency: currency_name.to_string(),
                chain_name: currency.chain_name.clone(),
                kind: currency.kind,
                decimals: currency.decimals,
                rpc_url: currency.rpc_url.clone(),
                asset_id: currency.asset_id,
            })
        }
    */
    //     pub fn rpc(&self) -> &str {
    //         &self.rpc
    //     }

    //     pub fn destination(&self) -> &Option<Account> {
    //         &self.destination
    //     }

    //     pub fn write(&self) -> Result<WriteTransaction<'_>> {
    //         self.db
    //             .begin_write()
    //             .map(WriteTransaction)
    //             .context("failed to begin a write transaction for the database")
    //     }

    //     pub fn read(&self) -> Result<ReadTransaction<'_>> {
    //         self.db
    //             .begin_read()
    //             .map(ReadTransaction)
    //             .context("failed to begin a read transaction for the database")
    //     }

    //     pub async fn properties(&self) -> RwLockReadGuard<'_, ChainProperties> {
    //         self.properties.read().await
    //     }

    //     pub fn pair(&self) -> &Pair {
    //         &self.pair
    //     }
}

/*
pub struct ReadTransaction(redb::ReadTransaction);

impl ReadTransaction {
    pub fn invoices(&self) -> Result<ReadInvoices> {
        self.0
            .open_table(INVOICES)
            .map(ReadInvoices)
            .with_context(|| format!("failed to open the `{}` table", INVOICES.name()))
    }
}

pub struct ReadInvoices<'a>(ReadOnlyTable<&'a [u8], Invoice>);

impl <'a> ReadInvoices<'a> {
    pub fn get(&self, account: &Account) -> Result<Option<AccessGuard<'_, Invoice>>> {
        self.0
            .get(&*account)
            .context("failed to get an invoice from the database")
    }
*/
//     pub fn try_iter(
//         &self,
//     ) -> Result<impl Iterator<Item = Result<(AccessGuard<'_, &[u8; 32]>, AccessGuard<'_, Invoice>)>>>
//     {
//         self.0
//             .iter()
//             .context("failed to get the invoices iterator")
//             .map(|iter| iter.map(|item| item.context("failed to get an invoice from the iterator")))
//     }
// }

// pub struct WriteTransaction<'db>(redb::WriteTransaction<'db>);

// impl<'db> WriteTransaction<'db> {
//     pub fn root(&self) -> Result<Root<'db, '_>> {
//         self.0
//             .open_table(ROOT)
//             .map(Root)
//             .with_context(|| format!("failed to open the `{}` table", ROOT.name()))
//     }

//     pub fn invoices(&self) -> Result<WriteInvoices<'db, '_>> {
//         self.0
//             .open_table(INVOICES)
//             .map(WriteInvoices)
//             .with_context(|| format!("failed to open the `{}` table", INVOICES.name()))
//     }

//     pub fn commit(self) -> Result<()> {
//         self.0
//             .commit()
//             .context("failed to commit a write transaction in the database")
//     }
// }

// pub struct WriteInvoices<'db, 'tx>(Table<'db, 'tx, &'static [u8; 32], Invoice>);

// impl WriteInvoices<'_, '_> {
//     pub fn save(
//         &mut self,
//         account: &Account,
//         invoice: &Invoice,
//     ) -> Result<Option<AccessGuard<'_, Invoice>>> {
//         self.0
//             .insert(AsRef::<[u8; 32]>::as_ref(account), invoice)
//             .context("failed to save an invoice in the database")
//     }
// }

// pub struct Root<'db, 'tx>(Table<'db, 'tx, &'static str, Vec<u8>>);

// impl Root<'_, '_> {
//     pub fn save_last_block(&mut self, number: BlockNumber) -> Result<()> {
//         self.0
//             .insert(LAST_BLOCK, Compact(number).encode())
//             .context("context")?;

//         Ok(())
//     }
// }

// fn get_slot(table: &Table<'_, &str, Vec<u8>>, key: &str) -> Result<Option<Vec<u8>>> {
//     table
//         .get(key)
//         .map(|slot_option| slot_option.map(|slot| slot.value().clone()))
//         .with_context(|| format!("failed to get the {key:?} slot"))
// }

// fn decode_slot<T: Decode>(mut slot: &[u8], key: &str) -> Result<T> {
//     T::decode(&mut slot).with_context(|| format!("failed to decode the {key:?} slot"))
// }

// fn insert_daemon_info(
//     table: &mut Table<'_, '_, &str, Vec<u8>>,
//     rpc: String,
//     key: Public,
// ) -> Result<()> {
//     table
//         .insert(DAEMON_INFO, DaemonInfo { rpc, key }.encode())
//         .map(|_| ())
//         .context("failed to insert the daemon info")
// }
