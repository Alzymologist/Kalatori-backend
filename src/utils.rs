//! The module for miscellaneous utilities.

use crate::error::{NotHex, UtilError};
use std::{
    fmt::{Display, Formatter, Result as FmtResult},
    path::{self, Path},
};

pub mod logger;
pub mod shutdown;
pub mod task_tracker;

pub fn unhex(hex_data: &str, what_is_hex: NotHex) -> Result<Vec<u8>, UtilError> {
    if let Some(stripped) = hex_data.strip_prefix("0x") {
        const_hex::decode(stripped).map_err(|_| UtilError::NotHex(what_is_hex))
    } else {
        const_hex::decode(hex_data).map_err(|_| UtilError::NotHex(what_is_hex))
    }
}

pub struct PathDisplay<T>(pub T);

impl<T: AsRef<Path>> Display for PathDisplay<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        let path = self.0.as_ref();

        match path::absolute(path) {
            Ok(absolute) => absolute.display().fmt(f),
            Err(_) => path.display().fmt(f),
        }
    }
}
