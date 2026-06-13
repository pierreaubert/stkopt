# Changelog

All notable changes to `stkopt-tui` are documented here.

## 0.1.7 - 2026-06-12

### Added

- Added visible nomination status feedback for optimizer, manual selection, and QR generation actions.
- Added regression coverage for nomination optimization when APY data is unavailable.

### Changed

- Suppressed noisy light-client tracing targets in the default TUI log filter.
- Delayed the Connected UI state until startup validator and pool data have loaded.
- Moved validator, pool, staking-history, and APY-unavailable optimizer calculations out of the TUI and onto shared chain/core helpers.
- Batched nomination-pool nomination lookups when enriching pool APY.
- Account history loading now requests the last 30 days and converts that window to the correct number of eras for each network.
- Amount parsing now uses the shared `parse_token_amount` helper and rejects inputs with more decimal places than the network supports; zero amounts are also rejected.
- History loading now fetches missing eras in parallel with bounded concurrency.
- Filtered and sorted validator/pool lists are now cached and only recomputed when inputs change.
- Validator/pool enrichment now uses the shared `stkopt-chain` orchestration helpers.

### Fixed

- Restored per-validator APY enrichment by using the newest complete chain era with reward, reward-points, and exposure data.
- Fixed startup caching so cached pools are shown immediately, pool refreshes are persisted, and cached validator names avoid unnecessary People-chain fetches.
- Fixed the Nominate tab `o` shortcut appearing to do nothing when light-client data has no APY values by falling back to low-commission active validators.
- Fixed APY-unavailable nomination fallback so zero-stake/no-exposure validators are not selected.
- Fixed APY-unavailable nomination optimization feedback by focusing the first selected validator and showing unavailable APY as `n/a` instead of `0.00%`.
- Fixed Account History showing stale cached eras when older history existed by loading the latest cached eras and filtering them to the current recent-era window.
- Fixed existing databases that had a `cached_pools` table without the newer nullable `apy` column.
- Fixed the Nominate tab `g` shortcut silently doing nothing by showing actionable messages for missing accounts, missing selections, or address encoding failures.
- Fixed nomination QR payload failures so they are shown in the UI instead of only written to logs.
- Fixed state hygiene when removing an address-book account: if the removed account is the currently watched account, its balance, history, and loaded-for state are now cleared.
- Fixed state hygiene when switching networks: derived validator, pool, account, history, optimization, and QR states are now reset.
- Fixed braille chart rendering to fall back to `?` instead of panicking on invalid Unicode values.
- Removed several unused `Action` variants and the unused `PendingTransaction.description` field.
- Replaced remaining `println!`/`eprintln!` calls in update mode with `tracing` macros.
- Unified the SQLite data directory with the desktop app and core config; legacy TUI caches are migrated automatically.
- Paseo SS58 prefix and token metadata now come from `Network` methods instead of hard-coded values.
- Account address input validation now accepts any valid SS58 address, including Kusama addresses that start with letters.
- `withdraw_unbonded` and pool `withdraw_unbonded` transaction payloads now query the real number of slashing spans instead of always passing `0`.
- Identity cache reads now respect the TTL and ignore stale identities.

## 0.1.4 - 2026-05-31

### Changed

- Version aligned with the workspace 0.1.4 release.
