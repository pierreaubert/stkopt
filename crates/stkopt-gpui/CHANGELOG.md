# Changelog

All notable changes to `stkopt-gpui` are documented here.

## 0.1.7 - 2026-06-12

### Added

- Added connection progress gating for operations, validators, pools, and watched-account history.
- Added tests for the new progress-completion rules.

### Changed

- Moved staking display enrichment and optimizer selection logic out of the desktop app and onto shared chain/core helpers.
- Transaction, nomination, pool, and refresh actions now stay disabled until the chain data needed by those flows is ready.
- Watching an account now clears stale account-specific data and fetches fresh account state through a shared path.
- Suppressed noisy smoldot/light-client tracing targets in the desktop logger.
- Removed the duplicated local `Network` and `ConnectionMode` enums; the desktop app now uses `stkopt_core::Network` and `stkopt_chain::ConnectionMode` directly.
- History fetch now uses a lookback in days (converted to eras per network), matching the TUI semantics.
- History loading now fetches missing eras in parallel with bounded concurrency.
- Filtered and sorted validator/pool lists are now cached and only recomputed when inputs change.
- Validator/pool enrichment now uses the shared `stkopt-chain` orchestration helpers.

### Fixed

- Restored desktop per-validator APY enrichment by using the newest complete chain era with APY inputs instead of only the immediately previous era.
- Fixed desktop validator identity refresh so cached names are reused and only missing names are fetched from People chain.
- Fixed stale account balances and history remaining visible after account or network changes.
- Fixed action buttons becoming available before account data and chain startup data had finished loading.
- Fixed state hygiene when removing an address-book account: if the removed account is the currently watched account, its staking info and history are now cleared.
- Fixed network switching to persist the new network in settings and to clear validator selections, optimization results, connection errors, and QR/modal state.
- Fixed connection-mode changes to clear the same derived state before reconnecting.
- Fixed watching a new account to clear connection errors and close any open QR modal so stale transaction payloads are not left visible.
- Replaced remaining `eprintln!` calls with `tracing::error!`.
- `format_stake` and validator table stake formatting now use the shared `format_token_balance` helper with the correct network decimals (Kusama/Westend no longer shown 100× too large).
- Paseo SS58 prefix and token metadata now come from `Network` methods instead of hard-coded values.
- `withdraw_unbonded` and pool `withdraw_unbonded` transaction payloads now query the real number of slashing spans instead of always passing `0`.
- `AccountData` now includes `frozen_balance` and `StakingInfo.transferable` is computed as `free - frozen`, correctly accounting for on-chain locks.
- Startup cache loading now uses the real active era from `CachedChainMetadata` instead of era `0`, so fresh caches are displayed immediately.
- Identity cache reads now respect the TTL and ignore stale identities.

## 0.1.4 - 2026-05-31

### Added

- Added a GPUI theme bridge for persisted theme settings and theme-aware chart colors.
- Added Midnight, Forest, and Black & White theme options to settings.

### Changed

- Reworked settings and optimizer controls to use richer GPUI toolkit components.
- Made light and dark mode colors theme-aware across GPUI views and modals.
- Replaced the history APY plot with a native GPUI chart whose y-axis starts at 0, uses themed text, and formats x-axis dates as `mm-dd`.
- Moved Account above Dashboard in the vertical navigation.
- Added a full-width bottom log pane with a collapsible, draggable pane divider.

### Fixed

- Fixed dashboard Claim Rewards so it generates the claim payout QR flow or shows visible feedback when claiming is unavailable.
- Fixed switching from RPC to Light Client so active sessions reconnect instead of remaining disconnected.
- Fixed primary action button text contrast in light themes, including the Account Watch button.
- Fixed optimization average APY display so it uses the same percentage units as selected validator rows.
