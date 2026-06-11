# Changelog

All notable changes to `stkopt-chain` are documented here.

## 0.1.7 - 2026-06-11

### Added

- Added batch storage helpers for AccountId-indexed maps to reduce light-client and RPC round trips.
- Added a network QA helper binary for checking supported networks and runtime compatibility.

### Changed

- Updated the chain client to Subxt 0.50 and refreshed bundled light-client chain specs.
- Improved People-chain identity loading by waiting for readiness and batching identity lookups.
- Improved light-client validator discovery with relay/session and era-staker fallbacks.

### Fixed

- Fixed Polkadot Vault signing payloads for runtimes using `AuthorizeCall`, `EthSetOrigin`, and `StorageWeightReclaim` transaction extensions.
- Fixed dynamic account decoding so malformed values with the wrong byte length are rejected instead of truncated.
- Fixed nomination QR payload generation errors so callers receive visible failures instead of log-only failures.

## 0.1.4 - 2026-05-31

### Changed

- Version aligned with the workspace 0.1.4 release.
