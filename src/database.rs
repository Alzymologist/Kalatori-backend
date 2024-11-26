//! Database server module
//!
//! We do not need concurrency here, as this is our actual source of truth for legally binging
//! commercial offers and contracts, hence causality is a must. Care must be taken that no threads
//! are spawned here other than main database server thread that does everything in series.

use crate::definitions::api_v2::ServerInfo;
use crate::{
    definitions::{
        api_v2::{
            CurrencyInfo, OrderCreateResponse, OrderInfo, OrderQuery, PaymentStatus, Timestamp,
            WithdrawalStatus,
        },
        Version,
    },
    error::DbError,
    utils::task_tracker::TaskTracker,
};
use codec::{Decode, Encode};
use names::Generator;
use std::time::SystemTime;
use substrate_crypto_light::common::AccountId32;
use tokio::sync::{mpsc, oneshot};

pub const MODULE: &str = module_path!();

const DB_VERSION: Version = 0;

// Tables
const ACCOUNTS: &str = "accounts";

//type ACCOUNTS_KEY = (Option<AssetId>, Account);
//type ACCOUNTS_VALUE = InvoiceKey;

const TRANSACTIONS: &str = "transactions";

const HIT_LIST: &str = "hit_list";

// The database version must be stored in a separate slot to be used by the not implemented yet
// database migration logic.
const DB_VERSION_KEY: &str = "db_version";
const SERVER_INFO_ID: &str = "instance_id";

const ORDERS_TABLE: &[u8] = b"orders";
const SERVER_INFO_TABLE: &[u8] = b"server_info";

// Slots

type InvoiceKey = &'static [u8];
type U256Slot = [u64; 4];
type BlockHash = [u8; 32];
type ChainHash = [u8; 32];
type PublicSlot = [u8; 32];
type BalanceSlot = u128;
type Derivation = [u8; 32];
pub type Account = [u8; 32];

pub struct ConfigWoChains {
    pub recipient: AccountId32,
    pub debug: Option<bool>,
    pub remark: Option<String>,
    //pub depth: Option<Duration>,
}

/// Database server handle
#[derive(Clone, Debug)]
pub struct Database {
    tx: mpsc::Sender<DbRequest>,
}

impl Database {
    pub fn init(
        path_option: Option<String>,
        task_tracker: TaskTracker,
        account_lifetime: Timestamp,
    ) -> Result<Self, DbError> {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1024);
        let database = if let Some(path) = path_option {
            tracing::info!("Creating/Opening the database at {path:?}.");

            sled::open(path).map_err(DbError::DbStartError)?
        } else {
            // TODO
            /*
            tracing::warn!(
                "The in-memory backend for the database is selected. All saved data will be deleted after the shutdown!"
            );*/
            sled::open("temp.db").map_err(DbError::DbStartError)?
        };
        let orders = database
            .open_tree(ORDERS_TABLE)
            .map_err(DbError::DbStartError)?;

        task_tracker.spawn("Database server", async move {
            // No process forking beyond this point!
            while let Some(request) = rx.recv().await {
                match request {
                    DbRequest::ActiveOrderList(res) => {
                        let _unused = res.send(Ok(orders
                            .iter()
                            .filter_map(std::result::Result::ok)
                            .filter_map(|(a, b)| {
                                match (String::decode(&mut &a[..]), OrderInfo::decode(&mut &b[..]))
                                {
                                    (Ok(a), Ok(b)) => Some((a, b)),
                                    _ => None,
                                }
                            })
                            .filter(|(a, b)| b.payment_status == PaymentStatus::Pending)
                            .collect()));
                    }
                    DbRequest::CreateOrder(request) => {
                        let _unused = request.res.send(create_order(
                            request.order,
                            request.query,
                            request.currency,
                            request.payment_account,
                            &orders,
                            account_lifetime,
                        ));
                    }
                    DbRequest::ReadOrder(request) => {
                        let _unused = request.res.send(read_order(request.order, &orders));
                    }
                    DbRequest::MarkPaid(request) => {
                        let _unused = request.res.send(mark_paid(request.order, &orders));
                    }
                    DbRequest::MarkWithdrawn(request) => {
                        let _unused = request.res.send(mark_withdrawn(request.order, &orders));
                    }
                    DbRequest::MarkForced(request) => {
                        let _unused = request.res.send(mark_forced(request.order, &orders));
                    }
                    DbRequest::MarkStuck(request) => {
                        let _unused = request.res.send(mark_stuck(request.order, &orders));
                    }
                    DbRequest::InitializeServerInfo(res) => {
                        let server_info_tree = database
                            .open_tree(SERVER_INFO_TABLE)
                            .map_err(DbError::DbStartError);
                        let result = server_info_tree.and_then(|tree| {
                            if let Some(server_info_data) =
                                tree.get(SERVER_INFO_ID).map_err(DbError::DbInternalError)?
                            {
                                let server_info: ServerInfo =
                                    serde_json::from_slice(&server_info_data).map_err(|e| {
                                        DbError::DeserializationError(e.to_string())
                                    })?;
                                Ok(server_info.instance_id)
                            } else {
                                let mut generator = Generator::default();
                                let new_instance_id = generator
                                    .next()
                                    .unwrap_or_else(|| "unknown-instance".to_string());
                                let server_info_data = ServerInfo {
                                    version: env!("CARGO_PKG_VERSION").to_string(),
                                    instance_id: new_instance_id.clone(),
                                    debug: false,
                                    kalatori_remark: None,
                                };
                                tree.insert(
                                    SERVER_INFO_ID,
                                    serde_json::to_vec(&server_info_data)
                                        .map_err(|e| DbError::SerializationError(e.to_string()))?,
                                )?;
                                Ok(new_instance_id)
                            }
                        });
                        let _unused = res.send(result);
                    }
                    DbRequest::Shutdown(res) => {
                        let _ = res.send(());
                        break;
                    }
                };
            }

            drop(database.flush());

            Ok("Database server is shutting down")
        });

        Ok(Self { tx })
    }

    pub async fn initialize_server_info(&self) -> Result<String, DbError> {
        let (res, rx) = oneshot::channel();
        let _unused = self.tx.send(DbRequest::InitializeServerInfo(res)).await;
        rx.await.map_err(|_| DbError::DbEngineDown)?
    }

    pub async fn order_list(&self) -> Result<Vec<(String, OrderInfo)>, DbError> {
        let (res, rx) = oneshot::channel();
        let _unused = self.tx.send(DbRequest::ActiveOrderList(res)).await;
        rx.await.map_err(|_| DbError::DbEngineDown)?
    }

    pub async fn create_order(
        &self,
        order: String,
        query: OrderQuery,
        currency: CurrencyInfo,
        payment_account: String,
    ) -> Result<OrderCreateResponse, DbError> {
        let (res, rx) = oneshot::channel();
        let _unused = self
            .tx
            .send(DbRequest::CreateOrder(CreateOrder {
                order,
                query,
                currency,
                payment_account,
                res,
            }))
            .await;
        rx.await.map_err(|_| DbError::DbEngineDown)?
    }

    pub async fn read_order(&self, order: String) -> Result<Option<OrderInfo>, DbError> {
        let (res, rx) = oneshot::channel();
        let _unused = self
            .tx
            .send(DbRequest::ReadOrder(ReadOrder { order, res }))
            .await;
        rx.await.map_err(|_| DbError::DbEngineDown)?
    }

    pub async fn mark_paid(&self, order: String) -> Result<OrderInfo, DbError> {
        let (res, rx) = oneshot::channel();
        let _unused = self
            .tx
            .send(DbRequest::MarkPaid(MarkPaid { order, res }))
            .await;
        rx.await.map_err(|_| DbError::DbEngineDown)?
    }

    pub async fn mark_withdrawn(&self, order: String) -> Result<(), DbError> {
        let (res, rx) = oneshot::channel();
        let _unused = self
            .tx
            .send(DbRequest::MarkWithdrawn(ModifyOrder { order, res }))
            .await;
        rx.await.map_err(|_| DbError::DbEngineDown)?
    }
    pub async fn mark_forced(&self, order: String) -> Result<(), DbError> {
        let (res, rx) = oneshot::channel();
        let _unused = self
            .tx
            .send(DbRequest::MarkForced(ModifyOrder { order, res }))
            .await;
        rx.await.map_err(|_| DbError::DbEngineDown)?
    }

    pub async fn mark_stuck(&self, order: String) -> Result<(), DbError> {
        let (res, rx) = oneshot::channel();
        let _unused = self
            .tx
            .send(DbRequest::MarkStuck(ModifyOrder { order, res }))
            .await;
        rx.await.map_err(|_| DbError::DbEngineDown)?
    }

    pub async fn shutdown(&self) {
        let (tx, rx) = oneshot::channel();
        let _unused = self.tx.send(DbRequest::Shutdown(tx)).await;
        let _ = rx.await;
    }
}

enum DbRequest {
    CreateOrder(CreateOrder),
    ActiveOrderList(oneshot::Sender<Result<Vec<(String, OrderInfo)>, DbError>>),
    ReadOrder(ReadOrder),
    MarkPaid(MarkPaid),
    MarkWithdrawn(ModifyOrder),
    MarkForced(ModifyOrder),
    MarkStuck(ModifyOrder),
    InitializeServerInfo(oneshot::Sender<Result<String, DbError>>),
    Shutdown(oneshot::Sender<()>),
}

pub struct CreateOrder {
    pub order: String,
    pub query: OrderQuery,
    pub currency: CurrencyInfo,
    pub payment_account: String,
    pub res: oneshot::Sender<Result<OrderCreateResponse, DbError>>,
}

pub struct ReadOrder {
    pub order: String,
    pub res: oneshot::Sender<Result<Option<OrderInfo>, DbError>>,
}

pub struct ModifyOrder {
    pub order: String,
    pub res: oneshot::Sender<Result<(), DbError>>,
}

pub struct MarkPaid {
    pub order: String,
    pub res: oneshot::Sender<Result<OrderInfo, DbError>>,
}

fn calculate_death_ts(account_lifetime: Timestamp) -> Timestamp {
    let start = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    Timestamp(start + account_lifetime.0)
}

fn create_order(
    order: String,
    query: OrderQuery,
    currency: CurrencyInfo,
    payment_account: String,
    orders: &sled::Tree,
    account_lifetime: Timestamp,
) -> Result<OrderCreateResponse, DbError> {
    let order_key = order.encode();
    Ok(if let Some(record) = orders.get(&order_key)? {
        let mut old_order_info = OrderInfo::decode(&mut &record[..])?;
        match old_order_info.payment_status {
            PaymentStatus::Pending => {
                let death = calculate_death_ts(account_lifetime);

                old_order_info.death = death;
                old_order_info.currency = currency;
                old_order_info.amount = query.amount;

                orders.insert(&order_key, old_order_info.encode())?;
                OrderCreateResponse::Modified(old_order_info)
            }
            PaymentStatus::Paid => OrderCreateResponse::Collision(old_order_info),
        }
    } else {
        let death = calculate_death_ts(account_lifetime);
        let order_info_new = OrderInfo::new(query, currency, payment_account, death);

        orders.insert(&order_key, order_info_new.encode())?;
        OrderCreateResponse::New(order_info_new)
    })
}

fn read_order(order: String, orders: &sled::Tree) -> Result<Option<OrderInfo>, DbError> {
    let order_key = order.encode();
    if let Some(order) = orders.get(order_key)? {
        Ok(Some(OrderInfo::decode(&mut &order[..])?))
    } else {
        Ok(None)
    }
}

fn mark_paid(order: String, orders: &sled::Tree) -> Result<OrderInfo, DbError> {
    let order_key = order.encode();
    if let Some(order_info) = orders.get(order_key)? {
        let mut order_info = OrderInfo::decode(&mut &order_info[..])?;
        if order_info.payment_status == PaymentStatus::Pending {
            order_info.payment_status = PaymentStatus::Paid;
            orders.insert(order.encode(), order_info.encode())?;
            Ok(order_info)
        } else {
            Err(DbError::AlreadyPaid(order))
        }
    } else {
        Err(DbError::OrderNotFound(order))
    }
}
fn mark_withdrawn(order: String, orders: &sled::Tree) -> Result<(), DbError> {
    let order_key = order.encode();
    if let Some(order_info) = orders.get(order_key)? {
        let mut order_info = OrderInfo::decode(&mut &order_info[..])?;
        if order_info.payment_status == PaymentStatus::Paid {
            if order_info.withdrawal_status == WithdrawalStatus::Waiting {
                order_info.withdrawal_status = WithdrawalStatus::Completed;
                orders.insert(order.encode(), order_info.encode())?;
                Ok(())
            } else {
                Err(DbError::WithdrawalWasAttempted(order))
            }
        } else {
            Err(DbError::NotPaid(order))
        }
    } else {
        Err(DbError::OrderNotFound(order))
    }
}

fn mark_forced(order: String, orders: &sled::Tree) -> Result<(), DbError> {
    let order_key = order.encode();
    if let Some(order_info) = orders.get(order_key)? {
        let mut order_info = OrderInfo::decode(&mut &order_info[..])?;
        if order_info.payment_status == PaymentStatus::Pending
            || order_info.payment_status == PaymentStatus::Paid
        {
            if order_info.withdrawal_status == WithdrawalStatus::Waiting {
                order_info.withdrawal_status = WithdrawalStatus::Forced;
                orders.insert(order.encode(), order_info.encode())?;
                Ok(())
            } else {
                Err(DbError::WithdrawalWasAttempted(order))
            }
        } else {
            Err(DbError::NotPaid(order))
        }
    } else {
        Err(DbError::OrderNotFound(order))
    }
}
fn mark_stuck(order: String, orders: &sled::Tree) -> Result<(), DbError> {
    if let Some(order_info) = orders.get(order.clone())? {
        let mut order_info = OrderInfo::decode(&mut &order_info[..])?;
        if order_info.payment_status == PaymentStatus::Paid {
            if order_info.withdrawal_status == WithdrawalStatus::Waiting {
                order_info.withdrawal_status = WithdrawalStatus::Failed;
                orders.insert(order.encode(), order_info.encode())?;
                Ok(())
            } else {
                Err(DbError::WithdrawalWasAttempted(order))
            }
        } else {
            Err(DbError::NotPaid(order))
        }
    } else {
        Err(DbError::OrderNotFound(order))
    }
}
