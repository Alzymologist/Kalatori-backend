## 1

**All slots are encoded with `parity-scale-codec = "3"`.**

### Tables

* `root`
```rs
type Key = String;
type Value = Vec<u8>;
```

Public keys of all active accounts.
* `keys`
```rs
/// A public key.
type Key = Public;
/// An accounts amount (never equals 0).
type Value = U256;
```

* `chains`
```rs
/// A chain hash.
type Key = Chain;
/// A last scanned block.
type Value = Compact<BlockNumber>;
```

* `invoices`
```rs
/// An invoice name.
type Key = Vec<u8>;
type Value = Invoice;
```

* `accounts[chain_hash]`
```rs
/// An asset ID & an account ID.
type Key = (Compact<Asset>, Account);
/// An invoice name.
type Value = Vec<u8>;
```

* `transactions[chain_hash]`
```rs
/// A death block.
type Key = Compact<BlockNumber>;
/// Pending transactions.
/// An account ID, a transaction nonce, a transaction.
type Values = (Account, Compact<Nonce>, Transfer);
```

* `hit_list`
```rs
/// A creation timestamp.
type Key = Timestamp;
/// A chain hash, an asset ID, and an account ID.
type Value = (ChainHash, Asset, Account);
```

* `archive`
```rs
???
```

### `root` keyed untyped slots

* `db_version`
```rs
Compact<Version>
```

* `daemon_info`
```rs
struct DaemonInfo {
    chains: Vec<(String, ChainProperties)>,
    /// The current public key.
    current_key: Public,
}
```

### Typed slots

```rs
struct Invoice {
    /// A public key & a derivation hash.
    derivation: (Public, Derivation),
    status: InvoiceStatus,
    /// A creation timestamp.
    #[compact]
    timestamp: Timestamp,
    #[compact]
    price: Balance,
}

struct ChainProperties {
    genesis: BlockHash,
    hash: ChainHash,
    kind: ChainKind,
}

enum ChainKind {
    NoAssets,
    Id(Vec<<Compact<Asset>>),
    MultiLocation(Vec<<Compact<Asset>>),
}

enum InvoiceStatus {
    Unpaid,
    Paid,
}

struct Transfer(TransferKind, #[compact] Balance);

enum TransferKind {
    Assets(#[compact] Asset),
    Balances,
}

type Asset = u32;
type Account = [u8; 32];
type Public = [u8; 32];
type Derivation = [u8; 32];
type ChainHash = [u8; 32];
type BlockNumber = u64;
type Nonce = u32;
type Timestamp = u64;
type Balance = u128;
type BlockHash = [u8; 32];
type Version = u64;
```

## 0

**All slots are encoded with `parity-scale-codec = "3"`.**

### Tables

* `root`
```rs
type Key = String;
type Value = Vec<u8>;
```

* `invoices`
```rs
type Key = [u8; 32];
type Value = Invoice;
```

### `root` keyed untyped slots

* `db_version`
```rs
Compact<u64>
```

* `daemon_info`
```rs
struct DaemonInfo {
    rpc: String,
    key: [u8; 32],
}
```

* `last_block`
```rs
Compact<u32>
```

### Typed slots

```rs
struct Invoice {
    recipient: [u8; 32],
    order: [u8; 32],
    status: InvoiceStatus,
}

enum InvoiceStatus {
    Unpaid(u128),
    Paid(u128),
}
```
