# Changelog

All notable changes to `stkopt-tui` are documented here.

## 0.1.7 - 2026-06-11

### Added

- Added visible nomination status feedback for optimizer, manual selection, and QR generation actions.
- Added regression coverage for nomination optimization when APY data is unavailable.

### Changed

- Suppressed noisy light-client tracing targets in the default TUI log filter.
- Delayed the Connected UI state until startup validator and pool data have loaded.
- Batched nomination-pool nomination lookups when enriching pool APY.

### Fixed

- Fixed the Nominate tab `o` shortcut appearing to do nothing when light-client data has no APY values by falling back to low-commission active validators.
- Fixed the Nominate tab `g` shortcut silently doing nothing by showing actionable messages for missing accounts, missing selections, or address encoding failures.
- Fixed nomination QR payload failures so they are shown in the UI instead of only written to logs.

## 0.1.4 - 2026-05-31

### Changed

- Version aligned with the workspace 0.1.4 release.
