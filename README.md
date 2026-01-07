# stkopt - Polkadot Staking Optimizer

A terminal user interface (TUI) application for optimizing Polkadot staking. Browse validators, analyze APY history, manage nominations, and sign transactions with Polkadot Vault using QR codes.

## Features

- **Multi-network support**: Polkadot, Kusama, Westend, Paseo
- **Validator browser**: View validators with APY, commission, and nomination counts
- **Nomination pools**: Browse pools with aggregated APY
- **Account status**: View balances, staking info, and nominations
- **Staking history**: Visualize APY over time with ASCII graphs
- **Nomination optimizer**: Automatically select optimal validators
- **QR code signing**: Generate transaction QR codes for Polkadot Vault
- **Theme support**: Auto-detects dark/light terminal background
- **Batch mode**: Fetch and cache staking history from cron jobs

## Installation

### From source

Requires Rust 1.85 or later.

```bash
git clone https://github.com/example/stkopt.git
cd stkopt
cargo build --release
```

The binary will be at `./target/release/stkopt`.

## Usage

```bash
# Run with default network (Polkadot)
stkopt

# Run with specific network
stkopt --network kusama
stkopt --network westend
stkopt --network paseo

# Use custom RPC endpoint
stkopt --network kusama --url wss://kusama-rpc.polkadot.io

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
| `-u, --url <URL>` | Custom RPC endpoint URL (overrides default endpoints) |
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
| `1-5` | Jump to tab |
| `↑/k`, `↓/j` | Navigate list |
| `?` | Toggle help |

### Account Tab

| Key | Action |
|-----|--------|
| `a` | Enter account address |
| `c` | Clear account |

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

### Log Viewer

| Key | Action |
|-----|--------|
| `PgUp/PgDn` | Scroll logs |
| `End` | Jump to latest |

## Signing Transactions

stkopt generates unsigned transactions as QR codes compatible with [Polkadot Vault](https://signer.parity.io/):

1. Go to the Nominate tab
2. Select validators (or run optimizer with `o`)
3. Press `g` to generate QR code
4. Scan with Polkadot Vault to sign
5. Scan the signed QR back (coming soon)

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
