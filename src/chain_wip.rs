//! Everything related to the actual interaction with a blockchain.

use crate::{
    arguments::{Chain, ChainIntervals, ChainName},
    error::{Error, TaskError},
    utils::task_tracker::ShortTaskTracker,
};
use ahash::RandomState;
use indexmap::{map::Entry, IndexMap, IndexSet};
use jsonrpsee::ws_client::{PingConfig, WsClientBuilder};
use std::sync::Arc;
use tokio::sync::{
    mpsc::{self, Sender as MpscSender},
    oneshot::{self, Sender as OsSender},
    RwLock,
};

pub mod definitions;

mod api;

use definitions::{ChainPreparator, ConnectedChain};

pub const MODULE: &str = module_path!();

pub async fn connect(
    chains: Vec<Chain>,
    root_intervals: ChainIntervals,
) -> Result<IndexMap<ChainName, ConnectedChain, RandomState>, Error> {
    let ri_shared = Arc::new(RwLock::const_new(root_intervals));
    let mut connected_chains = IndexMap::with_capacity_and_hasher(chains.len(), RandomState::new());
    let (connected_chains_tx, mut connected_chains_rx) = mpsc::channel::<(_, _, OsSender<_>)>(1);

    let connected_chains_jh = tokio::spawn(async move {
        while let Some((name, connected_chain, is_occupied)) = connected_chains_rx.recv().await {
            is_occupied
                .send(match connected_chains.entry(name) {
                    Entry::Occupied(_) => true,
                    Entry::Vacant(entry) => {
                        tracing::info!("Connected to the {:?} chain.", entry.key());

                        entry.insert(connected_chain);

                        false
                    }
                })
                .expect("receiver side should be alive");
        }

        connected_chains
    });

    let task_tracker = ShortTaskTracker::new();

    for chain in chains {
        task_tracker.spawn(
            ChainPreparator(chain.name.clone()),
            connect_to_chain(chain, connected_chains_tx.clone(), ri_shared.clone()),
        );
    }

    // Drop the sender before `task_tracker.try_wait()` to prevent a deadlock.
    drop(connected_chains_tx);

    task_tracker.try_wait().await?;

    Ok(connected_chains_jh
        .await
        .expect("this `JoinHandle` shouldn't panic"))
}

#[tracing::instrument(skip_all, fields(chain = ?chain.name))]
async fn connect_to_chain(
    chain: Chain,
    connected_chains: MpscSender<(ChainName, ConnectedChain, OsSender<bool>)>,
    root_intervals: Arc<RwLock<ChainIntervals>>,
) -> Result<(), TaskError> {
    chain
        .config
        .inner
        .intervals
        .check(&*root_intervals.read().await)?;

    let mut endpoints = chain.config.endpoints.into_iter();
    let first_endpoint = endpoints.next().ok_or(TaskError::NoChainEndpoints)?;
    let mut remaining_endpoints =
        IndexSet::with_capacity_and_hasher(endpoints.len(), RandomState::new());

    for endpoint in endpoints {
        if !remaining_endpoints.insert(endpoint) {
            return Err(TaskError::DuplicateEndpoints);
        }
    }

    if remaining_endpoints.contains(&first_endpoint) {
        return Err(TaskError::DuplicateEndpoints);
    }

    let client_config = WsClientBuilder::new().enable_ws_ping(PingConfig::new());
    let client = client_config.clone().build(&first_endpoint.0).await?;
    let genesis = api::fetch_genesis_hash(&client).await?;
    // let finalized_head = methods.get_finalized_head().await?;
    // let runtime = RuntimeApi::new(methods.get_metadata(&finalized_head).await?, methods)?;

    // if !chain.config.inner.asset.is_empty() {
    //     let assets = runtime.assets()?;

    //     for asset in &chain.config.inner.asset {
    //         assets.storage().asset(asset.id).await;
    //     }
    // }

    let (is_occupied_tx, is_occupied_rx) = oneshot::channel();

    connected_chains
        .send((
            chain.name,
            ConnectedChain {
                endpoints: (first_endpoint, remaining_endpoints),
                chain_config: chain.config.inner,
                genesis,
                client,
                client_config,
            },
            is_occupied_tx,
        ))
        .await
        .expect("receiver side should be open");

    if is_occupied_rx
        .await
        .expect("sender side should send a value")
    {
        Err(TaskError::ChainDuplicate)
    } else {
        Ok(())
    }
}

// #[allow(clippy::module_name_repetitions)]
// pub struct ChainManager;

// impl ChainManager {
//     pub async fn new(
//         database: Arc<Database>,
//         chains: IndexMap<Arc<str>, ConnectedChain, RandomState>,
//         root_intervals: ArgChainIntervals,
//     ) -> Result<Self, Error> {
//         let root_ci = RootChainIntervals::new(&root_intervals)?;
//         let task_tracker = ShortTaskTracker::new();

//         for chain in chains {
//             task_tracker.spawn(
//                 ChainPreparator(chain.0.clone()),
//                 prepare_chain(chain.0, chain.1, database.clone(), root_ci),
//             );
//         }

//         task_tracker.try_wait().await?;

//         todo!()
//     }
// }

// #[tracing::instrument(skip_all, fields(name))]
// async fn prepare_chain(
//     name: Arc<str>,
//     chain: ConnectedChain,
//     database: Arc<Database>,
//     root_ci: RootChainIntervals,
// ) -> Result<(), TaskError> {
//     let intervals = ChainIntervals::new(root_ci, &chain.config.intervals)?;
//     let finalized_head = api::fetch_finalized_head(&chain.rpc_client).await?;
//     let metadata = api::fetch_metadata(&chain.rpc_client, &finalized_head).await?;
//     let runtime_version: RuntimeVersion =
//         api::extract_constant(&metadata, SystemConstant::Version)?;

//     println!("{:#?}", runtime_version);

//     Ok(())
// }
