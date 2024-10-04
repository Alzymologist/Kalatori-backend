//! The arguments module.
//!
//! Contains everything related to CLI arguments, environment variables, and the config.

use crate::{
    chain_wip::definitions::{Url, H256, HEX_PREFIX},
    database::definitions::{AssetId, Timestamp},
    error::{AccountParseError, ChainIntervalError, ConfigError},
    logger,
    server::definitions::new::{Decimals, SS58Prefix, SubstrateAccount},
    utils::PathDisplay,
};
use clap::{Arg, ArgAction, Parser};
use serde::{Deserialize, Serialize};
use std::{
    borrow::{Borrow, Cow},
    env,
    ffi::{OsStr, OsString},
    fmt::{Debug, Display, Formatter, Result as FmtResult},
    fs::File,
    io::{ErrorKind, Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::Path,
    str,
    sync::Arc,
};
use substrate_crypto_light::common::{AccountId32, AsBase58};
use toml_edit::de;

shadow_rs::shadow!(shadow);

use shadow::{BUILD_TIME_3339, RUST_VERSION, SHORT_COMMIT};

macro_rules! env_var_prefix {
    ($var:literal) => {
        concat!("KALATORI_", $var)
    };
}

pub const SEED: &str = env_var_prefix!("SEED");
pub const OLD_SEED: &str = env_var_prefix!("OLD_SEED_");

const YEAR: &str = "2024";
const DEFAULT_CONFIG: &[u8] = include_bytes!("../configs/polkadot.toml");

#[expect(edition_2024_expr_fragment_specifier)]
macro_rules! default_var {
    ($var:ident: $var_type:ty = $var_value:expr) => {
        #[expect(non_snake_case)]
        fn $var() -> $var_type {
            const VAR: $var_type = $var_value;

            VAR
        }
    };
}

default_var!(DATABASE: Cow<'static, str> = Cow::Borrowed("kalatori.db"));
default_var!(HOST: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 16726));

// Seems like `clap` can't include a short help message as a header for the corresponding long help
// message, so we store short ones here to reuse them in both short & long ones.

const CONFIG_HELP: &str = "A path to the daemon config.";
const LOG_HELP: &str = "Directives for the daemon logger.";
const REMARK_HELP: &str = "An arbitrary string to be used verbatim in API for the field of the \
    same name.";
const RECIPIENT_HELP: &str = "A recipient address of all tokens from orders on all chains.";
const DATABASE_HELP: &str = "A path to the daemon database.";

// Since main text coloring crates aren't compatible with `const_format`. We use manually written
// escape codes.

const ESCAPE: &str = "\x1B[";
const M: &str = "m";
const RESET: &str = const_format::concatcp!(ESCAPE, M);
const BOLD: &str = const_format::concatcp!(ESCAPE, "1", M);
const HEADER: &str = const_format::concatcp!(BOLD, ESCAPE, "4", M);

#[derive(Parser)]
#[command(
    about,
    disable_help_flag(true),
    arg(
        Arg::new("help")
            .short('h')
            .long("help")
            .action(ArgAction::Help)
            .help("Print this text.")
    ),
    disable_version_flag(true),
    arg(
        Arg::new("version")
            .short('V')
            .long("version")
            .action(ArgAction::Version)
            .help("Print the daemon version (and its build metadata).")
    ),
    after_help(const_format::concatcp!(
        HEADER,
        "Environment variables:",
        RESET,
        "\n  ",
        BOLD,
        SEED,
        RESET,
        indoc::indoc! {"
            =<PHRASE>
                      A mnemonic phrase in the BIP-39 format.

                      Used for generating order accounts. The daemon saves the public key of the \
            phrase in the database & refuses to work if the database contains accounts generated \
            from a different phrase.
                      To change the current phrase in the existing database, use `"
        },
        OLD_SEED,
        indoc::indoc! {"
            *` environment variables.

                      A required argument.

              "
        },
        BOLD,
        OLD_SEED,
        "*",
        RESET,
        indoc::indoc! {"
            =<PHRASE>
                      A mnemonic phrase in the BIP-39 format.

                      This is a prefix for variables containing an old phrase. Used for the key \
            rotation mechanism. If there's no accounts generated from the provided old phrase in \
            the database, the daemon deletes the corresponding old public key (if there's one) in \
            the database.

                      This argument is required only when the database contains accounts generated \
            from phrases different than the one provided in the `"
        },
        SEED,
        indoc::indoc! {"
            ` environment variable.

            More documentation can be found at "
        },
        env!("CARGO_PKG_REPOSITORY"),
        indoc::indoc! {"
            .

            Copyright (C) "
        },
        YEAR,
        " ",
        clap::crate_authors!()
    )),
    version,
    long_version(const_format::concatcp!(
        clap::crate_version!(),
        const_format::formatcp!(" ({SHORT_COMMIT})"),
        indoc::indoc! {"


            Built on "
        },
        // Replaces the local offset part (e.g. "-08:00") with the "00:00" (aka "Z") UTC offset.
        const_format::str_splice!(BUILD_TIME_3339, BUILD_TIME_3339.len() - 6.., "Z").output,
        indoc::indoc! {"

            with "
        },
        RUST_VERSION,
        ".",
    )),
    next_line_help(true),
)]
pub struct CliArgs {
    #[arg(
        short,
        long,
        env(env_var_prefix!("CONFIG")),
        value_name("PATH"),
        default_value("kalatori.toml"),
        help(CONFIG_HELP),
        long_help(const_format::concatcp!(
            CONFIG_HELP,
            indoc::indoc! {"


                The daemon creates the default config at the specified path if there's no file at \
                it."
            },
        )),
    )]
    pub config: OsString,

    #[arg(
        short,
        long,
        env(env_var_prefix!("LOG")),
        value_name("DIRECTIVES"),
        default_value(logger::default_filter()),
        default_missing_value("debug"),
        num_args(0..=1),
        require_equals(true),
        help(LOG_HELP),
        long_help(const_format::concatcp!(
            LOG_HELP,
            indoc::indoc! {"


                By default, it prints only \"info\" messages from crucial modules.
                If no value is specified, the logger prints all messages with the \"debug\" level \
                & above from all modules.

                The syntax of directives is described at \
                https://docs.rs/tracing-subscriber/0.3.18/tracing_subscriber/filter/struct.EnvFilter.html#directives."
            },
        )),
    )]
    pub log: String,

    #[arg(
        long,
        env(env_var_prefix!("REMARK")),
        visible_alias("rmrk"),
        value_name("STRING"),
        help(REMARK_HELP),
        long_help(const_format::concatcp!(
            REMARK_HELP,
            indoc::indoc! {"


                Does nothing daemonwise."
            },
        )),
    )]
    pub remark: Option<String>,

    #[arg(
        short,
        long,
        env(env_var_prefix!("RECIPIENT")),
        value_name("HEX/SS58 ADDRESS"),
        help(RECIPIENT_HELP),
        long_help(const_format::concatcp!(
            RECIPIENT_HELP,
            indoc::indoc! {"


                Note that after the address is changed, all previous & pending transfer \
                transactions to the recipient will still have the previous address. The new \
                address is used for new transactions only."
            },
        )),
    )]
    pub recipient: OsString,

    #[arg(
        short,
        long,
        env(env_var_prefix!("DATABASE")),
        value_name("PATH"),
        help(DATABASE_HELP),
        long_help(const_format::concatcp!(
            DATABASE_HELP,
            indoc::indoc! {"


                If there's no file at the path, the daemon creates the new database at it.
                If this argument is omitted, the daemon reads the database path from the daemon \
                config. Otherwise, the path in the config is ignored.
                For testing purposes, the daemon can create & use the temporary in-memory \
                database. To do this, specify this argument with no value."
            },
        )),
    )]
    #[expect(clippy::option_option)]
    pub database: Option<Option<OsString>>,
}

/// User-supplied settings through the config file.
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    #[serde(flatten)]
    pub intervals: ChainIntervals,
    #[serde(default = "HOST")]
    pub host: SocketAddr,
    #[serde(default = "DATABASE")]
    pub database: Cow<'static, str>,
    #[serde(default)]
    pub debug: bool,
    #[serde(default)]
    pub chain: Vec<Chain>,
}

impl Config {
    pub fn parse(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let mut file = match File::create_new(&path) {
            Ok(mut file) => {
                tracing::info!("Creating the default config at {}.", PathDisplay(path));

                file.write_all(DEFAULT_CONFIG)?;

                file
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                tracing::info!("Opening the config file at {}.", PathDisplay(&path));

                File::open(path)?
            }
            Err(error) => return Err(error.into()),
        };

        let mut unparsed_config = vec![];

        file.read_to_end(&mut unparsed_config)?;

        de::from_slice(&unparsed_config).map_err(Into::into)
    }
}

#[derive(Deserialize, Clone, Hash, PartialEq, Eq, Serialize)]
pub struct ChainName(pub Arc<str>);

impl Borrow<str> for ChainName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl Display for ChainName {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        Display::fmt(&self.0, f)
    }
}

impl Debug for ChainName {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        Debug::fmt(&self.0, f)
    }
}

#[derive(Deserialize, Clone)]
pub struct Chain {
    pub name: ChainName,
    #[serde(flatten)]
    pub config: ChainConfig,
}

#[derive(Deserialize, Clone)]
pub struct ChainConfig {
    pub endpoints: Vec<Url>,
    #[serde(flatten)]
    pub inner: ChainConfigInner,
}

#[derive(Deserialize, Clone, Debug)]
pub struct ChainConfigInner {
    pub native: Option<Native>,
    #[serde(default)]
    pub asset: Vec<AssetInfo>,
    #[serde(flatten)]
    pub intervals: ChainIntervals,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ChainIntervals {
    pub restart_gap: Option<Timestamp>,
    pub account_lifetime: Option<Timestamp>,
}

impl ChainIntervals {
    pub fn check(&self, root: &Self) -> Result<(), ChainIntervalError> {
        self.account_lifetime
            .or(root.account_lifetime)
            .ok_or(ChainIntervalError::AccountLifetime)?;
        self.restart_gap
            .or(root.restart_gap)
            .ok_or(ChainIntervalError::RestartGap)?;

        Ok(())
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct Native {
    pub name: String,
    pub decimals: Decimals,
}

#[derive(Deserialize, Clone, Debug)]
pub struct AssetInfo {
    pub name: String,
    pub id: AssetId,
}

#[derive(Clone, Copy)]
pub enum Account {
    Hex(AccountId32),
    Substrate(SubstrateAccount),
}

impl Account {
    pub fn from_os_str(string: impl AsRef<OsStr>) -> Result<Self, AccountParseError> {
        let s = string.as_ref();

        Ok(
            if let Some(stripped) = s.as_encoded_bytes().strip_prefix(HEX_PREFIX.as_bytes()) {
                H256::from_hex(stripped).map(|hash| Self::Hex(AccountId32(hash.to_be_bytes())))?
            } else {
                AccountId32::from_base58_string(
                    s.to_str().ok_or(AccountParseError::InvalidUnicode)?,
                )
                .map(|(account, p)| Self::Substrate(SubstrateAccount(SS58Prefix(p), account)))?
            },
        )
    }
}

impl Display for Account {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::Hex(a) => Display::fmt(&H256::from_be_bytes(a.0), f),
            Self::Substrate(SubstrateAccount(p, a)) => {
                let s = &a.to_base58_string(p.0);

                if f.alternate() {
                    Debug::fmt(s, f)
                } else {
                    Display::fmt(s, f)
                }
            }
        }
    }
}

impl From<Account> for AccountId32 {
    fn from(value: Account) -> Self {
        match value {
            Account::Hex(a) | Account::Substrate(SubstrateAccount(_, a)) => a,
        }
    }
}
