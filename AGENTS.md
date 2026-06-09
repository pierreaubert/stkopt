# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build Commands

```bash
# Using just (recommended - run `just` to see all commands)
just check          # Fast compile check
just lint           # Run clippy
just test           # Run tests
just build          # Build release binary
just build-macos    # Build and sign for macOS (camera support)
just run            # Build, sign, and run

# Raw cargo commands
cargo check --workspace
cargo clippy --workspace
cargo test --workspace --lib
cargo test -p stkopt-core              # Single crate
cargo test -p stkopt-core test_name    # Specific test

# Run the application
cargo run --release
cargo run --release -- --network kusama
cargo run --release -- --rpc           # Use RPC instead of light client

# Examples
just example-connection                # Test chain connection
just example-pallets                   # Check pallet compatibility
```

## Architecture

Polkadot/Kusama staking optimizer with TUI and desktop GUI. The workspace has four crates:

### stkopt-core
Pure domain logic with no external dependencies beyond `thiserror`:
- `apy.rs` - APY calculations from staking rewards
- `optimizer.rs` - Validator selection algorithm
- `types.rs` - Shared data structures (ValidatorInfo, PoolInfo, etc.)

### stkopt-chain
Blockchain client using `subxt` with light client support (smoldot):
- `client.rs` - Main RPC client wrapper
- `config.rs` - Network configuration (endpoints, SS58 prefix)
- `queries/` - Chain queries (account, era, identity, pools, validators)
- `transactions.rs` - Transaction building for nominations

### stkopt-tui
TUI application using `ratatui`:
- `main.rs` - Entry point, async runtime, background chain task
- `app.rs` - Application state and UI logic
- `ui.rs` - Widget rendering
- `db.rs` - SQLite schema for caching staking history
- `config.rs` - User configuration (JSON)
- `theme.rs` - Dark/light terminal theme detection

### stkopt-gpui
Desktop GUI application using GPUI (Zed's framework):
- `main.rs` - Entry point, GPUI app initialization
- `app.rs` - Main application component and state
- `chain.rs` - Async chain interaction integration
- `views/` - UI views (account, validators, history, etc.)
- `db.rs`, `db_service.rs` - SQLite persistence layer
- `gpui_tokio.rs` - Bridge between GPUI and tokio async runtime

## Key Patterns

- **Async channels**: `tokio::sync::mpsc` channels connect UI thread to background chain task
- **Logging**: Use `tracing` macros, not `println!` (except CLI-only modes)
- **Error handling**: Propagate with `Result`, use `thiserror` for custom errors
- **No default match arms**: Crash hard on unknown values rather than silently handling
- **Light client first**: Uses smoldot by default; `--rpc` flag for traditional RPC

## Data Storage

Config and cache stored in OS-specific app data directory (`~/Library/Application Support/stkopt/` on macOS):
- `config.json` - User preferences
- `history.db` - SQLite cache for staking history
