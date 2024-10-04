# ⛩️ A gateway daemon for Kalatori

## 🔽 Download

Compiled binaries for 🐧 Linux x86-64 can be found in [the "Releases" section](https://github.com/Alzymologist/Kalatori-backend/releases).

## 🛠️ Compile from the source

To compile the daemon, the **latest stable** Rust compiler version is required.
> ❔ Instructions for the Rust installation can be found at https://www.rust-lang.org/tools/install.

Then run the following command:

```sh
cargo b -r
```
Compiled binary can be found at the [`target/release/kalatori`](./target/release/kalatori) path.

## 🕹️ Usage

```
kalatori [OPTIONS] --recipient <HEX/SS58 ADDRESS>

Options:
  -c, --config <PATH>
          A path to the daemon config.

          The daemon creates the default config at the specified path if there's no file at it.

          [env: KALATORI_CONFIG=]
          [default: kalatori.toml]

  -l, --log[=<DIRECTIVES>]
          Directives for the daemon logger.

          By default, it prints only "info" messages from crucial modules.
          If no value is specified, the logger prints all messages with the "debug" level & above
          from all modules.

          The syntax of directives is described at
          https://docs.rs/tracing-subscriber/0.3.18/tracing_subscriber/filter/struct.EnvFilter.html#directives.

          [env: KALATORI_LOG=]
          [default:
          off,kalatori::callback=info,kalatori::database=info,kalatori::chain=info,kalatori::server=info,kalatori::utils::shutdown=info,kalatori=info]

      --remark <STRING>
          An arbitrary string to be used verbatim in API for the field of the same name.

          Does nothing daemonwise.

          [env: KALATORI_REMARK=]
          [aliases: rmrk]

  -r, --recipient <HEX/SS58 ADDRESS>
          A recipient address of all tokens from orders on all chains.

          Note that after the address is changed, all previous & pending transfer transactions to
          the recipient will still have the previous address. The new address is used for new
          transactions only.

          [env: KALATORI_RECIPIENT=]

  -d, --database [<PATH>]
          A path to the daemon database.

          If there's no file at the path, the daemon creates the new database at it.
          If this argument is omitted, the daemon reads the database path from the daemon config.
          Otherwise, the path in the config is ignored.
          For testing purposes, the daemon can create & use the temporary in-memory database. To do
          this, specify this argument with no value.

          [env: KALATORI_DATABASE=]

  -h, --help
          Print this text.

  -V, --version
          Print the daemon version (and its build metadata).

Environment variables:
  KALATORI_SEED=<PHRASE>
          A mnemonic phrase in the BIP-39 format.

          Used for generating order accounts. The daemon saves the public key of the phrase in the
          database & refuses to work if the database contains accounts generated from a different
          phrase.
          To change the current phrase in the existing database, use `KALATORI_OLD_SEED_*`
          environment variables.

          A required argument.

  KALATORI_OLD_SEED_*=<PHRASE>
          A mnemonic phrase in the BIP-39 format.

          This is a prefix for variables containing an old phrase. Used for the key rotation
          mechanism. If there's no accounts generated from the provided old phrase in the database,
          the daemon deletes the corresponding old public key (if there's one) in the database.

          This argument is required only when the database contains accounts generated from phrases
          different than the one provided in the `KALATORI_SEED` environment variable.
```

### 🕹️📝 Examples
Start the daemon, and create (or open existing) the config at the default path in the current directory, create (or open existing) the database at the default path in the current directory, and enable the default logging.
```sh
KALATORI_SEED="impulse idea people slow agent orphan color sugar siege laugh view amused property destroy ghost" \
kalatori -r 5FuwwzpHSYp8k1Bmv9fC394vGNdwqAvPRCmqYqdv3KrXpXWi
```

Start the daemon, and create (or open existing) the config at `my_config.toml` in the current directory.
```sh
KALATORI_SEED="impulse idea people slow agent orphan color sugar siege laugh view amused property destroy ghost" \
kalatori -r 5FuwwzpHSYp8k1Bmv9fC394vGNdwqAvPRCmqYqdv3KrXpXWi -c my_config.toml
```

Start the daemon, enable the "debug" logging for all modules, and create the database on the RAM instead of the persistent storage. \
Useful for testing.
```sh
KALATORI_SEED="impulse idea people slow agent orphan color sugar siege laugh view amused property destroy ghost" \
kalatori -r 5FuwwzpHSYp8k1Bmv9fC394vGNdwqAvPRCmqYqdv3KrXpXWi -ld
```

Start the daemon, disable the logging altogether, and create (or open existing) the database at `my.db` in the current directory.
```sh
KALATORI_SEED="impulse idea people slow agent orphan color sugar siege laugh view amused property destroy ghost" \
kalatori -r 0x91b171bb158e2d3848fa23a9f1c25182fb8e20313b2c1eb49219da7a70ce90c3 -l= -d my.db
```

Start the daemon and change the current seed.
```sh
KALATORI_OLD_SEED_1="impulse idea people slow agent orphan color sugar siege laugh view amused property destroy ghost" \
KALATORI_SEED="ghost idea people slow agent orphan color sugar siege laugh view amused property destroy impulse" \
kalatori -r 5FuwwzpHSYp8k1Bmv9fC394vGNdwqAvPRCmqYqdv3KrXpXWi
```

## ⚙️ Config format

The config format uses the 🇹 TOML syntax.
> ❔ See https://toml.io/en for the TOML specification.

#### The `host` field

A socket address which the daemon server listens at.

The default value is `"127.0.0.1:16726"`.

#### The `database` field

A path to the daemon database.

If there's no file at the path, the daemon creates the new database at it.

The default value is `"kalatori.db"`.

#### The `debug` field

A boolean to be used in API for the field of the same name.

Does nothing daemonwise.

The default value is `false`.

#### The `account-lifetime` field

An interval in milliseconds after which the daemon stops monitoring newly generated order accounts.

This parameter also can be set per chain. See the `chain` table.

❗️ A required parameter, if at least 1 chain has no this field set.

#### The `restart-gap` field

TODO

This parameter also can be set per chain. See the `chain` table.

❗️ A required parameter, if at least 1 chain has no this field set.

### The `chain` tables

#### The `name` field

A name of the chain.

Must be unique amongst other chain names specified in this config.

❗️ A required parameter.

#### The `endpoints` field

An array of URLs of the chain endpoints.

Obviously, all endpoints must represent the same chain. The only supported protocol is WebSocket, so all endpoints must have its scheme. The daemon checks the genesis hash of a used endpoint, if the database contains at least 1 order account from its chain.

The daemon tries to use the first operational endpoint in the array, falling back to the next one in a loop in the event of any chain/network related error.

⚠️ **Every provided endpoint represents a source of truth. Specifying untrusted endpoints may render an invalid data for all orders related to this chain in the database!**

❗️ A required parameter. Must have at least 1 URL element.

#### The `account-lifetime` field

An interval in milliseconds after which the daemon stops monitoring newly generated order accounts.

This parameter also can be set globally. See the config root fields.

❗️ A required parameter, if the global one isn't set.

#### The `restart-gap` field

TODO

This parameter also can be set globally. See the config root fields.

❗️ A required parameter, if the global one isn't set.

#### The `native` table

##### The `name` field

A name of the native token.

Must be unique amongst both native & asset tokens specified in this config.

❗️ A required parameter.

##### The `decimals` field

A number of digits in the fractional part of a number representing an amount of native tokens.

❗️ A required parameter.

#### The `asset` tables

##### The `name` field

A name of an asset token.

Must be unique amongst both native & asset tokens specified in this config.

❗️ A required parameter.

##### The `id` field

A identifier number of the asset token.

* Must be unique amongst other asset token identifiers specified for this chain in this config.
* The asset of this identifier must be *sufficient* ([here's more info about this](https://support.polkadot.network/support/solutions/articles/65000181800-what-is-asset-hub-and-how-do-i-use-it-#Sufficient-and-non-sufficient-assets)).

❗️ A required parameter.


### ⚙️📝 Examples

Can be found at [`configs`](./configs) in the repository.
