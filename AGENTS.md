# AGENTS.md

Guidance for AI coding agents working in this repository.

## Project Overview

**stkopt** is a Polkadot/Kusama staking optimizer. It provides a terminal user interface (TUI), a desktop GUI, and an iOS app for browsing validators, analyzing APY history, managing nominations and nomination pools, and signing transactions via QR codes with Polkadot Vault.

- **Repository**: `https://github.com/pierreaubert/stkopt.git`
- **License**: ISC
- **Version**: `0.1.7`
- **Rust MSRV**: `1.96` (specified in workspace `Cargo.toml`)
- **Rust Edition**: `2024`
- **Workspace Resolver**: `3`

> **WARNING**: The code is new and has had no security review. Use at your own risk.

## Technology Stack

### Rust Workspace
- **Async runtime**: `tokio` (full features)
- **Blockchain client**: `subxt` v0.44 with `native` and `unstable-light-client` features (smoldot)
- **TUI framework**: `ratatui` v0.29 with `crossterm`
- **Desktop GUI framework**: `gpui` v0.2 (Zed's framework) via `gpui-ui-kit`
- **Serialization**: `serde`, `serde_json`
- **Error handling**: `thiserror` (libraries), `color-eyre` / `anyhow` (applications)
- **Logging**: `tracing`, `tracing-subscriber`
- **Database**: `rusqlite` (bundled) for local caching
- **QR codes**: `qrcode` (generation), `rqrr` (decoding), `nokhwa` (camera access)
- **CLI parsing**: `clap` v4.5 with derive features
- **Property testing**: `proptest`

### iOS App
- **Language**: Swift 5.9+
- **Framework**: SwiftUI
- **Minimum deployment**: iOS 18.0
- **Project generation**: XcodeGen (`project.yml`)
- **Dependencies**: Starscream (WebSocket), KeychainAccess, CodeScanner

## Build Commands

The project uses [`just`](https://github.com/casey/just) as the command runner. Install it with `cargo install just`.

```bash
# List all available commands
just --list

# Fast compile check
just check            # => cargo check --workspace

# Linting
just lint             # => cargo clippy --workspace

# Testing
just test             # => cargo test --workspace --lib

# Formatting
just fmt              # => cargo fmt --all
just fmt-check        # => cargo fmt --all -- --check

# Release builds
just build            # => cargo build --release
just build-macos      # Build + ad-hoc sign with entitlements (camera)
just build-dmg        # Build signed DMG (requires DEVELOPER_ID env var)
just build-dmg-unsigned  # Build unsigned DMG for local testing

# Run the TUI application
just run              # Build, sign, and run
just run-rpc          # Build, sign, and run in RPC mode

# CI pipeline
just ci               # => fmt-check + lint + test

# Examples / manual testing
just example-connection   # Test chain connection
just example-pallets      # Check pallet compatibility

# Batch / cron mode
just update-history <address> <eras>
```

### Raw Cargo Commands

```bash
cargo check --workspace
cargo clippy --workspace
cargo test --workspace --lib
cargo test -p stkopt-core              # Single crate
cargo test -p stkopt-core test_name    # Specific test
cargo run --release                    # Run TUI binary (`stkopt`)
cargo run --release -- --network kusama
cargo run --release -- --rpc           # Force RPC instead of light client
cargo run --release -- --update --address <ss58> --eras 30
```

### iOS Build

```bash
cd stkopt-ios
xcodegen generate
open stkopt.xcodeproj
```

## Workspace Architecture

The Rust workspace lives in the root `Cargo.toml` and contains four crates under `crates/`:

### `stkopt-core` — Domain Logic
Pure domain logic with minimal external dependencies.
- `apy.rs` — APY calculations from staking rewards (compound interest formula over era durations).
- `optimizer.rs` — Validator selection algorithms (`TopApy`, `RandomFromTop`, `DiversifyByStake`).
- `types.rs` — Core data structures (`Network`, `ValidatorPreferences`, `Balance`, `EraIndex`, etc.).
- `display.rs` — Display-oriented types shared with UIs.
- `db.rs` — SQLite schema and caching (`StakingDb`), gated behind `persistence` feature.
- `config.rs` — JSON configuration management (`AppConfig`, `AddressBook`, etc.), gated behind `persistence` feature.

**Feature flags**:
- `default = []`
- `persistence` — enables `db.rs`, `config.rs`, and pulls in `rusqlite`, `directories`, `chrono`, `serde_json`.

### `stkopt-chain` — Blockchain Client
Client layer using `subxt` with both light-client (smoldot) and traditional RPC support.
- `client.rs` — Main `ChainClient` wrapper, connection management, reconnection logic.
- `config.rs` — Network-specific configuration (endpoints, SS58 prefixes).
- `error.rs` — `ChainError` enum using `thiserror`.
- `lightclient.rs` — Smoldot light client initialization and connection handling.
- `queries/` — Chain storage queries:
  - `account.rs` — Balances, staking ledger, nominations, pool membership.
  - `era.rs` — Active era, era duration, history depth.
  - `identity.rs` — People chain identity lookups (`PeopleChainClient`).
  - `pools.rs` — Nomination pool metadata, states, roles.
  - `validators.rs` — Validator preferences, exposures, points.
- `ss58.rs` — SS58 address encoding/decoding.
- `transactions.rs` — Transaction building, QR encoding for Vault, signature decoding, extrinsic assembly.

**Examples** (in `examples/`):
- `test_connection.rs` — Verify chain connectivity.
- `check_pallets.rs` — Verify pallet compatibility.
- `verify_staking_location.rs` — Inspect staking pallet location.
- `inspect_extensions.rs` — Inspect transaction extensions.

**Integration tests** (in `tests/`):
- `compare_connection_modes.rs` — Compares RPC vs Light Client data consistency.

### `stkopt-tui` — Terminal Application
Binary name: `stkopt`. Ratatui-based TUI.
- `main.rs` — Entry point, CLI parsing (`clap`), async runtime (`tokio::main`), main event loop.
- `app.rs` — Application state (`App`), input modes, tab management, keyboard handling.
- `ui.rs` — Widget rendering (tables, charts, modals, QR display).
- `action.rs` — `Action` enum for state updates (account data, validator lists, QR data, tx status).
- `event.rs` — Crossterm event handling (key presses, resize, ticks).
- `tui.rs` — Terminal initialization and teardown.
- `db.rs` — Re-exports `StakingDb` from `stkopt-core` (alias `HistoryDb`).
- `config.rs` — Local config helpers.
- `theme.rs` — Dark/light terminal theme detection (`terminal-light`).
- `log_buffer.rs` — In-memory log buffer for display inside the TUI.
- `qr_reader.rs` — Camera-based QR scanning using `nokhwa` + `rqrr`.
- `tcc.rs` — macOS TCC (camera permission) helpers.

**Build script**: `build.rs` embeds `scripts/Info.plist` into the macOS binary for camera entitlement support.

### `stkopt-gpui` — Desktop GUI Application
Binary name: `stkopt-desktop`. GPUI-based desktop app.
- `main.rs` — Entry point, `MiniApp` initialization, tokio runtime creation.
- `app.rs` — Main `StkoptApp` component, view routing, log pane management.
- `chain.rs` — Async chain interaction layer.
- `views/` — UI view modules:
  - `dashboard.rs`, `account.rs`, `validators.rs`, `optimization.rs`, `pools.rs`, `history.rs`, `settings.rs`, `help.rs`, `logs.rs`
  - `qr_modal.rs`, `staking_modal.rs`, `pool_modal.rs`
- `db_service.rs` — SQLite persistence service.
- `persistence.rs` — Re-exports config system from `stkopt-core`.
- `gpui_tokio.rs` — Bridge between GPUI's synchronous model and tokio async runtime.
- `qr_reader.rs` — Camera QR scanning.
- `errors.rs` — Application error types (`AppError`, `ErrorSeverity`, `Notification`).
- `tests/` — Unit tests in `lib.rs` + minimal E2E harness under `tests/e2e/`.

### `stkopt-ios` — iOS/iPadOS Application
Native Swift app, separate from the Rust workspace.
- `Sources/App/` — SwiftUI app entry point (`StkoptApp.swift`), `AppState`.
- `Sources/Core/` — Domain logic (`Network`, `Types`, `APYCalculator`, `ValidatorOptimizer`).
- `Sources/Chain/` — JSON-RPC/WebSocket client (`ChainClient`, `ChainQueries`, `Transactions`).
- `Sources/UI/` — SwiftUI views (`ContentView`, screens, components, modals).
- `Sources/Services/` — `NetworkService`, `WalletService`, `QRService`, `StorageService`, `CachingService`.
- `Sources/Utilities/` — `SS58.swift`, `Formatting.swift`.
- `stkoptTests/` — Unit tests.

## Code Style and Conventions

### Rust
- **Edition 2024**, MSRV `1.96`.
- **No `rustfmt.toml` or `.clippy.toml`** — use default `cargo fmt` and `cargo clippy`.
- **No default match arms on enums**: The codebase intentionally omits `_ =>` catch-all arms on enums like `Network`. Adding a new enum variant must be handled explicitly at every match site. Do not add catch-all arms unless the domain genuinely requires it.
- **Logging**: Use `tracing` macros (`tracing::info!`, `tracing::warn!`, `tracing::error!`, `tracing::debug!`). Do **not** use `println!` except in CLI-only example binaries or diagnostic scripts.
- **Error handling**:
  - Libraries (`stkopt-core`, `stkopt-chain`): Use `thiserror` enums. Propagate with `Result`.
  - Applications (`stkopt-tui`, `stkopt-gpui`): Use `color-eyre` or `anyhow` at the top level. The TUI installs `color_eyre` in `main()`.
- **Doc comments**: Use `//!` for module-level docs and `///` for item-level docs. Keep them factual and concise.
- **Naming**: Follow standard Rust naming (`snake_case` for functions/variables, `PascalCase` for types, `SCREAMING_SNAKE_CASE` for constants).
- **Constants**: Domain constants (e.g., `MS_PER_YEAR`, `MAX_NOMINATIONS`, `DEFAULT_MAX_COMMISSION`) live in `stkopt-core` near their usage.
- **Re-exports**: Crate roots (`lib.rs` / `main.rs`) re-export commonly used items to reduce boilerplate for consumers.

### Swift (iOS)
- Swift 5.9, iOS 18.0 minimum.
- Standard Swift naming conventions.
- Views organized by screen under `Sources/UI/Screens/`.

## Async and UI Patterns

### TUI
- The main loop runs in `tokio::main`.
- A background `chain_task` handles all blockchain I/O.
- Communication between the UI thread and the chain task uses **two `tokio::sync::mpsc` channels**:
  - `action_tx/rx` — chain task → UI (results, data updates).
  - `request_tx/rx` — UI → chain task (operations like `FetchAccount`, `GenerateNominationQR`, `ExecuteStakingOp`).
- A `tokio::sync::watch` channel is used for canceling long-running history fetches.
- The UI polls a `qr_reader::QrReader` on ticks when camera scanning is active.

### Desktop (GPUI)
- GPUI is synchronous; async work is dispatched onto a dedicated `tokio` runtime.
- `gpui_tokio.rs` provides the bridge to schedule futures and receive results back on the GPUI thread.

## Testing Strategy

- **Unit tests**: Inline in each crate under `#[cfg(test)] mod tests { ... }`. Present in virtually every Rust source file.
- **Property-based tests**: `proptest` is used in `stkopt-core` (optimizer) and `stkopt-gpui`.
- **Integration tests**:
  - `crates/stkopt-chain/tests/compare_connection_modes.rs` — live network comparison of RPC vs Light Client (may be slow and can fail if light client sync is unstable; not strictly asserted in CI).
- **Examples as manual tests**: `stkopt-chain` has several `examples/` for manual connectivity and compatibility verification.
- **E2E tests**: `stkopt-gpui` has a minimal E2E harness under `src/tests/e2e/` (currently mostly scaffolding).
- **iOS tests**: `stkoptTests/stkoptTests.swift`.

### Verification Gate for Agents

- After making Rust code changes, run the relevant `cargo test` command and do not report the work as done unless it passes cleanly.
- Prefer the narrowest meaningful test command while iterating (for example `cargo test -p stkopt-gpui --lib` for GPUI-only changes), but run broader tests when the change touches shared crates, transaction logic, persistence, or cross-crate behavior.
- If `cargo test` cannot be run or does not pass, the final response must say the work is not done and include the exact failing or blocked command.
- `cargo check`, `cargo clippy`, and formatting checks are useful supporting verification, but they do not replace a passing `cargo test` after code changes.

### Running Tests
```bash
# Fast unit-test-only run
just test

# All tests including integration tests (may hit live networks)
cargo test --workspace

# Single crate
cargo test -p stkopt-core
```

## Security Considerations

- **No security audit has been performed.**
- **Private keys are never stored.** The app generates unsigned transactions as QR codes for offline signing with Polkadot Vault. Signed transactions are scanned back via camera and submitted.
- **Camera permissions**: On macOS, the TUI binary must be signed with `scripts/entitlements.plist` (camera access) for QR scanning to work. The `build.rs` in `stkopt-tui` embeds `scripts/Info.plist` into the binary's `__TEXT,__info_plist` section.
- **macOS signing**: `just build-macos` performs ad-hoc signing. For distribution, use `./scripts/build-dmg.sh --sign` with a valid Apple Developer ID.
- **Hardened runtime**: The entitlements file disables app sandbox (network client entitlement is used instead) and enables JIT allowance for Rust runtime compatibility.
- **Input validation**: Token amount parsing rejects negative values, empty strings, and over-precision inputs. SS58 address validation is performed before chain queries.
- **No `unsafe` blocks** are used in the main Rust workspace (verified via search).

## Data Storage and Configuration

Config and cache are stored in OS-specific application data directories:

- **macOS**: `~/Library/Application Support/stkopt/`
- **Linux**: `~/.config/stkopt/`
- **Windows**: `%APPDATA%\stkopt\`

Files:
- `config.json` — User preferences, saved accounts, address book, theme, network selection.
- `history.db` — SQLite cache for staking history, validator identities, and chain metadata.

The `directories` crate resolves these paths. Both `stkopt-tui` and `stkopt-gpui` share the same config/database format via `stkopt-core`'s `persistence` feature.

## Deployment and Packaging

### macOS TUI
- **Standalone binary**: `target/release/stkopt`
- **App bundle**: `target/dmg/stkopt.app` (contains a launcher script that opens Terminal)
- **DMG**: `target/dmg/stkopt-<version>.dmg`
- The `scripts/build-dmg.sh` script handles binary signing, app bundle creation, dylib bundling, DMG creation, and optional notarization.
- Environment variables for signed builds: `DEVELOPER_ID`, `APPLE_ID`, `APPLE_APP_PASSWORD`, `APPLE_TEAM_ID`.

### iOS
- Build and distribute via Xcode after running `xcodegen generate`.
- No automated CI for iOS in this repository.

## CI/CD

GitHub Actions workflow: `.github/workflows/rust.yml`
- Triggers on `push` and `pull_request` to `main`.
- Runs on `ubuntu-latest`.
- Steps: `cargo build --verbose`, `cargo test --verbose`.
- No macOS or iOS CI jobs are present.

## Useful Constants and Domain Knowledge

- **Max nominations per nominator**: `16` (`MAX_NOMINATIONS` in `stkopt-core`).
- **Default max commission**: `0.15` (15%).
- **History depth**: `21` eras of on-chain reward data.
- **APY formula**: compound interest per era, annualized by `MS_PER_YEAR / era_duration_ms`.
- **Supported networks**: Polkadot (`DOT`, 10 decimals, SS58 prefix 0), Kusama (`KSM`, 12 decimals, prefix 2), Westend (`WND`, 12 decimals, prefix 42), Paseo (`PAS`, 10 decimals, prefix 0).
- **Connection modes**: `LightClient` (default, trustless via smoldot) and `Rpc` (traditional RPC endpoints). Use `--rpc` flag or `ConnectionMode::Rpc` to switch.

## Common Tasks for Agents

### Adding a New Network
1. Add variant to `Network` enum in `stkopt-core/src/types.rs`.
2. Update **all** `match` expressions on `Network` across the workspace (no catch-all arms).
3. Add endpoints / config in `stkopt-chain/src/config.rs`.
4. Update CLI help text and parser in `stkopt-tui/src/main.rs`.
5. Update iOS `Network.swift` if parity with mobile is desired.

### Adding a New Staking Operation
1. Define the operation in the chain request enum (e.g., `ChainRequest` in `stkopt-tui/src/main.rs` or equivalent in `stkopt-gpui`).
2. Add transaction building logic in `stkopt-chain/src/transactions.rs`.
3. Add UI action handling in `action.rs`, `app.rs`, and the relevant view module.
4. Add keyboard shortcut mapping if applicable.

### Adding a New Database Table / Column
1. Modify `stkopt-core/src/db.rs` (schema, migrations, query methods).
2. Both TUI and GPUI will pick it up automatically since they consume `stkopt-core` with the `persistence` feature.

### Running Integration Tests Against Live Networks
Integration tests in `stkopt-chain/tests/` connect to real RPC endpoints and may take 30–60 seconds for light client sync. They are not run by `just test` (which uses `--lib`). Run them explicitly with:
```bash
cargo test --test compare_connection_modes -- --ignored --nocapture
```
