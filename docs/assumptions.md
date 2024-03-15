The current Substrate RPC API & pallets used by the daemon, unfortunately, don't have any strict standart, which all nodes & chains follow, and the common implementations of RPC API & pallets lack of some essential features. This creates restrictions & assumptions for which nodes & chains the daemon can be used with. Of course, our first priority is and will be a support of the Polkadot relaychain & the Polkadot Asset Hub parachain with a support of USDT & USDC assets on that parachain but in the future, we'll try to lift the following limitations and support a broader range of nodes & chains along with the evolution of the Substrate ecosystem & its standarts.

### Transfers

All extrinsics sent by the daemon are transfer transactions. Despite they themselves are quite unsophisticated, the whole process of the approximate fee calculation to subtract it from the transfer amount and then, in case of a fail because a real fee was higher than the daemon estimated, the resending of the same transaction after another never accurate calculation does sound like a really hard path.

We tried to use the [`transfer_allow_death`] call in the Balances pallet for all transfers because we thought that a transfer fee would never be greater than the existential deposit but that isn't true for all chains (e.g. it's true for Polkadot, but not for Kusama), and unlikely will be a standart, so we stuck with the [`transfer_all`] call for transfers to both the beneficiary & overpayers accounts.

The Assets pallet doesn't have a call similar to Balances's [`transfer_all`], so the daemon uses [`transfer`] assuming that a fee for that call wouldn't be higher than asset's [`min_balance`]. For now, that's true for USDT & USDC on Polkadot Asset Hub, but **if one these or another asset won't meet this criteria, the daemon won't be able to work with them**. We plan to propose and hopefully include some kind of `transfer_all` call in Assets pallet to subsequently eliminate this restriction.

[`transfer_all`]: https://docs.rs/pallet-balances/30.0.0/pallet_balances/pallet/struct.Pallet.html#method.transfer_all
[`transfer_allow_death`]: https://docs.rs/pallet-balances/30.0.0/pallet_balances/pallet/struct.Pallet.html#method.transfer_allow_death
[`transfer`]: https://docs.rs/pallet-assets/31.0.0/pallet_assets/pallet/struct.Pallet.html#method.transfer
[`min_balance`]: https://docs.rs/pallet-assets/31.0.0/src/pallet_assets/types.rs.html#66
