# stkopt - Polkadot Staking Optimizer

A terminal user interface (TUI) application for optimizing Polkadot staking. Browse validators, analyze APY history, manage nominations, and sign transactions with Polkadot Vault using QR codes.

## WARNING

- The code is new and there has been no security review. Use at your own risk.
- If you webcam does not have a depth sensor then it is hard to get the TUI to scan the QR code (you need to be very still).
- Subtleties like [this](https://forum.polkadot.network/t/validators-flipping-their-commision-twice-in-an-era-and-cheating-nominators/16569/3) are not yet implemented.

## Features

- **Light client by default**: Uses smoldot embedded light client for fully decentralized connectivity (no trusted RPC required)
- **Multi-network support**: Polkadot, Kusama, Westend
- **Validator browser**: View validators with APY, commission, and nomination counts
- **Nomination pools**: Browse pools with aggregated APY
- **Account status**: View balances, staking info, and nominations
- **Staking history**: Visualize APY over time with ASCII graphs
- **Nomination optimizer**: Automatically select optimal validators
- **Full staking operations**: Bond, unbond, rebond, withdraw, change reward destination, chill
- **Pool operations**: Join pools, bond extra, claim rewards, unbond, withdraw
- **QR code signing**: Generate transaction QR codes for Polkadot Vault
- **QR code scanning**: Scan signed transactions back from Vault using camera (with live preview)
- **Theme support**: Auto-detects dark/light terminal background
- **Batch mode**: Fetch and cache staking history from cron jobs

## Installation

### From source

Requires Rust 1.85 or later.

```bash
git clone https://github.com/dotidx/stkopt.git
cd stkopt
cargo install just
just build
```

The binary will be at `./target/release/stkopt`.

### Building for macOS with camera support

QR code scanning requires camera access. On macOS, the binary must be signed with entitlements:

```bash
# Install just (command runner)
cargo install just

# Build and sign for local use
just build-macos

# Or build a DMG for distribution
./scripts/build-dmg.sh
```

**Camera permissions:** When you first use the QR scanner, macOS will prompt for camera access.

### Building a signed DMG on MacOS

```bash
# Ad-hoc signed (local testing)
./scripts/build-dmg.sh

# Developer ID signed (distribution)
export DEVELOPER_ID="Developer ID Application: Your Name (TEAMID)"
./scripts/build-dmg.sh --sign

# With notarization (for Gatekeeper)
export DEVELOPER_ID="Developer ID Application: Your Name (TEAMID)"
export APPLE_ID="your@email.com"
./scripts/build-dmg.sh --notarize
```

Output:
- Standalone binary: `target/release/stkopt`
- App bundle: `target/dmg/stkopt.app`
- DMG: `target/dmg/stkopt-<version>.dmg`

## Struggling with permissions on MacOS

Normally the app should do all the hard work for you, but if it does not:

Open the terminal you want (Terminal, iTerm, Kitty, ...):
```bash
swift -e 'import AVFoundation; AVCaptureDevice.requestAccess(for: .video) { _ in }'
```
You should see a popup, click yes.
You can check if it is working by typing
```bash
sqlite3 ~/Library/Application\ Support/com.apple.TCC/TCC.db "SELECT client, * FROM access WHERE service='kTCCServiceCamera';"
```
You should see your Terminal in the list.

Note: the old advise to authorize Terminal in Preferences -> Security & Privacy -> Privacy -> Camera is not working anymore.

## Usage

```bash
# Run with default network (Polkadot) using light client
stkopt

# Run with specific network
stkopt --network kusama
stkopt --network westend
stkopt --network paseo

# Use traditional RPC instead of light client
# (recommended for historical queries or when light client has issues on 3G for example)
stkopt --rpc

# Use custom RPC endpoints
stkopt --relay-url wss://rpc.polkadot.io --rpc

# Show help
stkopt --help

# Batch mode: update staking history for an account, then exit
# Suitable for cron jobs or CI/CD pipelines
stkopt --update --address <ss58_address> --eras 30
```

## Command Line Options

| Option | Description |
|--------|-------------|
| `-n, --network <NETWORK>` | Network to connect to (polkadot, kusama, westend, paseo; default: polkadot) |
| `--relay-url <URL>` | Custom relay chain RPC endpoint URL |
| `--asset-hub-url <URL>` | Custom Asset Hub RPC endpoint URL |
| `--people-url <URL>` | Custom People chain RPC endpoint URL |
| `--rpc` | Use traditional RPC instead of light client (useful for historical queries) |
| `--update` | Batch mode: fetch staking history and exit (use with --address) |
| `-a, --address <ADDRESS>` | Account address for --update mode (SS58 format) |
| `-e, --eras <NUM>` | Number of eras to fetch in update mode (default: 30) |

## Keyboard Shortcuts

### Global

| Key | Action |
|-----|--------|
| `q` | Quit application |
| `Tab` | Next tab |
| `Shift+Tab` | Previous tab |
| `1-6` | Jump to tab |
| `↑/k`, `↓/j` | Navigate list |
| `?` | Toggle help |

### Account Tab

| Key | Action |
|-----|--------|
| `a` | Enter account address |
| `c` | Clear account |
| `n` | Switch network |

### Staking Tab (Tab 2)

| Key | Action |
|-----|--------|
| `b` | Bond funds |
| `u` | Unbond funds |
| `+` | Bond extra |
| `r` | Change reward destination |
| `w` | Withdraw unbonded |
| `x` | Chill (stop validating/nominating) |

### History Tab

| Key | Action |
|-----|--------|
| `l` | Load staking history |
| `c` | Cancel loading |

### Nominate Tab

| Key | Action |
|-----|--------|
| `o` | Run optimizer |
| `Space` | Toggle validator selection |
| `c` | Clear nominations |
| `g` | Generate QR code |

### Pools Tab

| Key | Action |
|-----|--------|
| `j` | Join selected pool |
| `J` | Bond extra to pool |
| `C` | Claim pool rewards |
| `U` | Unbond from pool |
| `W` | Withdraw from pool |

### Log Viewer

| Key | Action |
|-----|--------|
| `PgUp/PgDn` | Scroll logs |
| `End` | Jump to latest |

## Signing Transactions

stkopt generates unsigned transactions as QR codes compatible with [Polkadot Vault](https://signer.parity.io/):

1. Go to the Nominate tab (or use staking/pool operations)
2. Select validators (or run optimizer with `o`)
3. Press `g` to generate QR code
4. Scan with Polkadot Vault to sign
5. Switch to the "Scan" tab to scan the signed QR back with your camera
6. Submit the signed transaction

## Batch Mode (Cron Jobs)

For headless environments, use `--update` mode to fetch and cache staking history:

```bash
# Fetch 30 eras of history for an account
stkopt --update --address 15oF4uVJwmo4TdGW7VfQxELSav3wstvDAb7E7V9VFEJDvY6 --eras 30

# Cron job example (run daily at 2 AM):
# 0 2 * * * /usr/local/bin/stkopt --update --address <your_address> --eras 30 >> /var/log/stkopt.log 2>&1
```

Data is stored in the application data directory and loaded automatically when running the TUI.

## Project Structure

```
stkopt/
├── crates/
│   ├── stkopt-chain/   # Chain client (subxt)
│   ├── stkopt-core/    # Domain logic (APY calculations, optimizer)
│   └── stkopt-tui/     # TUI application
├── scripts/
│   ├── build-dmg.sh    # macOS DMG builder
│   ├── Info.plist      # macOS app bundle metadata
│   └── entitlements.plist  # macOS entitlements (camera access)
```

## Development

```bash
# Install just (command runner)
cargo install just

# List all commands
just --list

# Common commands
just build          # Build release binary
just build-macos    # Build and sign for macOS (with camera support)
just run            # Build, sign, and run
just test           # Run tests
just check          # Quick compile check
just lint           # Run clippy
just fmt            # Format code
```

## Configuration

Configuration and cache are stored in:
- Linux: `~/.config/stkopt/`
- macOS: `~/Library/Application Support/stkopt/`
- Windows: `C:\Users\<user>\AppData\Roaming\stkopt\`

Files:
- `config.json` - Application configuration (accounts, settings)
- `history.db` - Cached staking history (SQLite)

## License

ISC License. See [LICENSE](LICENSE) for details.
