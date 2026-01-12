# GPUI Desktop App - Implementation Plan

This document tracks the feature parity between the TUI app (`stkopt-tui`) and the GPUI desktop app (`stkopt-gpui`).

## Feature Comparison

### Core Infrastructure

| Feature | TUI | GPUI | Notes |
|---------|-----|------|-------|
| Network selection (Polkadot/Kusama/Westend) | ✅ | ✅ | Wired to chain connection |
| Connection mode (RPC/Light Client) | ✅ | ✅ | Wired to chain connection |
| Chain client integration | ✅ | ✅ | Uses `stkopt-chain` via `chain.rs` |
| Connection status display | ✅ | ✅ | Real-time status updates |
| Theme support (light/dark) | ✅ | ✅ | Via gpui-ui-kit MiniApp |
| Configuration persistence | ✅ | ✅ | Saves/loads via `persistence.rs` |
| Database for history cache | ✅ | ✅ | SQLite via `db.rs` and `db_service.rs` |
| CLI arguments | ✅ | ❌ | Not needed for GUI app |
| Logging | ✅ | ✅ | Log viewer with Cmd+L/Ctrl+L |

### Views/Tabs

| View | TUI | GPUI | Notes |
|------|-----|------|-------|
| Account Status | ✅ | ✅ | Shows watched account details |
| Account Changes | ✅ | ❌ | Not implemented |
| Account History | ✅ | 🔶 | Chart works, history fetch incomplete |
| Nominate | ✅ | ✅ | Called "Optimization", fully functional |
| Validators | ✅ | ✅ | Shows real validator data from chain |
| Pools | ✅ | ✅ | Shows real pool data from chain |

### Account Management

| Feature | TUI | GPUI | Notes |
|---------|-----|------|-------|
| Watch account by address | ✅ | ✅ | Input wired with validation |
| Address validation | ✅ | ✅ | SS58 validation in `account.rs` |
| Address book (saved accounts) | ✅ | 🔶 | Types exist, no UI |
| Account balance display | ✅ | ✅ | Fetches on watch, shows in dashboard |
| Staking ledger display | ✅ | ✅ | Bonded amount shown in dashboard |
| Nomination info display | ✅ | 🔶 | Data fetched, count shown |
| Pool membership display | ✅ | 🔶 | Data fetched but not displayed |
| Auto-restore last account | ✅ | ✅ | Loads from config on startup |

### Validators

| Feature | TUI | GPUI | Notes |
|---------|-----|------|-------|
| Validator list display | ✅ | ✅ | Shows real data from chain |
| Validator search/filter | ✅ | ✅ | Search input wired to filter logic |
| Validator sorting | ✅ | ✅ | Clickable column headers |
| Show/hide blocked validators | ✅ | ❌ | Not implemented |
| Validator selection | ✅ | 🔶 | Via optimization only, no manual selection |
| Validator details (commission, stake, APY) | ✅ | ✅ | Displayed in table |
| Identity names | ✅ | ❌ | TODO in chain.rs |
| Cached validators | ✅ | ✅ | Database caching implemented |

### Nomination Pools

| Feature | TUI | GPUI | Notes |
|---------|-----|------|-------|
| Pool list display | ✅ | ✅ | Shows real data from chain |
| Pool search/filter | ✅ | ❌ | Not implemented |
| Pool sorting | ✅ | ❌ | Not implemented |
| Pool details (members, state, APY) | ✅ | ✅ | Displayed in table |
| Pool selection for join | ✅ | ❌ | Not implemented |

### Optimization

| Feature | TUI | GPUI | Notes |
|---------|-----|------|-------|
| Strategy selection | ✅ | 🔶 | UI shows options, only TopApy wired |
| Top APY strategy | ✅ | ✅ | Fully implemented |
| Random from Top strategy | ✅ | ✅ | Logic exists in `optimization.rs` |
| Diversify by Stake strategy | ✅ | ✅ | Logic exists in `optimization.rs` |
| Max validators parameter | ✅ | 🔶 | UI shows value, not editable |
| Max commission parameter | ✅ | 🔶 | UI shows value, not editable |
| Min self-stake parameter | ✅ | ❌ | Not shown in UI |
| Run optimization | ✅ | ✅ | Button wired and working |
| Display optimization results | ✅ | ✅ | Shows selected validators and avg APY |

### Staking History

| Feature | TUI | GPUI | Notes |
|---------|-----|------|-------|
| History chart | ✅ | ✅ | Line chart via gpui-px |
| Era-by-era rewards | ✅ | 🔶 | UI ready, fetch returns error |
| APY calculation | ✅ | ✅ | Stats computed from history |
| History loading progress | ✅ | ❌ | Not implemented |
| History caching | ✅ | ✅ | Database caching implemented |

### Transactions (Polkadot Vault Integration)

| Feature | TUI | GPUI | Notes |
|---------|-----|------|-------|
| QR code generation | ✅ | ✅ | Payload generation in `transactions.rs` |
| QR code display | ✅ | 🔶 | Shows hex payload, no visual QR |
| Camera QR scanning | ✅ | ❌ | Has nokhwa dependency, not implemented |
| Transaction signing flow | ✅ | 🔶 | Payload generation works, no signed response handling |
| Transaction submission | ✅ | ❌ | Deferred to live chain integration |
| Transaction status tracking | ✅ | ❌ | Not implemented |

### Staking Operations

| Feature | TUI | GPUI | Notes |
|---------|-----|------|-------|
| Bond | ✅ | ❌ | Not implemented |
| Unbond | ✅ | ❌ | Not implemented |
| Bond Extra | ✅ | ❌ | Not implemented |
| Set Payee | ✅ | ❌ | Not implemented |
| Withdraw Unbonded | ✅ | ❌ | Not implemented |
| Chill | ✅ | ❌ | Not implemented |
| Nominate | ✅ | ❌ | Not implemented |

### Pool Operations

| Feature | TUI | GPUI | Notes |
|---------|-----|------|-------|
| Join Pool | ✅ | ❌ | Not implemented |
| Bond Extra to Pool | ✅ | ❌ | Not implemented |
| Claim Pool Rewards | ✅ | ❌ | Not implemented |
| Unbond from Pool | ✅ | ❌ | Not implemented |
| Withdraw from Pool | ✅ | ❌ | Not implemented |

## Implementation Phases

### Phase 1: Chain Integration (Priority: High) ✅ COMPLETED
1. ✅ Add `stkopt-chain` dependency
2. ✅ Create async runtime for chain operations (`chain_service.rs`)
3. ✅ Implement connection management (connect/disconnect) - mock service
4. ✅ Wire up connection status updates via Action enum
5. ⏳ Add loading indicators during chain sync (deferred to UI phase)

**Tests Added (30 total):**
- Unit tests for Action enum, ChainCommand, ChainService
- Async tests for connect/disconnect flow
- Property-based tests (proptest) for action roundtrips
- Negative tests for disconnect when not connected

### Phase 2: Account Data (Priority: High) ✅ COMPLETED
1. ✅ Implement account address validation (SS58) - `account.rs`
2. ✅ Wire up "Watch" button to validate and set account
3. ⏳ Display account balance breakdown (deferred - needs chain data)
4. ⏳ Display staking ledger (deferred - needs chain data)
5. ⏳ Display current nominations (deferred - needs chain data)
6. ⏳ Display pool membership (deferred - needs chain data)

**Tests Added (19 new, 53 total):**
- Unit tests for SS58 validation (valid, invalid, empty)
- Proptest for validation consistency
- Negative tests for edge cases (unicode, null bytes, wrong checksum)
- Action tests for account-related actions

### Phase 3: Validators & Pools (Priority: High) ✅ COMPLETED
1. ✅ Validator data types and mock data generation
2. ✅ Implement validator sorting (by name, commission, stake, APY, etc.)
3. ✅ Implement validator search/filter
4. ✅ Pool data types (PoolInfo, PoolState)
5. ⏳ Pool table with sorting (deferred - needs UI integration)
6. ⏳ Identity name resolution (deferred - needs chain data)

**Tests Added (15 new, 58 total):**
- Unit tests for sorting by all columns (ascending/descending)
- Unit tests for filtering by name and address
- Unit tests for format_stake, format_commission, format_apy
- Proptest for sort/filter invariants

### Phase 4: Optimization (Priority: Medium) ✅ COMPLETED
1. ✅ Optimization criteria types (max_commission, exclude_blocked, target_count)
2. ✅ Selection strategies (TopApy, RandomFromTop, DiversifyByStake, MinCommission)
3. ✅ Optimization result with APY stats
4. ✅ Criteria validation
5. ✅ UI integration (optimization runs and displays results)

**Tests Added (16 new, 74 total):**
- Unit tests for all strategies
- Tests for filtering (blocked, high commission)
- Tests for statistics calculation
- Proptest for invariants (never exceeds target, valid indices)

### Phase 5: History & Charts (Priority: Medium) ✅ COMPLETED
1. ✅ History data types (HistoryPoint, HistoryRange, HistoryStats)
2. ✅ Mock history data generation
3. ✅ Statistics computation (total rewards, avg APY, min/max)
4. ✅ Cumulative rewards and moving average APY
5. ✅ Chart integration with gpui-px (APY trend line chart)

**Tests Added (19 new, 93 total):**
- Unit tests for stats computation
- Unit tests for filtering by range
- Unit tests for cumulative/moving average
- Proptest for invariants

### Phase 6: Transactions (Priority: Medium) ✅ COMPLETED
1. ✅ Transaction types (Nominate, Bond, Unbond, Pool operations)
2. ✅ Transaction builders (nominate, bond, unbond)
3. ✅ QR payload encoding for Polkadot Vault
4. ✅ Signed QR parsing
5. ✅ Transaction validation
6. ⏳ Chain submission (deferred - needs live chain)

**Tests Added (26 new, 119 total):**
- Unit tests for all transaction types
- Tests for tx builders
- Tests for QR payload encoding
- Tests for validation
- Proptest for invariants

### Phase 7: Persistence (Priority: Low) ✅ COMPLETED
1. ✅ AppConfig with network, theme, connection mode
2. ✅ AddressBook with add/remove/find/update
3. ✅ ValidatorCache with staleness detection
4. ✅ HistoryCache with update detection
5. ✅ File I/O utilities (load/save config and address book)

**Tests Added (18 new, 137 total):**
- Unit tests for config/address book
- Tests for cache staleness
- Proptest for address book operations

### Phase 8: Polish ✅ COMPLETED
1. ✅ Keyboard shortcuts (?, Esc, Cmd+,/Ctrl+,, Cmd+L/Ctrl+L for logs)
2. ✅ Error handling module with AppError, ErrorSeverity, Notification types
3. ✅ Help overlay (press ? to toggle)
4. ✅ Log viewer overlay (press Cmd+L/Ctrl+L to toggle)
5. Performance optimization (deferred - no bottlenecks yet)
6. Accessibility improvements (deferred - basic structure in place)

**Tests Added (11 new, 155 total):**
- Unit tests for AppError category/message/recoverable/suggestion
- Tests for ErrorSeverity labels and icons
- Tests for Notification constructors

### Phase 9: Settings Page ✅ COMPLETED
1. ✅ Settings view with General, Network, and Keyboard Shortcuts sections
2. ✅ Platform-specific keyboard shortcuts (Cmd+, on macOS, Ctrl+, on Linux/Windows)
3. ✅ Escape key to close settings
4. ✅ Theme, network, connection mode selectors
5. ✅ Shortcuts module with display labels

**Tests Added (3 new, 158 total):**
- Unit tests for shortcut display/labels
- Tests for shortcuts_by_category

## Architecture Notes

### State Management
The GPUI app uses a single `StkoptApp` struct as the root state, similar to the TUI's `App` struct. State updates should be done through the GPUI entity system using `cx.notify()` to trigger re-renders.

### Async Operations
Chain operations are async and should be spawned as background tasks. Use channels or GPUI's async primitives to communicate results back to the UI.

### Component Structure
```
StkoptApp (root)
├── Sidebar (navigation)
└── Content Area
    ├── DashboardSection
    ├── AccountSection
    ├── ValidatorsSection
    ├── OptimizationSection
    ├── PoolsSection
    └── HistorySection
```

### Key Dependencies
- `gpui` - UI framework
- `gpui-ui-kit` - UI components (Card, Button, Input, etc.)
- `gpui-px` - Plotting/charts (for history)
- `stkopt-chain` - Blockchain client
- `stkopt-core` - Core types and optimization logic

## Legend
- ✅ Fully implemented
- 🔶 Partially implemented (UI exists but not functional)
- ❌ Not implemented
