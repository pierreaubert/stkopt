# Changelog

All notable changes to `stkopt-core` are documented here.

## 0.1.7 - 2026-06-12

### Added

- Added shared display helpers `parse_token_amount`, `format_token_balance`, and `format_apy_ratio` used by both TUI and desktop apps.
- Added identity cache TTL support via `DEFAULT_IDENTITY_MAX_AGE_SECS` and `get_validator_identities_within_age`.
- Extended `CachedAccountStatus` to persist staking unbonding chunks, pool unbonding eras, and the pool last-recorded reward counter so cached account views are complete.

### Changed

- Aligned dependency declarations with the workspace dependency set for the 0.1.7 release.
- Token amount parsing now rejects over-precision inputs instead of silently truncating extra decimal places.

### Fixed

- Fixed APY calculations to handle zero era duration, avoid premature rounding, and filter out validators with zero stake.
- Fixed optimizer fallback selection so zero-stake or no-exposure validators are not chosen when APY data is unavailable.
- Fixed cache freshness checks to treat era 0 and missing metadata as stale.
- Fixed era duration fallback to return the on-chain `MaxEraDuration` instead of a hard-coded value.
- Fixed identity judgement recognition to accept `Reasonable` and `KnownGood` as positive judgements.
- `parse_token_amount` now rejects zero amounts so both front-ends share the same validation.
- `DiversifyByStake` now uses `HashSet` lookups for linear-time selection and excludes candidates with non-finite APYs.

## 0.1.4 - 2026-05-31

### Added

- Added persisted theme variants for Midnight, Forest, and Black & White UI themes.

### Changed

- Version aligned with the workspace 0.1.4 release.
