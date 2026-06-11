# Changelog

All notable changes to `stkopt-gpui` are documented here.

## 0.1.7 - 2026-06-11

### Added

- Added connection progress gating for operations, validators, pools, and watched-account history.
- Added tests for the new progress-completion rules.

### Changed

- Transaction, nomination, pool, and refresh actions now stay disabled until the chain data needed by those flows is ready.
- Watching an account now clears stale account-specific data and fetches fresh account state through a shared path.
- Suppressed noisy smoldot/light-client tracing targets in the desktop logger.

### Fixed

- Fixed stale account balances and history remaining visible after account or network changes.
- Fixed action buttons becoming available before account data and chain startup data had finished loading.

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
