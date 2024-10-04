//! Common objects for the chain interaction system.

use crate::{
    arguments::{ChainConfigInner, ChainName},
    database::definitions::ChainHash,
};
use ahash::RandomState;
use arrayvec::ArrayString;
use const_hex::FromHexError;
use indexmap::IndexSet;
use jsonrpsee::ws_client::{WsClient, WsClientBuilder};
use ruint::aliases::U256;
use serde::{
    de::{Error as DeError, Unexpected, Visitor},
    Deserialize, Deserializer, Serialize, Serializer,
};
use std::{
    fmt::{Debug, Display, Formatter, Result as FmtResult},
    hash::Hash,
    sync::Arc,
};

pub const HEX_PREFIX: &str = "0x";

#[derive(Debug)]
pub struct ChainPreparator(pub ChainName);

impl Display for ChainPreparator {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str("the ")?;

        f.double(|formatter| Display::fmt(&self.0, formatter))?;

        f.write_str(" chain preparator")
    }
}

pub struct ConnectedChain {
    pub endpoints: (Url, IndexSet<Url, RandomState>),
    pub chain_config: ChainConfigInner,
    pub genesis: BlockHash,
    pub client: WsClient,
    pub client_config: WsClientBuilder,
}

pub struct PreparedChain {
    pub hash: ChainHash,
    pub connected: ConnectedChain,
}

#[derive(Deserialize, Clone, Copy, Serialize)]
pub struct BlockHash(pub H256);

#[expect(edition_2024_expr_fragment_specifier)]
macro_rules! hash_impl {
    ($name:ident, $inner:ident, $bytes:expr) => {
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct VisitorImpl;

                impl Visitor<'_> for VisitorImpl {
                    type Value = $name;

                    fn expecting(&self, f: &mut Formatter<'_>) -> FmtResult {
                        write!(
                            f,
                            "a hexidecimal {}-character long string prefixed with {HEX_PREFIX:?}",
                            $name::HEX_LENGTH
                        )
                    }

                    fn visit_str<E: DeError>(self, string: &str) -> Result<Self::Value, E> {
                        decode_hex_for_visitor(string, |stripped| $name::from_hex(stripped))
                    }
                }

                deserializer.deserialize_str(VisitorImpl)
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.to_prefixed_hex())
            }
        }

        paste::paste! {
            #[doc = "`" $name "` printing implementations:" ]
            /// - `{}` prints `0x00000000...`.
            /// - `{:#}` prints `"0x00000000..."`.
            #[doc = "- `{:?}` prints `" $name "(0x00000000...)`." ]
            #[derive(Clone, Copy, PartialEq, Eq, Hash)]
            pub struct $name(pub $inner);

            impl Debug for $name {
                fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
                    f.debug_tuple(stringify!($name)).field(&[<$name Display>](self)).finish()
                }
            }

            impl Display for $name {
                fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
                    let hex = [<$name Display>](self);

                    if f.alternate() {
                        f.double(|formatter| hex.fmt(formatter))
                    } else {
                        hex.fmt(f)
                    }
                }
            }

            struct [<$name Display>]<'a>(&'a $name);

            impl Debug for [<$name Display>]<'_> {
                fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
                    f.write_str(&self.0.to_prefixed_hex())
                }
            }
        }

        impl $name {
            pub const HEX_LENGTH: usize = $bytes * 2;

            pub fn from_hex(string: impl AsRef<[u8]>) -> Result<Self, FromHexError> {
                const_hex::decode_to_array(string.as_ref()).map(|array| Self::from_be_bytes(array))
            }

            pub fn to_hex(self) -> ArrayString<{ Self::HEX_LENGTH }> {
                let mut array = [0; Self::HEX_LENGTH];

                const_hex::encode_to_slice(self.to_be_bytes(), &mut array).unwrap();
                ArrayString::from_byte_string(&array).unwrap()
            }

            pub fn to_prefixed_hex(self) -> ArrayString<{ Self::HEX_LENGTH + HEX_PREFIX.len() }> {
                let mut array = [0; Self::HEX_LENGTH + 2];
                let (prefix, hex) = array.split_at_mut(HEX_PREFIX.len());

                prefix.copy_from_slice(HEX_PREFIX.as_bytes());

                const_hex::encode_to_slice(self.to_be_bytes(), hex).unwrap();
                ArrayString::from_byte_string(&array).unwrap()
            }

            pub fn from_be_bytes(bytes: impl Into<[u8; $bytes]>) -> Self {
                Self($inner::from_be_bytes(bytes.into()))
            }

            pub fn to_be_bytes(self) -> [u8; $bytes] {
                self.0.to_be_bytes()
            }
        }
    };
}

hash_impl!(H256, U256, U256::BYTES);
hash_impl!(H64, u64, (u64::BITS / 8) as usize);

pub fn decode_hex_for_visitor<T, E: DeError>(
    string: &str,
    decode: impl FnOnce(&str) -> Result<T, FromHexError>,
) -> Result<T, E> {
    if let Some(stripped) = string.strip_prefix(HEX_PREFIX) {
        decode(stripped).map_err(DeError::custom)
    } else {
        Err(DeError::invalid_value(
            Unexpected::Str(string),
            &"a string prefixed with \"0x\"",
        ))
    }
}

enum Quote {
    Double,
    Backtick,
}

impl From<Quote> for &'static str {
    fn from(quote: Quote) -> Self {
        match quote {
            Quote::Double => "\"",
            Quote::Backtick => "`",
        }
    }
}

pub trait EncaseInQuotes {
    fn encase_in(
        quote: &'static str,
        f: &mut Formatter<'_>,
        format: impl FnOnce(&mut Formatter<'_>) -> FmtResult,
    ) -> FmtResult {
        f.write_str(quote)?;

        format(f)?;

        f.write_str(quote)
    }

    fn double(&mut self, format: impl FnOnce(&mut Formatter<'_>) -> FmtResult) -> FmtResult;

    fn backtick(&mut self, format: impl FnOnce(&mut Formatter<'_>) -> FmtResult) -> FmtResult;
}

impl EncaseInQuotes for Formatter<'_> {
    fn double(&mut self, format: impl FnOnce(&mut Formatter<'_>) -> FmtResult) -> FmtResult {
        Self::encase_in("\"", self, format)
    }

    fn backtick(&mut self, format: impl FnOnce(&mut Formatter<'_>) -> FmtResult) -> FmtResult {
        Self::encase_in("`", self, format)
    }
}

#[derive(Deserialize, Serialize, Clone, PartialEq, Eq, Hash)]
pub struct Url(pub Arc<str>);

// #[derive(Serialize)]
// pub struct StorageKey(pub String);

// #[derive(DecodeAsType, Debug)]
// pub struct Asset {
//     is_sufficient: bool,
//     min_balance: u128,
// }

// #[derive(Debug, Clone, Copy)]
// pub enum Pallet {
//     System,
// }

// impl Pallet {
//     pub fn to_str(self) -> &'static str {
//         match self {
//             Self::System => "System",
//         }
//     }
// }

// impl Display for Pallet {
//     fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
//         f.backtick(|formatter| formatter.write_str(self.to_str()))
//     }
// }

// #[derive(Debug, Clone, Copy)]
// pub enum Constant {
//     System(SystemConstant),
// }

// impl Constant {
//     pub fn pallet_n_constant(self) -> (Pallet, &'static str) {
//         match self {
//             Constant::System(constant) => (Pallet::System, constant.to_str()),
//         }
//     }
// }

// impl Display for Constant {
//     fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
//         let (pallet, constant) = self.pallet_n_constant();

//         f.write_str("the constant ")?;
//         f.backtick(|formatter| formatter.write_str(constant))?;
//         f.write_str("in the pallet ")?;

//         f.backtick(|formatter| formatter.write_str(pallet.to_str()))
//     }
// }

// #[derive(Debug, Clone, Copy)]
// pub enum SystemConstant {
//     Version,
//     SS58Prefix,
// }

// impl SystemConstant {
//     pub fn to_str(self) -> &'static str {
//         match self {
//             Self::Version => "Version",
//             Self::SS58Prefix => "SS58Prefix",
//         }
//     }
// }

// impl From<SystemConstant> for Constant {
//     fn from(constant: SystemConstant) -> Self {
//         Constant::System(constant)
//     }
// }

// pub mod metadata {
//     use crate::error::{MetadataError, PalletStorageMetadataError, StorageEntryMetadataError};
//     use frame_metadata::v14::{
//         PalletMetadata, PalletStorageMetadata, RuntimeMetadataV14, StorageEntryMetadata,
//         StorageEntryType, StorageHasher,
//     };
//     use scale_info::{
//         form::{Form, PortableForm},
//         PortableRegistry,
//     };
//     use std::fmt::{Display, Formatter, Result as FmtResult};

//     use error_helpers::StorageEntryTypeName;

//     pub mod error_helpers {
//         use frame_metadata::v14::StorageEntryType;
//         use scale_info::form::Form;
//         use std::fmt::{Display, Formatter, Result as FmtResult};

//         #[derive(Debug)]
//         pub enum StorageEntryTypeName {
//             Plain,
//             Map,
//         }

//         impl Display for StorageEntryTypeName {
//             fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
//                 match self {
//                     StorageEntryTypeName::Plain => "plain",
//                     StorageEntryTypeName::Map => "map",
//                 }
//                 .fmt(f)
//             }
//         }

//         impl<T: Form> From<StorageEntryType<T>> for StorageEntryTypeName {
//             fn from(value: StorageEntryType<T>) -> Self {
//                 match value {
//                     StorageEntryType::Plain(_) => Self::Plain,
//                     StorageEntryType::Map { .. } => Self::Map,
//                 }
//             }
//         }
//     }

//     #[derive(Debug)]
//     pub struct PalletName(pub &'static str);

//     impl Display for PalletName {
//         fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
//             self.0.fmt(f)
//         }
//     }

//     #[derive(Debug)]
//     pub struct StorageEntryName(pub &'static str);

//     impl Display for StorageEntryName {
//         fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
//             self.0.fmt(f)
//         }
//     }

//     impl TryFrom<RuntimeMetadataV14> for Runtime {
//         type Error = MetadataError;

//         fn try_from(
//             RuntimeMetadataV14 { types, pallets, .. }: RuntimeMetadataV14,
//         ) -> Result<Self, Self::Error> {
//             let mut assets = None;

//             for PalletMetadata { name, storage, .. } in pallets {
//                 if name == AssetsPallet::NAME.0 {
//                     fn process_pallet(
//                         storage: Option<PalletStorageMetadata<PortableForm>>,
//                     ) -> Result<AssetsPallet, PalletStorageMetadataError> {
//                         let PalletStorageMetadata { prefix, entries } =
//                             storage.ok_or(PalletStorageMetadataError::NotFound)?;

//                         let mut asset = None;

//                         for StorageEntryMetadata { name, ty, .. } in entries {
//                             if name == AssetAssetsPalletStorageEntry::NAME.0 {
//                                 fn process_entry(
//                                     ty: StorageEntryType<PortableForm>,
//                                 ) -> Result<AssetAssetsPalletStorageEntry, StorageEntryMetadataError>
//                                 {
//                                     let StorageEntryType::Map {
//                                         hashers,
//                                         key,
//                                         value,
//                                     } = ty
//                                     else {
//                                         return Err(
//                                             StorageEntryMetadataError::UnexpectedStorageType {
//                                                 expected: StorageEntryTypeName::Map,
//                                                 got: ty.into(),
//                                             },
//                                         );
//                                     };

//                                     let [hasher] = hashers.try_into().map_err(|_| {
//                                         StorageEntryMetadataError::UnexpectedStorageHashersAmount {
//                                             expected: 1,
//                                             got: 0,
//                                         }
//                                     })?;

//                                     Ok(AssetAssetsPalletStorageEntry { hasher, key, value })
//                                 }

//                                 asset = Some(process_entry(ty).map_err(|e| {
//                                     PalletStorageMetadataError::Entry(
//                                         AssetAssetsPalletStorageEntry::NAME,
//                                         e,
//                                     )
//                                 })?);
//                             }
//                         }

//                         Ok(AssetsPallet {
//                             storage: AssetsPalletStorage {
//                                 prefix,
//                                 entries: AssetsPalletStorageEntries {
//                                     asset: asset.ok_or(
//                                         PalletStorageMetadataError::EntryNotFound(
//                                             AssetAssetsPalletStorageEntry::NAME,
//                                         ),
//                                     )?,
//                                 },
//                             },
//                         })
//                     }

//                     assets = Some(
//                         process_pallet(storage)
//                             .map_err(|e| MetadataError::Storage(AssetsPallet::NAME, e))?,
//                     );
//                 }
//             }

//             Ok(Self {
//                 types,
//                 pallets: Pallets { assets },
//             })
//         }
//     }

//     pub struct Runtime {
//         pub types: PortableRegistry,
//         pub pallets: Pallets,
//     }

//     pub struct Pallets {
//         pub assets: Option<AssetsPallet>,
//     }

//     pub struct AssetsPallet {
//         pub storage: AssetsPalletStorage,
//     }

//     impl AssetsPallet {
//         pub const NAME: PalletName = PalletName("Assets");
//     }

//     pub struct AssetsPalletStorage {
//         pub prefix: String,
//         pub entries: AssetsPalletStorageEntries,
//     }

//     pub struct AssetsPalletStorageEntries {
//         pub asset: AssetAssetsPalletStorageEntry,
//     }

//     pub struct AssetAssetsPalletStorageEntry {
//         pub hasher: StorageHasher,
//         pub key: <PortableForm as Form>::Type,
//         pub value: <PortableForm as Form>::Type,
//     }

//     impl AssetAssetsPalletStorageEntry {
//         pub const NAME: StorageEntryName = StorageEntryName("Asset");
//     }
// }
