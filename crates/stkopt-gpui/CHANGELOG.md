# Changelog

All notable changes to `stkopt-gpui` are documented here.

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
