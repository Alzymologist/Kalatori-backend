use crate::{
    callback,
    chain::ChainManager,
    database::{definitions::Timestamp, Database, OrdersReadable},
    error::{Error, OrderError},
    server::definitions::api_v2::{
        CurrencyProperties, OrderCreateResponse, OrderInfo, OrderQuery, OrderResponse, OrderStatus,
        ServerInfo, ServerStatus,
    },
    signer::Signer,
    utils::task_tracker::TaskTracker,
};
use std::{collections::HashMap, sync::Arc};
use substrate_crypto_light::common::{AccountId32, AsBase58};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

/// Struct to store state of daemon. If something requires cooperation of more than one component,
/// it should go through here.
#[derive(Clone, Debug)]
pub struct State {
    tx: tokio::sync::mpsc::Sender<StateAccessRequest>,
}

impl State {
    pub fn initialise(
        signer: Arc<Signer>,
        debug: Option<bool>,
        remark: Option<String>,
        db: Arc<Database>,
        chain_manager: oneshot::Receiver<ChainManager>,
        task_tracker: TaskTracker,
        shutdown_notification: CancellationToken,
        recipient: AccountId32,
        account_lifetime: Timestamp,
    ) -> Self {
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

        let server_info = ServerInfo {
            // TODO
            version: env!("CARGO_PKG_VERSION").to_string(),
            debug: debug.unwrap_or_default(),
            kalatori_remark: remark.clone(),
            instance_id: db.instance().to_string(),
        };

        // Remember to always spawn async here or things might deadlock
        task_tracker.clone().spawn("State Handler", async move {
            let chain_manager = chain_manager.await.map_err(|_| Error::Fatal)?;
            let db_wakeup = db.clone();
            let chain_manager_wakeup = chain_manager.clone();
            let currencies = HashMap::new();
            let mut state = StateData {
                currencies,
                server_info,
                db,
                chain_manager,
                signer,
                recipient,
                account_lifetime,
            };


            // TODO: consider doing this even more lazy
            task_tracker.spawn("Restore saved orders", async move {
                if let Some(orders) = db_wakeup.read()?.orders()? {
                    let order_list = orders.active()?;

                    for result in order_list {
                            let (order, order_details) = result?;

                            // chain_manager_wakeup
                            //     .add_invoice(order.value(), order_details, recipient)
                            //     .await?;
                            todo!()
                    }

                    Ok("All saved orders restored")
                } else {
                    Ok("")
                }
            });

            loop {
                tokio::select! {
                    biased;
                    request_option = rx.recv() => {
                        let Some(request) = request_option else {
                            break;
                        };

                        match request {
                            StateAccessRequest::ConnectChain(assets) => {
                                // it MUST be asserted in chain tracker that assets are those and only
                                // those that user requested
                                state.update_currencies(assets);
                            }
                            StateAccessRequest::GetInvoiceStatus(request) => {
                                request
                                    .res
                                    .send(state.get_invoice_status(request.order))
                                    .map_err(|_| Error::Fatal)?;
                            }
                            StateAccessRequest::CreateInvoice(request) => {
                                request
                                    .res
                                    .send(state.create_invoice(request.order_query).await)
                                    .map_err(|_| Error::Fatal)?;
                            }
                            StateAccessRequest::ServerStatus(res) => {
                                let server_status = ServerStatus {
                                    description: "tododododododo".into(),
                                    server_info: state.server_info.clone(),
                                    supported_currencies: state.currencies.clone(),
                                };
                                res.send(server_status).map_err(|_| Error::Fatal)?;
                            }
                            StateAccessRequest::OrderPaid(id) => {
                                // Only perform actions if the record is saved in ledger
                                match state.db.write(|tx| tx.orders()?.mark_paid(&id)) {
                                    Ok(order) => {
                                        // TODO: callback here
                                        // callback::callback(&order.callback, OrderStatus {
                                        //     order: id.clone(),
                                        //     message: String::new(),
                                        //     recipient: state.recipient.clone().to_base58_string(42),
                                        //     server_info: state.server_info.clone(),
                                        //     order_info: order.clone(),
                                        //     payment_page: String::new(),
                                        //     redirect_url: String::new(),
                                        // }).await;
                                        // drop(state.chain_manager.reap(id, order, state.recipient).await);
                                        todo!()
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            "Order was paid but this could not be recorded! {e:?}"
                                        );
                                    }
                                }
                            }
                            StateAccessRequest::ForceWithdrawal { order, res } => {
                                res.send(state.force_withdrawal(order).await).map_err(|_| Error::Fatal)?;
                            }
                        };
                    }
                    // Orchestrate shutdown from here
                    () = shutdown_notification.cancelled() => {
                        // Web server shuts down on its own; it does not matter what it sends now.

                        // First shut down active actions for external world. If something yet
                        // happens, we should record it in db.
                        state.chain_manager.shutdown().await;

                        // And shut down finally
                        break;
                    }
                }
            }

            Ok("State handler is shutting down")
        });

        Self { tx }
    }

    pub async fn connect_chain(&self, assets: HashMap<String, CurrencyProperties>) {
        self.tx.send(StateAccessRequest::ConnectChain(assets)).await;
    }

    pub async fn order_status(&self, order: &str) -> Result<OrderResponse, Error> {
        let (res, rx) = oneshot::channel();
        self.tx
            .send(StateAccessRequest::GetInvoiceStatus(GetInvoiceStatus {
                order: order.to_string(),
                res,
            }))
            .await
            .map_err(|_| Error::Fatal)?;
        rx.await.map_err(|_| Error::Fatal)?
    }

    pub async fn server_status(&self) -> Result<ServerStatus, Error> {
        let (res, rx) = oneshot::channel();
        self.tx
            .send(StateAccessRequest::ServerStatus(res))
            .await
            .map_err(|_| Error::Fatal)?;
        rx.await.map_err(|_| Error::Fatal)
    }

    pub async fn create_order(&self, order_query: OrderQuery) -> Result<OrderResponse, Error> {
        let (res, rx) = oneshot::channel();
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
            .await
            .map_err(|_| Error::Fatal)?;
        rx.await.map_err(|_| Error::Fatal)?
    }

    pub async fn order_paid(&self, order: String) {
        if self
            .tx
            .send(StateAccessRequest::OrderPaid(order))
            .await
            .is_err()
        {
            tracing::warn!("Data race on shutdown; please restart the daemon for cleaning up");
        };
    }

    #[allow(dead_code)]
    pub async fn force_withdrawal(&self, order: String) -> Result<OrderStatus, OrderStatus> {
        todo!()
    }

    pub fn interface(&self) -> Self {
        State {
            tx: self.tx.clone(),
        }
    }
}

enum StateAccessRequest {
    ConnectChain(HashMap<String, CurrencyProperties>),
    GetInvoiceStatus(GetInvoiceStatus),
    CreateInvoice(CreateInvoice),
    ServerStatus(oneshot::Sender<ServerStatus>),
    OrderPaid(String),
    ForceWithdrawal {
        order: String,
        res: oneshot::Sender<Result<Option<OrderStatus>, Error>>,
    },
}

struct GetInvoiceStatus {
    pub order: String,
    pub res: oneshot::Sender<Result<OrderResponse, Error>>,
}

struct CreateInvoice {
    pub order_query: OrderQuery,
    pub res: oneshot::Sender<Result<OrderResponse, Error>>,
}

struct StateData {
    currencies: HashMap<String, CurrencyProperties>,
    server_info: ServerInfo,
    db: Arc<Database>,
    chain_manager: ChainManager,
    signer: Arc<Signer>,
    recipient: AccountId32,
    account_lifetime: Timestamp,
}

impl StateData {
    async fn force_withdrawal(&mut self, order: String) -> Result<Option<OrderStatus>, Error> {
        // let Some(orders) = self.db.read()?.orders()? else {
        //     return Ok(None);
        // };

        // let Some(order_info) = orders.read(&order)? else {
        //     return Ok(None);
        // };

        // self.chain_manager
        //     .reap(order.clone(), order_info.clone(), self.recipient)
        //     .await?;

        // Ok(Some(OrderStatus {
        //     order,
        //     message: String::new(),
        //     recipient: self.recipient.clone().to_base58_string(42),
        //     server_info: self.server_info.clone(),
        //     order_info,
        //     payment_page: String::new(),
        //     redirect_url: String::new(),
        // }))

        todo!()
    }

    fn update_currencies(&mut self, currencies: HashMap<String, CurrencyProperties>) {
        self.currencies.extend(currencies);
    }

    fn get_invoice_status(&self, order: String) -> Result<OrderResponse, Error> {
        // let Some(orders) = self.db.read()?.orders()? else {
        //     return Ok(OrderResponse::NotFound);
        // };

        // if let Some(order_info) = orders.read_order(&order)? {
        //     let message = String::new(); //TODO
        //     Ok(OrderResponse::FoundOrder(OrderStatus {
        //         order,
        //         message,
        //         recipient: self.recipient.clone().to_base58_string(2), // TODO maybe but spec says use "2"
        //         server_info: self.server_info.clone(),
        //         order_info,
        //         payment_page: String::new(),
        //         redirect_url: String::new(),
        //     }))
        // } else {
        //     Ok(OrderResponse::NotFound)
        // }
        todo!()
    }

    async fn create_invoice(&self, order_query: OrderQuery) -> Result<OrderResponse, Error> {
        // let order = order_query.order.clone();
        // tracing::debug!("creating order {order_query:?}");
        // let currency = self
        //     .currencies
        //     .get(&order_query.currency)
        //     .ok_or(OrderError::UnknownCurrency)?;
        // let currency = currency.info(order_query.currency.clone());
        // let payment_account = self.signer.construct_order_account(&order)?;
        // match self.db.write(|tx| {
        //     tx.orders()?.create_order(
        //         &order,
        //         order_query,
        //         currency,
        //         payment_account,
        //         self.account_lifetime,
        //     )
        // })? {
        //     OrderCreateResponse::New(new_order_info) => {
        //         self.chain_manager
        //             .add_invoice(order.clone(), new_order_info.clone(), self.recipient)
        //             .await?;
        //         Ok(OrderResponse::NewOrder(self.order_status(
        //             order,
        //             new_order_info,
        //             String::new(),
        //         )))
        //     }
        //     OrderCreateResponse::Modified(order_info) => Ok(OrderResponse::ModifiedOrder(
        //         self.order_status(order, order_info, String::new()),
        //     )),
        //     OrderCreateResponse::Collision(order_status) => {
        //         Ok(OrderResponse::CollidedOrder(self.order_status(
        //             order,
        //             order_status,
        //             String::from("Order with this ID was already processed"),
        //         )))
        //     }
        // }
        todo!()
    }

    fn order_status(&self, order: String, order_info: OrderInfo, message: String) -> OrderStatus {
        OrderStatus {
            order,
            message,
            recipient: self.recipient.clone().to_base58_string(2), // TODO maybe but spec says use "2"
            server_info: self.server_info.clone(),
            order_info,
            payment_page: String::new(),
            redirect_url: String::new(),
        }
    }
}
