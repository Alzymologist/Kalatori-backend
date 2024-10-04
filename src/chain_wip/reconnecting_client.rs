//! The reconnecting JSON RPC WS client with the endpoint rotation support.

use crate::{database::definitions::Timestamp, error::PrettyCause};
use ahash::RandomState;
use indexmap::IndexSet;
use jsonrpsee::{
    core::{traits::ToRpcParams, ClientError as RpcError},
    ws_client::{WsClient, WsClientBuilder},
};
use rand::{rngs::ThreadRng, Rng};
use serde::de::DeserializeOwned;
use serde_json::value::RawValue;
use std::{iter, time::Duration};
use tokio::time;

pub struct ReconnectingClient {
    // client: WsClient,
    // endpoints: IndexSet<String, RandomState>,
    // current: usize,
}

// impl ReconnectingClient {
//     pub async fn new(
//         client: WsClient,
//         builder: WsClientBuilder,
//         endpoints: (String, Vec<String>),
//     ) -> Result<Self, ClientError> {
//         let mut map = IndexSet::with_capacity_and_hasher(endpoints.1.len(), RandomState::new());

//         for endpoint in endpoints.1 {
//             if !map.insert(endpoint) {
//                 return Err(ClientError::DuplicateEndpoints);
//             }
//         }

//         if map.contains(&endpoints.0) {
//             return Err(ClientError::DuplicateEndpoints);
//         }

//         todo!()
//     }

//     pub async fn request<R: DeserializeOwned>(&self, method: &str, parameters: impl ToRpcParams) -> Result<R, ClientError> {
//         let serialized_parameters = parameters.to_rpc_params().map_err(RpcError::ParseError)?.expect("shouldn't be `None`");

//         todo!()
//     }

//     pub async fn batch_request() {
//         todo!()
//     }

//     pub async fn subscribe() {
//         todo!()
//     }
// }

// // TODO: delete
// impl Drop for ReconnectingClient {
//     fn drop(&mut self) {
//         todo!()
//     }
// }

enum Action {
    Request(Box<RawValue>),
    BatchRequest,
    Subscribe,
}

enum Response {}

struct Processor {
    client: WsClient,
    builder: WsClientBuilder,
    endpoints: (String, IndexSet<String, RandomState>),
    current_endpoint: usize,
    random: ThreadRng,
}

impl Processor {
    fn new(
        client: WsClient,
        builder: WsClientBuilder,
        endpoints: (String, IndexSet<String, RandomState>),
    ) -> Self {
        Processor {
            client,
            builder,
            endpoints,
            current_endpoint: 1,
            random: rand::thread_rng(),
        }
    }

    async fn ignite(mut self) {
        // let d = self.client.request(method, params)

        loop {
            tokio::select! {
                biased;
                () = self.client.on_disconnect() => {
                    self.reconnect().await;
                }
            }
        }
    }

    async fn reconnect(&mut self) {
        tracing::info!(
            "The connection's been lost. Trying to reconnect with the first endpoint..."
        );

        match self.builder.clone().build(&self.endpoints.0).await {
            Ok(client) => {
                self.client = client;

                tracing::info!("Successfully restored the connection with the first endpoint.");

                return;
            }
            Err(error) => {
                tracing::error!(
                    "Failed to restore the connection with the first endpoint:{}",
                    error.pretty_cause()
                );
            }
        }

        let mut backoff = ExponentialBackoff::new(&mut self.random);
        let endpoints = self.endpoints.1.iter().chain(iter::once(&self.endpoints.0));

        loop {
            for endpoint in endpoints.clone() {
                tracing::info!(
                    "Will try to connect with {endpoint} on {}.",
                    Timestamp::from_duration_in_millis(backoff.jittered_duration.into())
                );

                backoff.sleep().await;

                match self.builder.clone().build(endpoint).await {
                    Ok(client) => {
                        self.client = client;

                        tracing::info!(
                            "Successfully established the connection with endpoint at {endpoint}."
                        );

                        return;
                    }
                    Err(error) => {
                        tracing::error!(
                            "Failed to establish the connection with endpoint at {endpoint}:{}",
                            error.pretty_cause()
                        );
                    }
                }
            }

            backoff.increase();
        }
    }
}

struct ExponentialBackoff<'a> {
    jittered_duration: u32,
    duration: u32,
    random: &'a mut ThreadRng,
}

impl ExponentialBackoff<'_> {
    const START: u32 = 16;
    // 1 day.
    const MAX: u32 = 24 * 60 * 60 * 1000;
    // 1: 16 ms.
    // 2: 32 ms.
    // 10: 1.024 s.
    // 16: 1.092267 min.
    // 22: 1.165084 h.
    // 26: 0.7767361 d.
    const BASE: u32 = 2;

    fn new(random: &mut ThreadRng) -> ExponentialBackoff<'_> {
        ExponentialBackoff {
            random,
            duration: Self::START,
            jittered_duration: Self::START,
        }
    }

    async fn sleep(&self) {
        time::sleep(Duration::from_millis(self.jittered_duration.into())).await;
    }

    fn increase(&mut self) {
        let duration = if self.duration >= Self::MAX {
            Self::MAX
        } else {
            self.duration = self.duration.saturating_mul(Self::BASE);

            self.duration
        };

        let min = (duration >> 1).saturating_add(1);
        let max = duration.saturating_mul(2);

        self.jittered_duration = self.random.gen_range(min..max);
    }
}
