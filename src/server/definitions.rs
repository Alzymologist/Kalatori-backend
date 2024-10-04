//! Server definitions.
//!
//! <https://alzymologist.github.io/kalatori-api>

use serde::Deserialize;
use std::ops::{Deref, Sub};

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Balance(pub u128);

impl Deref for Balance {
    type Target = u128;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Sub for Balance {
    type Output = Self;

    fn sub(self, r: Self) -> Self {
        Balance(self.0 - r.0)
    }
}

impl Balance {
    #[allow(dead_code)] // TODO: remove once populated
    pub fn format(&self, decimals: api_v2::Decimals) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        let float = **self as f64;

        float / decimal_exponent_product(decimals)
    }

    pub fn parse(float: f64, decimals: api_v2::Decimals) -> Self {
        let parsed_float = (float * decimal_exponent_product(decimals)).round();

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Self(parsed_float as _)
    }
}

pub fn decimal_exponent_product(decimals: api_v2::Decimals) -> f64 {
    10f64.powi(decimals.into())
}

/// Self-sufficient schemas used by Api v2.0.0
pub mod api_v2 {
    use std::collections::HashMap;

    use crate::database::definitions::Timestamp;
    use codec::{Decode, Encode};
    use serde::{Deserialize, Serialize, Serializer};

    pub const AMOUNT: &str = "amount";
    pub const CURRENCY: &str = "currency";
    pub type AssetId = u32;
    pub type Decimals = u8;
    pub type BlockNumber = u64;
    pub type ExtrinsicIndex = u32;

    #[derive(Debug, Serialize)]
    pub struct InvalidParameter {
        pub parameter: String,
        pub message: String,
    }

    #[derive(Debug)]
    pub struct OrderQuery {
        pub order: String,
        pub amount: f64,
        pub callback: String,
        pub currency: String,
    }

    #[derive(Debug)]
    pub enum OrderResponse {
        NewOrder(OrderStatus),
        FoundOrder(OrderStatus),
        ModifiedOrder(OrderStatus),
        CollidedOrder(OrderStatus),
        NotFound,
    }

    #[derive(Debug, Serialize)]
    pub struct OrderStatus {
        pub order: String,
        pub message: String,
        pub recipient: String,
        pub server_info: ServerInfo,
        #[serde(flatten)]
        pub order_info: OrderInfo,
        pub payment_page: String,
        pub redirect_url: String,
    }

    #[derive(Clone, Debug, Serialize, Encode, Decode)]
    pub struct OrderInfo {
        pub withdrawal_status: WithdrawalStatus,
        pub payment_status: PaymentStatus,
        pub amount: f64,
        pub currency: CurrencyInfo,
        pub callback: String,
        pub transactions: Vec<TransactionInfo>,
        pub payment_account: String,
        pub death: Timestamp,
    }

    impl OrderInfo {
        pub fn new(
            query: OrderQuery,
            currency: CurrencyInfo,
            payment_account: String,
            death: Timestamp,
        ) -> Self {
            OrderInfo {
                withdrawal_status: WithdrawalStatus::Waiting,
                payment_status: PaymentStatus::Pending,
                amount: query.amount,
                currency,
                callback: query.callback,
                transactions: Vec::new(),
                payment_account,
                death,
            }
        }
    }

    pub enum OrderCreateResponse {
        New(OrderInfo),
        Modified(OrderInfo),
        Collision(OrderInfo),
    }

    #[derive(Clone, Debug, Serialize, Decode, Encode, PartialEq)]
    #[serde(rename_all = "lowercase")]
    pub enum PaymentStatus {
        Pending,
        Paid,
        TimedOut,
    }

    #[derive(Clone, Debug, Serialize, Decode, Encode, PartialEq)]
    #[serde(rename_all = "lowercase")]
    pub enum WithdrawalStatus {
        Waiting,
        Failed,
        Completed,
        None,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct ServerStatus {
        pub description: String,
        pub server_info: ServerInfo,
        pub supported_currencies: HashMap<String, CurrencyProperties>,
    }

    #[allow(dead_code)] // TODO: Use this for health response?
    #[derive(Debug, Serialize)]
    struct ServerHealth {
        server_info: ServerInfo,
        connected_rpcs: Vec<RpcInfo>,
        status: Health,
    }

    #[derive(Debug, Serialize)]
    struct RpcInfo {
        rpc_url: String,
        chain_name: String,
        status: Health,
    }

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "lowercase")]
    enum Health {
        Ok,
        Degraded,
        Critical,
    }

    #[derive(Clone, Debug, Serialize, Decode, Encode)]
    pub struct CurrencyInfo {
        pub currency: String,
        pub chain_name: String,
        pub kind: TokenKind,
        pub decimals: Decimals,
        pub rpc_url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub asset_id: Option<AssetId>,
    }

    impl CurrencyInfo {
        pub fn properties(&self) -> CurrencyProperties {
            CurrencyProperties {
                chain_name: self.chain_name.clone(),
                kind: self.kind,
                decimals: self.decimals,
                rpc_url: self.rpc_url.clone(),
                asset_id: self.asset_id,
                ss58: 0,
            }
        }
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct CurrencyProperties {
        pub chain_name: String,
        pub kind: TokenKind,
        pub decimals: Decimals,
        pub rpc_url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub asset_id: Option<AssetId>,
        // #[serde(skip_serializing)]
        pub ss58: u16,
    }

    impl CurrencyProperties {
        pub fn info(&self, currency: String) -> CurrencyInfo {
            CurrencyInfo {
                currency,
                chain_name: self.chain_name.clone(),
                kind: self.kind,
                decimals: self.decimals,
                rpc_url: self.rpc_url.clone(),
                asset_id: self.asset_id,
            }
        }
    }

    #[derive(Clone, Copy, Debug, Serialize, Decode, Encode, Deserialize, PartialEq)]
    #[serde(rename_all = "lowercase")]
    pub enum TokenKind {
        Asset,
        Native,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct ServerInfo {
        pub version: String,
        pub instance_id: String,
        pub debug: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub kalatori_remark: Option<String>,
    }

    #[derive(Clone, Debug, Serialize, Decode, Encode)]
    pub struct TransactionInfo {
        #[serde(skip_serializing_if = "Option::is_none", flatten)]
        finalized_tx: Option<FinalizedTx>, // Clearly undefined in v2.1 - TODO
        transaction_bytes: String,
        sender: String,
        recipient: String,
        #[serde(serialize_with = "amount_serializer")]
        amount: Amount,
        currency: CurrencyInfo,
        status: TxStatus,
    }

    #[derive(Clone, Debug, Serialize, Decode, Encode)]
    struct FinalizedTx {
        block_number: BlockNumber,
        position_in_block: ExtrinsicIndex,
        timestamp: String,
    }

    #[derive(Clone, Debug, Decode, Encode)]
    enum Amount {
        All,
        Exact(f64),
    }

    fn amount_serializer<S: Serializer>(amount: &Amount, serializer: S) -> Result<S::Ok, S::Error> {
        match amount {
            Amount::All => serializer.serialize_str("all"),
            Amount::Exact(exact) => exact.serialize(serializer),
        }
    }

    #[derive(Clone, Debug, Serialize, Decode, Encode)]
    #[serde(rename_all = "lowercase")]
    enum TxStatus {
        Pending,
        Finalized,
        Failed,
    }
}

pub mod new {
    use crate::{
        arguments::ChainName,
        chain_wip::definitions::Url,
        database::definitions::{
            Amount, AmountKind as DbAmountKind, AssetId, Bytes, CurrencyInfo as DbCurrencyInfo,
            FinalizedTx, OrderInfo as DbOrderInfo, PaymentStatus, Timestamp, TokenKind,
            TransactionInfo as DbTransactionInfo, TxStatus, Url as DbUrl, WithdrawalStatus,
        },
    };
    use ahash::HashMap;
    use serde::{Deserialize, Serialize, Serializer};
    use std::sync::Arc;
    use substrate_crypto_light::common::{AccountId32, AsBase58};

    #[derive(Serialize, Deserialize, Clone, Copy, Debug)]
    pub struct Decimals(pub u8);

    #[derive(Clone, Copy)]
    pub struct SS58Prefix(pub u16);

    #[derive(Clone, Copy)]
    pub struct SubstrateAccount(pub SS58Prefix, pub AccountId32);

    impl Serialize for SubstrateAccount {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.serialize_str(&self.1.to_base58_string(self.0 .0))
        }
    }

    pub enum CreatedOrder {
        New(OrderStatus),
        Modified(OrderStatus),
        Unchanged(OrderStatus),
    }

    #[derive(Serialize)]
    pub struct OrderStatus {
        pub order: String,
        pub recipient: SubstrateAccount,
        pub server_info: ServerInfo,
        #[serde(flatten)]
        pub order_info: OrderInfo,
        pub payment_page: &'static str,
        pub redirect_url: &'static str,
    }

    #[derive(Serialize)]
    pub struct OrderInfo {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub message: Option<String>,
        pub withdrawal_status: WithdrawalStatus,
        pub payment_status: PaymentStatus,
        pub amount: f64,
        pub currency: CurrencyInfo,
        pub callback: DbUrl,
        pub transactions: Vec<TransactionInfo>,
        pub payment_account: SubstrateAccount,
        pub death: Timestamp,
    }

    impl OrderInfo {
        fn from_db(
            db: DbOrderInfo,
            prefix: SS58Prefix,
            decimals: Decimals,
            currency_name: Arc<str>,
            db_currency: DbCurrencyInfo,
            rpc_url: Url,
            chain_name: ChainName,
        ) -> Self {
            let currency = CurrencyInfo {
                currency: currency_name,
                properties: CurrencyProperties::from_db(db_currency, chain_name, decimals, rpc_url),
            };

            Self {
                message: db.message,
                withdrawal_status: db.withdrawal_status,
                payment_status: db.payment_status,
                amount: db.amount.format(decimals),
                callback: db.callback,
                transactions: db
                    .transactions
                    .into_iter()
                    .map(|tx| TransactionInfo::from_db(tx, currency.clone(), decimals, prefix))
                    .collect(),
                currency,
                payment_account: SubstrateAccount(prefix, db.payment_account.into()),
                death: db.death,
            }
        }
    }

    #[derive(Debug, Deserialize)]
    pub struct OrderPayloadRaw {
        pub amount: f64,
        pub currency: String,
        pub callback: DbUrl,
    }

    pub struct OrderPayload {
        pub amount: Amount,
        pub currency: String,
        pub callback: DbUrl,
    }

    impl OrderPayload {
        pub fn from_raw(raw: OrderPayloadRaw, decimals: Decimals) -> Self {
            Self {
                amount: Amount::parse(raw.amount, decimals),
                currency: raw.currency,
                callback: raw.callback,
            }
        }
    }

    #[derive(Serialize)]
    pub struct ServerStatus {
        pub description: &'static str,
        pub server_info: ServerInfo,
        pub supported_currencies: HashMap<String, CurrencyProperties>,
    }

    #[derive(Serialize)]
    struct ServerHealth {
        server_info: ServerInfo,
        connected_rpcs: Vec<RpcInfo>,
        status: Health,
    }

    #[derive(Serialize)]
    struct RpcInfo {
        rpc_url: Url,
        chain_name: ChainName,
        status: Health,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "lowercase")]
    enum Health {
        Ok,
        Degraded,
        Critical,
    }

    #[derive(Serialize, Clone)]
    pub struct CurrencyInfo {
        pub currency: Arc<str>,
        #[serde(flatten)]
        pub properties: CurrencyProperties,
    }

    #[derive(Serialize, Clone)]
    pub struct CurrencyProperties {
        pub chain_name: ChainName,
        pub kind: TokenKind,
        pub decimals: Decimals,
        pub rpc_url: Url,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub asset_id: Option<AssetId>,
    }

    impl CurrencyProperties {
        pub fn from_db(
            db: DbCurrencyInfo,
            chain_name: ChainName,
            decimals: Decimals,
            rpc_url: Url,
        ) -> Self {
            Self {
                chain_name,
                kind: db.kind,
                decimals,
                rpc_url,
                asset_id: db.asset_id,
            }
        }
    }

    #[derive(Serialize)]
    pub struct ServerInfo {
        pub version: &'static str,
        pub instance_id: Arc<str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub debug: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub kalatori_remark: Option<Arc<str>>,
    }

    #[derive(Serialize)]
    pub struct TransactionInfo {
        #[serde(skip_serializing_if = "Option::is_none", flatten)]
        // TODO: Define this field more clearly in API.
        finalized_tx: Option<FinalizedTx>,
        transaction_bytes: Bytes,
        sender: SubstrateAccount,
        recipient: SubstrateAccount,
        #[serde(serialize_with = "amount_serializer")]
        amount: AmountKind,
        currency: CurrencyInfo,
        status: TxStatus,
    }

    impl TransactionInfo {
        pub fn from_db(
            db: DbTransactionInfo,
            currency: CurrencyInfo,
            decimals: Decimals,
            prefix: SS58Prefix,
        ) -> Self {
            Self {
                finalized_tx: db.finalized_tx,
                transaction_bytes: db.transaction_bytes,
                sender: SubstrateAccount(prefix, db.sender.into()),
                recipient: SubstrateAccount(prefix, db.recipient.into()),
                amount: AmountKind::from_db(db.amount, decimals),
                currency,
                status: db.status,
            }
        }
    }

    #[derive(Serialize)]
    enum AmountKind {
        All,
        Exact(f64),
    }

    impl AmountKind {
        fn from_db(db: DbAmountKind, decimals: Decimals) -> Self {
            match db {
                DbAmountKind::Exact(amount) => Self::Exact(amount.format(decimals)),
                DbAmountKind::All => Self::All,
            }
        }
    }

    fn amount_serializer<S: Serializer>(
        amount: &AmountKind,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match amount {
            AmountKind::All => serializer.serialize_str("all"),
            AmountKind::Exact(exact) => exact.serialize(serializer),
        }
    }
}
