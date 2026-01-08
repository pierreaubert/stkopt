# ----------------------------------------------------------------------
# Justfile for stkopt - Polkadot Staking Optimizer
# ----------------------------------------------------------------------

default:
	just --list

# ----------------------------------------------------------------------
# Build
# ----------------------------------------------------------------------
build-debug:
    cargo build

build:
    cargo build --release

test:
    cargo test --workspace --lib

check:
    cargo check --workspace

lint:
    cargo clippy --workspace

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

ci: fmt-check lint test

sign-macos:
    codesign --entitlements crates/stkopt-tui/entitlements.plist --deep -fs - target/release/stkopt

build-macos: build sign-macos

# Build signed DMG for macOS distribution (requires DEVELOPER_ID env var)
build-dmg:
    ./scripts/build-dmg.sh --sign

# Build unsigned DMG for local testing
build-dmg-unsigned:
    ./scripts/build-dmg.sh

run:
    cargo run --release

run-rpc:
    cargo run --release -- --rpc

watch:
    cargo watch -x check

profile-build:
    cargo build --release --timings

audit:
    cargo audit

# Show binary size
size:
    @ls -lh target/release/stkopt 2>/dev/null || echo "Release binary not found. Run 'just build' first."

example-connection:
    cargo run --release -p stkopt-chain --example test_connection

example-pallets:
    cargo run --release -p stkopt-chain --example check_pallets

# Update mode: fetch staking history for an account
update-history address eras="30":
    cargo run --release -- --update --address {{address}} --eras {{eras}}

# ----------------------------------------------------------------------
# POST
# ----------------------------------------------------------------------

install:
	rustup default stable
	cargo install just
	cargo install cargo-wizard
	cargo install cargo-audit
	cargo install cargo-watch
	cargo install cargo-llvm-cov
	cargo install cargo-llvm-lines
	cargo install cross
	cargo install cargo-binstall
	cargo binstall cargo-nextest --secure

