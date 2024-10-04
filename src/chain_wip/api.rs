//! Substrate RPC API functions & calls.
//!
//! - <https://paritytech.github.io/json-rpc-interface-spec/api.html>
//! - <https://docs.rs/sc-rpc-api>

use crate::{chain_wip::definitions::BlockHash, error::ApiError};
use jsonrpsee::{core::client::ClientT, ws_client::WsClient};

pub async fn fetch_genesis_hash(client: &WsClient) -> Result<BlockHash, ApiError> {
    client
        .request("chain_getBlockHash", [0])
        .await
        .map_err(ApiError::GenesisHash)
}

// #[expect(clippy::module_name_repetitions)]
// pub struct ApiClient(pub WsClient);

// impl ApiClient {
//     pub async fn get_block_hash(&self) -> Result<BlockHash, ApiError> {
//         self.0
//             .request("chain_getBlockHash", [0])
//             .await
//             .map_err(ApiError::GenesisHash)
//     }

//     pub async fn get_finalized_head(&self) -> Result<BlockHash, ApiError> {
//         self.0
//             .request("chain_getFinalizedHead", jsonrpsee::rpc_params![])
//             .await
//             .map_err(ApiError::FinalizedHead)
//     }

//     pub async fn get_metadata(&self, block: &BlockHash) -> Result<RuntimeMetadataV14, ApiError> {
//         let bytes: Bytes = self
//             .0
//             .request("state_getMetadata", [block])
//             .await
//             .map_err(ApiError::FinalizedHead)?;
//         let prefixed =
//             RuntimeMetadataPrefixed::decode(&mut &*bytes).map_err(MetadataError::from)?;

//         if prefixed.0 != META_RESERVED {
//             return Err(MetadataError::UnexpectedPrefix(prefixed.0).into());
//         }

//         match prefixed.1 {
//             RuntimeMetadata::V14(metadata) => Ok(metadata),
//             other => Err(MetadataError::UnsupportedVersion(other.version()).into()),
//         }
//     }
// }

// #[expect(clippy::module_name_repetitions)]
// pub struct RuntimeApi {
//     pub api: ApiClient,
//     pub metadata: Runtime,
// }

// impl RuntimeApi {
//     // pub async fn fetch_storage(&self) {
//     //     self.0.request("state_getStorage", params)
//     // }

//     pub fn new(metadata: RuntimeMetadataV14, api: ApiClient) -> Result<Self, ApiError> {
//         Ok(Self {
//             api,
//             metadata: metadata.try_into()?,
//         })
//     }

//     pub fn assets(&self) -> Result<AssetsPalletApi<'_>, ApiError> {
//         Ok(AssetsPalletApi(
//             self,
//             self.metadata
//                 .pallets
//                 .assets
//                 .as_ref()
//                 .ok_or(MetadataError::PalletNotFound(AssetsPallet::NAME))?,
//         ))
//     }
// }

// #[expect(clippy::module_name_repetitions)]
// pub struct AssetsPalletApi<'a>(pub &'a RuntimeApi, pub &'a AssetsPallet);

// impl AssetsPalletApi<'_> {
//     pub fn storage(&self) -> AssetsPalletStorageApi<'_> {
//         AssetsPalletStorageApi(self.0, &self.1.storage)
//     }
// }

// #[expect(clippy::module_name_repetitions)]
// pub struct AssetsPalletStorageApi<'a>(pub &'a RuntimeApi, pub &'a AssetsPalletStorage);

// impl AssetsPalletStorageApi<'_> {
//     pub async fn asset(&self, id: AssetId) {
//         let id_en =
//             id.0.encode_as_type(self.1.entries.asset.key.id, &self.0.metadata.types)
//                 .unwrap();

//         let ss: Bytes = self
//             .0
//             .api
//             .0
//             .request(
//                 "state_getStorage",
//                 [format!(
//                     "0x{}{}{}",
//                     const_hex::encode(hasher::twox_128(AssetsPallet::NAME.0.as_bytes())),
//                     const_hex::encode(hasher::twox_128(
//                         AssetAssetsPalletStorageEntry::NAME.0.as_bytes()
//                     )),
//                     const_hex::encode([hasher::blake2_128(&id_en).to_vec(), id_en].concat()),
//                 )],
//             )
//             .await
//             .unwrap();

//         let asst = Asset::decode_as_type(&mut &*ss, self.1.entries.asset.value.id, &self.0.metadata.types).unwrap();

//         tracing::error!("asst: {asst:?}");
//         // self.1.entries.asset.hasher
//     }
// }

// pub fn extract_constant<T: DecodeAsType>(
//     metadata: &RuntimeMetadataV14,
//     constant_into: impl Into<Constant>,
// ) -> Result<T, ApiError> {
//     let constant = constant_into.into();
//     let (expected_pallet, expected_constant) = constant.pallet_n_constant();

//     let Some(pallet) = metadata
//         .pallets
//         .iter()
//         .find(|pallet| pallet.name == expected_pallet.to_str())
//     else {
//         return Err(ApiError::PalletNotFound(expected_pallet));
//     };

//     let Some(pallet_constant) = pallet
//         .constants
//         .iter()
//         .find(|pallet_constant| pallet_constant.name == expected_constant)
//     else {
//         return Err(ApiError::ConstantNotFound(constant));
//     };

//     // let parsed = substrate_parser::decode_all_as_type::<_, _, RuntimeMetadataV14>(
//     //     &pallet_constant.ty,
//     //     &&*pallet_constant.value,
//     //     &mut (),
//     //     &metadata.types,
//     // )?.data;

//     // Ok(T::decode(parsed).unwrap())

//     Ok(T::decode_as_type(
//         &mut &*pallet_constant.value,
//         pallet_constant.ty.id,
//         &metadata.types,
//     )
//     .unwrap())
//     // Ok(Value::decode_as_type(
//     //     &mut &*pallet_constant.value,
//     //     pallet_constant.ty.id,
//     //     &metadata.types,
//     // )
//     // .unwrap())
// }

// pub async fn fetch_runtime_version(
//     client: &WsClient,
//     block: &BlockHash,
// ) -> Result<RuntimeVersion, ApiError> {
//     client
//         .request("chain_getRuntimeVersion", [block])
//         .await
//         .map_err(ApiError::RuntimeVersion)
// }
