//! A tracker that follows individual chain

use std::{collections::HashMap, sync::Arc, time::SystemTime};

use frame_metadata::v15::RuntimeMetadataV15;
use jsonrpsee::ws_client::{WsClient, WsClientBuilder};
use serde_json::Value;
use substrate_parser::{AsMetadata, ShortSpecs};
use tokio::{
    sync::mpsc,
    time::{timeout, Duration},
};
use tokio_util::sync::CancellationToken;

use crate::{
    arguments::Chain,
    chain::{
        definitions::{ChainTrackerRequest, Invoice},
        payout::payout,
        rpc::{
            assets_set_at_block, block_hash, genesis_hash, metadata, next_block, next_block_number,
            runtime_version_identifier, specs, subscribe_blocks, transfer_events,
        },
        utils::was_balance_received_at_account,
    },
    chain_wip::definitions::BlockHash,
    error::ChainError,
    server::definitions::api_v2::{CurrencyProperties, TokenKind},
    signer::Signer,
    state::State,
    utils::task_tracker::TaskTracker,
};

#[allow(clippy::too_many_lines)]
pub fn start_chain_watch(
    chain: Chain,
    chain_tx: mpsc::Sender<ChainTrackerRequest>,
    mut chain_rx: mpsc::Receiver<ChainTrackerRequest>,
    state: State,
    signer: Arc<Signer>,
    task_tracker: TaskTracker,
    cancellation_token: CancellationToken,
) {
    task_tracker
        .clone()
        .spawn(format!("Chain {} watcher", chain.name.clone().0), async move {
            let watchdog = 120000;
            let mut watched_accounts = HashMap::new();
            let mut shutdown = false;
            // TODO: random pick instead
            for endpoint in chain.config.endpoints.iter().cycle() {
                // not restarting chain if shutdown is in progress
                if shutdown || cancellation_token.is_cancelled() {
                    break;
                }
                if let Ok(client) = WsClientBuilder::default().build(&endpoint.0).await {
                    // prepare chain
                    let watcher = match ChainWatcher::prepare_chain(&client, chain.clone(), &mut watched_accounts, &endpoint.0, chain_tx.clone(), state.interface(), task_tracker.clone())
                        .await
                    {
                        Ok(a) => a,
                        Err(e) => {
                            tracing::warn!(
                                "Failed to connect to chain {}, due to {} switching RPC server...",
                                chain.name.0,
                                e
                            );
                            continue;
                        }
                    };


                    // fulfill requests
                    while let Ok(Some(request)) =
                        timeout(Duration::from_millis(watchdog), chain_rx.recv()).await
                    {
                        match request {
                            ChainTrackerRequest::NewBlock(block) => {
                                // TODO: hide this under rpc module
                                let block = match block_hash(&client, Some(block)).await {
                                    Ok(a) => a,
                                    Err(e) => {
                                        tracing::info!(
                                            "Failed to receive block in chain {}, due to {}; Switching RPC server...",
                                            chain.name.0,
                                            e
                                        );
                                        break;
                                    },
                                };

                                tracing::debug!("Block hash {} from {}", block.0, chain.name.0);

                                if watcher.version != runtime_version_identifier(&client, &block).await? {
                                    tracing::info!("Different runtime version reported! Restarting connection...");
                                    break;
                                }

                                match transfer_events(
                                    &client,
                                    &block,
                                    &watcher.metadata,
                                )
                                    .await {
                                        Ok(events) => {
                                        let mut id_remove_list = Vec::new();
                                        let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis() as u64;

                                        for (id, invoice) in &watched_accounts {
                                            if events.iter().any(|event| was_balance_received_at_account(&invoice.address, &event.0.fields)) {
                                                match invoice.check(&client, &watcher, &block).await {
                                                    Ok(true) => {
                                                        tracing::debug!("order paid");
                                                        state.order_paid(id.clone()).await;
                                                        id_remove_list.push(id.to_owned());
                                                    }
                                                    Ok(false) => (),
                                                    Err(e) => {
                                                        tracing::warn!("account fetch error: {0:?}", e);
                                                    }
                                                }
                                            } else if invoice.death.as_millis() <= now {
                                                match invoice.check(&client, &watcher, &block).await {
                                                    Ok(paid) => {
                                                        if paid {
                                                            state.order_paid(id.clone()).await;
                                                        }

                                                        id_remove_list.push(id.to_owned());
                                                    }
                                                    Err(e) => {
                                                        tracing::warn!("account fetch error: {0:?}", e);
                                                    }
                                                }
                                            }
                                        }
                                        for id in id_remove_list {
                                            tracing::debug!("removing {id}");
                                            watched_accounts.remove(&id);
                                        }
                                    },
                                    Err(e) => {
                                        tracing::warn!("Events fetch error {e} at {}", chain.name.0);
                                        break;
                                    },
                                    }
                                tracing::debug!("Block {} from {} processed successfully", block.0, chain.name.0);
                            }
                            ChainTrackerRequest::WatchAccount(request) => {
                                watched_accounts.insert(request.id.clone(), Invoice::from_request(request));
                            }
                            ChainTrackerRequest::Reap(request) => {
                                let id = request.id.clone();
                                let rpc = endpoint.clone();
                                let reap_state_handle = state.interface();
                                let watcher_for_reaper = watcher.clone();
                                let signer_for_reaper = signer.clone();
                                task_tracker.clone().spawn(format!("Initiate payout for order {}", id.clone()), async move {
                                    let result = payout(rpc.0.as_ref().to_owned(), Invoice::from_request(request), reap_state_handle, watcher_for_reaper, signer_for_reaper).await;
                                    if let Err(er) = result {
                                        tracing::error!("{:?}", er);
                                    }
                                    Ok(format!("Payout attempt for order {id} terminated"))
                                });
                            }
                            ChainTrackerRequest::Shutdown(res) => {
                                shutdown = true;
                                let _ = res.send(());
                                break;
                            }
                        }
                    }
                }
            }
            Ok(format!("Chain {} monitor shut down", chain.name.0))
        });
}

#[derive(Clone)]
pub struct ChainWatcher {
    pub genesis_hash: BlockHash,
    pub metadata: RuntimeMetadataV15,
    pub specs: ShortSpecs,
    pub assets: HashMap<String, CurrencyProperties>,
    version: Value,
}

impl ChainWatcher {
    #[allow(clippy::too_many_lines)]
    pub async fn prepare_chain(
        client: &WsClient,
        chain: Chain,
        watched_accounts: &mut HashMap<String, Invoice>,
        rpc_url: &str,
        chain_tx: mpsc::Sender<ChainTrackerRequest>,
        state: State,
        task_tracker: TaskTracker,
    ) -> Result<Self, ChainError> {
        let genesis_hash = genesis_hash(&client).await?;
        let mut blocks = subscribe_blocks(&client).await?;
        let block = next_block(client, &mut blocks).await?;
        let version = runtime_version_identifier(client, &block).await?;
        let metadata = metadata(&client, &block).await?;
        let name = <RuntimeMetadataV15 as AsMetadata<()>>::spec_name_version(&metadata)?.spec_name;
        if *name != *chain.name.0 {
            return Err(ChainError::WrongNetwork {
                expected: chain.name.0.to_string(),
                actual: name,
                rpc: rpc_url.to_string(),
            });
        };
        let specs = specs(&client, &metadata, &block).await?;
        let mut assets =
            assets_set_at_block(&client, &block, &metadata, rpc_url, specs.clone()).await?;

        // TODO: make this verbosity less annoying
        tracing::info!(
            "chain {} requires native token {:?} and {:?}",
            &chain.name.0,
            &chain.config.inner.native,
            &chain.config.inner.asset
        );

        // Remove unwanted assets

        assets = assets
            .into_iter()
            .filter_map(|(name, properties)| {
                tracing::info!(
                    "chain {} has token {} with properties {:?}",
                    &chain.name.0,
                    &name,
                    &properties
                );

                chain
                    .config
                    .inner
                    .asset
                    .iter()
                    .find(|a| Some(a.id.0) == properties.asset_id)
                    .map(|a| (a.name.clone(), properties))
            })
            .collect();

        if let Some(native_token) = chain.config.inner.native.clone() {
            if native_token.decimals.0 == specs.decimals {
                assets.insert(
                    native_token.name,
                    CurrencyProperties {
                        chain_name: <RuntimeMetadataV15 as AsMetadata<()>>::spec_name_version(
                            &metadata,
                        )?
                        .spec_name,
                        kind: TokenKind::Native,
                        decimals: specs.decimals,
                        rpc_url: rpc_url.to_owned(),
                        asset_id: None,
                        ss58: 0,
                    },
                );
            }
        }

        // Deduplication is done on chain manager level;
        // Check that we have same number of assets as requested (we've checked that we have only
        // wanted ones and performed deduplication before)
        //
        // This is probably an optimisation, but I don't have time to analyse perfirmance right
        // now, it's just simpler to implement
        //
        // TODO: maybe check if at least one endpoint responds with proper assets and if not, shut
        // down
        if assets.len()
            != chain.config.inner.asset.len() + usize::from(chain.config.inner.native.is_some())
        {
            return Err(ChainError::AssetsInvalid(chain.name.0.to_string()));
        }
        // this MUST assert that assets match exactly before reporting it

        state.connect_chain(assets.clone()).await;

        let chain = ChainWatcher {
            genesis_hash,
            metadata,
            specs,
            assets,
            version,
        };

        // check monitored accounts
        let mut id_remove_list = Vec::new();
        for (id, account) in watched_accounts.iter() {
            match account.check(client, &chain, &block).await {
                Ok(true) => {
                    state.order_paid(id.clone()).await;
                    id_remove_list.push(id.to_owned());
                }
                Ok(false) => (),
                Err(e) => {
                    tracing::warn!("account fetch error: {0}", e);
                }
            }
        }
        for id in id_remove_list {
            watched_accounts.remove(&id);
        }

        let rpc = rpc_url.to_string();
        task_tracker.spawn(format!("watching blocks at {rpc}"), async move {
            loop {
                match next_block_number(&mut blocks).await {
                    Ok(block) => {
                        tracing::debug!("received block {block} from {rpc}");
                        if let Err(e) = chain_tx.send(ChainTrackerRequest::NewBlock(block)).await {
                            tracing::warn!(
                                "Block watch internal communication error: {e} at {rpc}"
                            );
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::warn! {"Block watch error: {e} at {rpc}"};
                        break;
                    }
                }
            }
            // this should reset chain monitor on timeout;
            // but if this breaks, it means that the latter is already down either way
            Ok(format!("Block watch at {rpc} stopped"))
        });

        Ok(chain)
    }
}
