# Changelog

All notable changes to `stkopt-chain` are documented here.

## 0.1.7 - 2026-06-12

### Added

- Added batch storage helpers for AccountId-indexed maps to reduce light-client and RPC round trips.
- Added batched nomination-pool nomination lookups for pool APY enrichment.
- Added a recent-era validator APY data probe that finds the newest era with reward, reward-points, and exposure data.
- Added a network QA helper binary for checking supported networks and runtime compatibility.

### Changed

- Updated the chain client to Subxt 0.50 and refreshed bundled light-client chain specs.
- Moved chain-derived validator, pool, and staking-history display enrichment into shared chain helpers used by both apps.
- Added shared validator/pool enrichment orchestration helpers in `enrichment.rs` to reduce front-end duplication.
- Improved People-chain identity loading by waiting for readiness and batching identity lookups.
- Improved light-client validator discovery with relay/session and era-staker fallbacks.

### Fixed

- Fixed transaction target-chain consistency so staking operations are built against the relay chain and account nonce/asset-hub queries use the Asset Hub client where applicable.
- Fixed transaction integer encodings to use explicit `Value::u128`/`Value::u32` encodings for runtime compatibility.
- Documented that metadata-hash signing is not supported; payloads continue to set the metadata hash flag when required by the runtime but signers must use the legacy format.
- Fixed Polkadot Vault QR payload layout so the genesis hash is no longer appended redundantly.
- Fixed Polkadot Vault signing payloads for runtimes using `AuthorizeCall`, `EthSetOrigin`, and `StorageWeightReclaim` transaction extensions.
- Fixed validator APY era selection so sparse partial exposure snapshots are rejected instead of leaving most validator APYs unavailable.
- Fixed dynamic account decoding so malformed values with the wrong byte length are rejected instead of truncated.
- Fixed nomination QR payload generation errors so callers receive visible failures instead of log-only failures.
- Fixed era duration fallback to return the on-chain `MaxEraDuration`.
- Fixed identity judgement recognition to accept `Reasonable` and `KnownGood` as positive judgements.
- Validator APY math now computes validator share and commission using `u128` integer arithmetic before converting to `f64` for the final APY, reducing precision loss on large balances.
- Pool APY now subtracts the pool's own commission from the averaged validator APY.

## 0.1.4 - 2026-05-31

### Changed

- Version aligned with the workspace 0.1.4 release.
