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

# Show help
stkopt --help
```

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

## Project Structure

```
stkopt/
├── crates/
│   ├── stkopt-chain/   # Chain client (subxt)
│   ├── stkopt-core/    # Domain logic (APY calculations, optimizer)
│   └── stkopt-tui/     # TUI application
```

## Configuration

Configuration is stored in:
- Linux: `~/.config/stkopt/config.json`
- macOS: `~/Library/Application Support/stkopt/config.json`
- Windows: `C:\Users\<user>\AppData\Roaming\stkopt\config.json`

## License

ISC License. See [LICENSE](LICENSE) for details.
