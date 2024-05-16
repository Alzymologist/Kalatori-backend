//! Separate engine for payout process.
//!
//! This is so unimportant for smooth SALES process, that it should be given the lowest possible
//! priority, optimized for lazy and very delayed process, and in some cases might be disabeled
//! altogether (TODO)

use crate::{
    chain::{
        definitions::Invoice,
        rpc::{block_hash, current_block_number, send_stuff},
        tracker::ChainWatcher,
        utils::{
            construct_batch_transaction, construct_single_asset_transfer_call,
            construct_single_balance_transfer_call, AssetTransferConstructor,
            BalanceTransferConstructor,
        },
    },
    definitions::api_v2::TokenKind,
    error::ErrorChain,
    signer::Signer,
    state::State,
};

use frame_metadata::v15::RuntimeMetadataV15;
use jsonrpsee::ws_client::WsClientBuilder;
use substrate_constructor::fill_prepare::{SpecialTypeToFill, TypeContentToFill};

/// Single function that should completely handle payout attmept. Just do not call anything else.
///
/// TODO: make this an additional runner independent from chain monitors
pub async fn payout(
    rpc: String,
    order: Invoice,
    state: State,
    chain: ChainWatcher,
    signer: Signer,
) -> Result<(), ErrorChain> {
    // TODO: make this retry and rotate RPCs maybe
    //
    // after some retries record a failure
    if let Ok(client) = WsClientBuilder::default().build(rpc).await {
        let block = block_hash(&client, None).await?; // TODO should retry instead
        let block_number = current_block_number(&client, &chain.metadata, &block).await?;
        let balance = order.balance(&client, &chain, &block).await?; // TODO same
        let loss_tolerance = 10000; // TODO: replace with multiple of existential
        let manual_intervention_amount = 1000000000000;
        let currency = chain
            .assets
            .get(&order.currency)
            .ok_or(ErrorChain::InvalidCurrency(order.currency))?;

        // Payout operation logic
        let transactions = match balance.0 - order.amount.0 {
            a if (0..=loss_tolerance).contains(&a) => match currency.kind {
                TokenKind::Balances => {
                    let balance_transfer_constructor = BalanceTransferConstructor {
                        amount: order.amount.0,
                        to_account: &order.recipient,
                        is_clearing: true,
                    };
                    vec![construct_single_balance_transfer_call(
                        &chain.metadata,
                        &balance_transfer_constructor,
                    )?]
                }
                TokenKind::Asset => {
                    let asset_transfer_constructor = AssetTransferConstructor {
                        asset_id: currency.asset_id.ok_or(ErrorChain::AssetId)?,
                        amount: order.amount.0,
                        to_account: &order.recipient,
                    };
                    vec![construct_single_asset_transfer_call(
                        &chain.metadata,
                        &asset_transfer_constructor,
                    )?]
                }
            },
            a if (loss_tolerance..=manual_intervention_amount).contains(&a) => {
                tracing::warn!("Overpayments not handled yet");
                return Ok(()); //TODO
            }
            _ => {
                tracing::error!("Balance is out of range: {balance:?}");
                return Ok(()); //TODO
            }
        };

        let mut batch_transaction = construct_batch_transaction(
            &chain.metadata,
            chain.genesis_hash,
            order.address,
            &transactions,
            block,
            block_number,
            0,
        )?;

        let sign_this = batch_transaction
            .sign_this()
            .ok_or(ErrorChain::TransactionNotSignable(format!(
                "{batch_transaction:?}"
            )))?;

        let signature = signer.sign(order.id, sign_this).await?;

        batch_transaction.signature.content =
            TypeContentToFill::SpecialType(SpecialTypeToFill::SignatureSr25519(Some(signature)));

        let extrinsic = batch_transaction
            .send_this_signed::<(), RuntimeMetadataV15>(&chain.metadata)?
            .ok_or(ErrorChain::NothingToSend)?;

        send_stuff(&client, &format!("0x{}", hex::encode(extrinsic))).await?;

        // TODO obvious
    }
    Ok(())
}
