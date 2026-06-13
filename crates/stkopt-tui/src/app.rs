//! Application state and logic.

use crate::action::{
    AccountStatus, Action, DisplayPool, DisplayValidator, PendingTransaction, PendingUnsignedTx,
    QrScanStatus, StakingHistoryPoint, StakingInputMode, TransactionInfo, TxSubmissionStatus,
};
use crate::log_buffer::LogBuffer;
use crate::theme::{Palette, Theme};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::TableState;
use std::collections::HashSet;
use std::sync::Arc;
use stkopt_chain::{ChainInfo, RewardDestination};
use stkopt_core::{ConnectionStatus, Network, OptimizationResult};
use subxt::utils::AccountId32;

/// Known address book entries (name, address).
pub const KNOWN_ADDRESSES: &[(&str, &str)] = &[
    (
        "Polkadot Treasury",
        "13UVJyLnbVp9RBZYFwCNuGnK87JYJ2nb7jMwaVe4vQ2UNCzN",
    ),
    (
        "Polkadot Fellowship",
        "16SpacegeUTft9v3ts27CEC3tJaxgvE4uZeCctThFH3Vb24p",
    ),
    (
        "Snowbridge",
        "13cKp89Nt7t1hZVWnqhKW9LY7Udhxk2BmLwKi3snVgUAjZGE",
    ),
];

/// Camera scan status for visual feedback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraScanStatus {
    /// Camera initializing.
    Initializing,
    /// Scanning, no QR code detected.
    Scanning,
    /// QR code detected but not decoded yet.
    Detected,
    /// Successfully decoded QR code.
    Success,
    /// Camera error.
    Error,
}

/// Input mode for the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    #[default]
    Normal,
    /// Entering account address.
    EnteringAccount,
    /// Searching in validators/pools.
    Searching,
    /// Showing sort menu.
    SortMenu,
    /// Showing strategy menu in nominate view.
    StrategyMenu,
    /// Handling staking/pool operation inputs.
    Staking,
}

/// Sort field for validators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ValidatorSortField {
    Name,
    Address,
    Commission,
    TotalStake,
    OwnStake,
    Points,
    Nominators,
    #[default]
    Apy,
    Blocked,
}

impl ValidatorSortField {
    pub fn all() -> &'static [Self] {
        &[
            Self::Name,
            Self::Address,
            Self::Commission,
            Self::TotalStake,
            Self::OwnStake,
            Self::Points,
            Self::Nominators,
            Self::Apy,
            Self::Blocked,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Address => "Address",
            Self::Commission => "Commission",
            Self::TotalStake => "Total Stake",
            Self::OwnStake => "Own Stake",
            Self::Points => "Points",
            Self::Nominators => "Nominators",
            Self::Apy => "APY",
            Self::Blocked => "Blocked",
        }
    }

    pub fn key(&self) -> char {
        match self {
            Self::Name => 'n',
            Self::Address => 'a',
            Self::Commission => 'c',
            Self::TotalStake => 't',
            Self::OwnStake => 'o',
            Self::Points => 'p',
            Self::Nominators => 'm',
            Self::Apy => 'y',
            Self::Blocked => 'b',
        }
    }

    pub fn from_key(key: char) -> Option<Self> {
        Self::all().iter().find(|f| f.key() == key).copied()
    }
}

/// Sort field for pools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PoolSortField {
    Id,
    Name,
    State,
    Members,
    Points,
    #[default]
    Apy,
}

impl PoolSortField {
    pub fn all() -> &'static [Self] {
        &[
            Self::Id,
            Self::Name,
            Self::State,
            Self::Members,
            Self::Points,
            Self::Apy,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Id => "ID",
            Self::Name => "Name",
            Self::State => "State",
            Self::Members => "Members",
            Self::Points => "Points",
            Self::Apy => "APY",
        }
    }

    pub fn key(&self) -> char {
        match self {
            Self::Id => 'i',
            Self::Name => 'n',
            Self::State => 's',
            Self::Members => 'm',
            Self::Points => 'p',
            Self::Apy => 'y',
        }
    }

    pub fn from_key(key: char) -> Option<Self> {
        Self::all().iter().find(|f| f.key() == key).copied()
    }
}

/// Current view/tab in the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    AccountStatus,
    AccountChanges,
    AccountHistory,
    Nominate,
    Validators,
    Pools,
}

impl View {
    pub fn all() -> &'static [View] {
        &[
            View::AccountStatus,
            View::AccountChanges,
            View::AccountHistory,
            View::Nominate,
            View::Validators,
            View::Pools,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            View::AccountStatus => "Account Status",
            View::AccountChanges => "Account Changes",
            View::AccountHistory => "Account History",
            View::Nominate => "Nominate",
            View::Validators => "Validators",
            View::Pools => "Pools",
        }
    }

    pub fn index(&self) -> usize {
        match self {
            View::AccountStatus => 0,
            View::AccountChanges => 1,
            View::AccountHistory => 2,
            View::Nominate => 3,
            View::Validators => 4,
            View::Pools => 5,
        }
    }

    pub fn from_index(index: usize) -> View {
        match index {
            0 => View::AccountStatus,
            1 => View::AccountChanges,
            2 => View::AccountHistory,
            3 => View::Nominate,
            4 => View::Validators,
            5 => View::Pools,
            _ => View::AccountStatus,
        }
    }
}

// === Grouped State Structs ===

/// Camera scanning state for QR code reading.
#[derive(Debug, Default)]
pub struct CameraState {
    /// Whether currently scanning for signature QR code.
    pub scanning: bool,
    /// Camera scan status for visual feedback.
    pub status: Option<CameraScanStatus>,
    /// Number of frames captured since scanning started.
    pub frames_captured: u32,
    /// Camera preview pixels for braille rendering (grayscale, downsampled).
    pub preview: Option<Vec<u8>>,
    /// Camera preview dimensions (width, height).
    pub preview_size: (usize, usize),
    /// QR code bounding box in normalized coordinates (0.0-1.0).
    pub qr_bounds: Option<[(f32, f32); 4]>,
}

/// QR code display and transaction state.
#[derive(Debug, Default)]
pub struct QrState {
    /// QR code data to display (if any).
    pub data: Option<Vec<u8>>,
    /// Transaction info for QR code display.
    pub tx_info: Option<TransactionInfo>,
    /// Current QR frame for animated multipart display.
    pub frame: usize,
    /// Whether showing QR code modal.
    pub showing: bool,
    /// Current tab in QR modal (0=QR, 1=Details, 2=Scan).
    pub modal_tab: usize,
    /// Pending unsigned transaction (waiting for signature from Vault).
    pub pending_unsigned: Option<PendingUnsignedTx>,
    /// Pending signed transaction (ready for/in-progress submission).
    pub pending_signed: Option<PendingTransaction>,
}

/// Staking history state.
#[derive(Debug, Default)]
pub struct HistoryState {
    /// Staking history for the watched account.
    pub points: Vec<StakingHistoryPoint>,
    /// Whether staking history is currently loading.
    pub loading: bool,
    /// Number of days to load when requesting recent history.
    pub lookback_days: u32,
    /// Total eras to load for history.
    pub total_eras: u32,
    /// Account for which history was loaded (to detect account changes).
    pub loaded_for: Option<String>,
}

impl HistoryState {
    fn new() -> Self {
        Self {
            lookback_days: 30,
            total_eras: 30,
            ..Default::default()
        }
    }
}

/// Loading progress and bandwidth state.
#[derive(Debug, Default)]
pub struct LoadingState {
    /// Loading progress (0.0 - 1.0).
    pub progress: f32,
    /// Whether validators are loading.
    pub validators: bool,
    /// Whether the chain is still connecting/syncing.
    pub chain: bool,
    /// Whether data was loaded from cache (validators, etc).
    pub using_cache: bool,
    /// Bytes loaded so far (for bandwidth estimation).
    pub bytes_loaded: u64,
    /// Load start time (for bandwidth calculation).
    pub start_time: Option<std::time::Instant>,
    /// Estimated bandwidth in bytes per second.
    pub bandwidth: Option<f64>,
    /// Tick counter for loading spinner animation.
    pub spinner_tick: usize,
}

/// Application state.
pub struct App {
    // === Core State ===
    /// Current theme.
    pub theme: Theme,
    /// Color palette for rendering.
    pub palette: Palette,
    /// Current network.
    pub network: Network,
    /// Connection status.
    pub connection_status: ConnectionStatus,
    /// Chain info (name, spec version, validation).
    pub chain_info: Option<ChainInfo>,
    /// Current view/tab.
    pub current_view: View,
    /// Current input mode.
    pub input_mode: InputMode,
    /// Whether showing help overlay.
    pub showing_help: bool,
    /// Whether the app should quit.
    pub should_quit: bool,
    /// Tick counter for animations.
    tick_count: u64,
    /// Log buffer for displaying logs.
    pub log_buffer: LogBuffer,
    /// Log scroll offset (0 = bottom/most recent).
    pub log_scroll: usize,

    // === Era State ===
    /// Current era index.
    pub current_era: Option<u32>,
    /// Era completion percentage.
    pub era_pct_complete: f64,
    /// Era duration in milliseconds.
    pub era_duration_ms: u64,

    // === Validators State ===
    /// Display validators (aggregated data).
    pub validators: Vec<DisplayValidator>,
    /// Validators table state.
    pub validators_table_state: TableState,
    /// Manually selected validators (indices into validators list).
    pub selected_validators: HashSet<usize>,
    /// Nominate table state.
    pub nominate_table_state: TableState,
    /// Search query for filtering.
    pub search_query: String,
    /// Whether to show blocked validators.
    pub show_blocked: bool,
    /// Validator sort field.
    pub validator_sort: ValidatorSortField,
    /// Validator sort ascending (false = descending).
    pub validator_sort_asc: bool,
    /// Nomination optimization result.
    pub optimization_result: Option<OptimizationResult>,
    /// Status message for nomination optimization and QR generation.
    pub nomination_status: Option<String>,
    /// Optimization strategy selection index.
    pub strategy_index: usize,

    // === Cached Filtered Lists ===
    /// Cached filtered and sorted validators.
    cached_filtered_validators: Arc<Vec<DisplayValidator>>,
    /// Whether the validator filter cache needs recomputation.
    validators_cache_dirty: bool,
    /// Cached filtered and sorted pools.
    cached_filtered_pools: Arc<Vec<DisplayPool>>,
    /// Whether the pool filter cache needs recomputation.
    pools_cache_dirty: bool,

    // === Pools State ===
    /// Display nomination pools (aggregated data).
    pub pools: Vec<DisplayPool>,
    /// Pools table state.
    pub pools_table_state: TableState,
    /// Pool sort field.
    pub pool_sort: PoolSortField,
    /// Pool sort ascending (false = descending).
    pub pool_sort_asc: bool,

    // === Account State ===
    /// Watched account address.
    pub watched_account: Option<AccountId32>,
    /// Account status (balance, staking, nominations).
    pub account_status: Option<AccountStatus>,
    /// Input buffer for entering account address.
    pub account_input: String,
    /// Currently focused panel in account view (0 = status, 1 = address book).
    pub account_panel_focus: usize,
    /// Address book table state.
    pub address_book_state: TableState,
    /// Whether to show the account input prompt popup.
    pub show_account_prompt: bool,
    /// Validation error message for account input.
    pub validation_error: Option<String>,

    // === Grouped State ===
    /// QR code display and transaction state.
    pub qr: QrState,
    /// Camera scanning state.
    pub camera: CameraState,
    /// Staking history state.
    pub history: HistoryState,
    /// Loading progress state.
    pub loading: LoadingState,

    // === Staking Operations State ===
    /// Current staking input mode.
    pub staking_input_mode: StakingInputMode,
    /// Input buffer for staking amount.
    pub staking_input_amount: String,
    /// Selected rewards destination.
    pub rewards_destination: RewardDestination,

    // === Pool Operations State ===
    /// Input buffer for pool amount.
    pub pool_input_amount: String,
    /// Selected pool for join operation.
    pub selected_pool_for_join: Option<usize>,
}

/// Direction for cycling selection.
enum Direction {
    Previous,
    Next,
}

/// Cycle a list selection, wrapping around at the ends.
fn cycle_selection(current: Option<usize>, len: usize, direction: Direction) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match direction {
        Direction::Previous => match current {
            Some(i) => {
                if i == 0 {
                    len - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        },
        Direction::Next => match current {
            Some(i) => {
                if i >= len - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        },
    })
}

impl App {
    /// Create a new application instance.
    pub fn new(network: Network, log_buffer: LogBuffer, theme: Theme) -> Self {
        let palette = theme.palette();
        Self {
            // Core state
            theme,
            palette,
            network,
            connection_status: ConnectionStatus::Disconnected,
            chain_info: None,
            current_view: View::default(),
            input_mode: InputMode::default(),
            showing_help: false,
            should_quit: false,
            tick_count: 0,
            log_buffer,
            log_scroll: 0,

            // Era state
            current_era: None,
            era_pct_complete: 0.0,
            era_duration_ms: 0,

            // Validators state
            validators: Vec::new(),
            validators_table_state: TableState::default(),
            selected_validators: HashSet::new(),
            nominate_table_state: TableState::default(),
            search_query: String::new(),
            show_blocked: true,
            validator_sort: ValidatorSortField::default(),
            validator_sort_asc: false, // Default descending (highest APY first)
            optimization_result: None,
            nomination_status: None,
            strategy_index: 0,

            // Cached filtered lists
            cached_filtered_validators: Arc::new(Vec::new()),
            validators_cache_dirty: true,
            cached_filtered_pools: Arc::new(Vec::new()),
            pools_cache_dirty: true,

            // Pools state
            pools: Vec::new(),
            pools_table_state: TableState::default(),
            pool_sort: PoolSortField::default(),
            pool_sort_asc: false,

            // Account state
            watched_account: None,
            account_status: None,
            account_input: String::new(),
            account_panel_focus: 0,
            address_book_state: TableState::default(),
            show_account_prompt: false,
            validation_error: None,

            // Grouped state
            qr: QrState::default(),
            camera: CameraState::default(),
            history: HistoryState::new(),
            loading: LoadingState {
                chain: true, // Start in loading state
                start_time: Some(std::time::Instant::now()),
                ..Default::default()
            },

            // Staking operation fields
            staking_input_mode: StakingInputMode::default(),
            staking_input_amount: String::new(),
            rewards_destination: RewardDestination::Staked,
            pool_input_amount: String::new(),
            selected_pool_for_join: None,
        }
    }

    /// Scroll logs up (older messages).
    pub fn scroll_logs_up(&mut self) {
        let log_count = self.log_buffer.len();
        if log_count > 3 {
            self.log_scroll = (self.log_scroll + 1).min(log_count.saturating_sub(3));
        }
    }

    /// Scroll logs down (newer messages).
    pub fn scroll_logs_down(&mut self) {
        self.log_scroll = self.log_scroll.saturating_sub(1);
    }

    /// Scroll logs to bottom (most recent).
    pub fn scroll_logs_to_bottom(&mut self) {
        self.log_scroll = 0;
    }

    /// Handle tick events for animations and updates.
    pub fn tick(&mut self) {
        self.tick_count = self.tick_count.wrapping_add(1);

        // Advance QR animation frame every tick (~100ms) for fast multipart display
        if self.qr.showing {
            self.qr.frame = self.qr.frame.wrapping_add(1);
        }

        // Advance spinner for loading animation
        if self.loading.chain || self.loading.validators {
            self.loading.spinner_tick = self.loading.spinner_tick.wrapping_add(1);
        }

        // Bandwidth is now computed from actual bytes in SetLoadingProgress handler
    }

    /// Get the current spinner character for loading animation.
    pub fn spinner_char(&self) -> char {
        const SPINNER_CHARS: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        SPINNER_CHARS[self.loading.spinner_tick % SPINNER_CHARS.len()]
    }

    /// Estimate remaining time in seconds based on progress rate.
    pub fn estimated_remaining_secs(&self) -> Option<f64> {
        let bw = self.loading.bandwidth?;
        if bw <= 0.0 {
            return None;
        }

        // bw is progress rate * 10MB, so reverse to get progress rate
        let progress_rate = bw / 10_000_000.0;
        let remaining_progress = 1.0 - self.loading.progress as f64;
        if progress_rate > 0.0 {
            Some(remaining_progress / progress_rate)
        } else {
            None
        }
    }

    /// Format estimated time remaining as a human-readable string.
    pub fn format_eta(&self) -> Option<String> {
        let secs = self.estimated_remaining_secs()?;
        if secs < 60.0 {
            Some(format!("~{:.0}s", secs))
        } else if secs < 3600.0 {
            Some(format!("~{:.0}m {:.0}s", secs / 60.0, secs % 60.0))
        } else {
            Some(format!("~{:.0}h", secs / 3600.0))
        }
    }

    /// Handle keyboard input.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        if key.code == KeyCode::Enter {
            tracing::info!(
                "[KEY] Enter received: input_mode={:?}, view={:?}, panel={}, qr={}, help={}",
                self.input_mode,
                self.current_view,
                self.account_panel_focus,
                self.qr.showing,
                self.showing_help,
            );
        }
        match self.input_mode {
            InputMode::Normal => self.handle_normal_key(key),
            InputMode::EnteringAccount => self.handle_input_key(key),
            InputMode::Searching => self.handle_search_key(key),
            InputMode::SortMenu => self.handle_sort_menu_key(key),
            InputMode::StrategyMenu => self.handle_strategy_menu_key(key),
            InputMode::Staking => self.handle_staking_key(key),
        }
    }

    /// Handle keyboard input when QR modal is showing.
    fn handle_qr_modal_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.qr.showing = false;
                self.qr.data = None;
                self.qr.tx_info = None;
                self.qr.frame = 0;
                self.qr.modal_tab = 0;
                self.camera.status = None;
                self.camera.preview = None;
                self.camera.qr_bounds = None;
                self.qr.pending_unsigned = None;
                self.qr.pending_signed = None;
                if self.camera.scanning {
                    return Some(Action::StopSignatureScan);
                }
            }
            KeyCode::Tab | KeyCode::Right => {
                let max_tab = if self.qr.pending_signed.is_some() {
                    4
                } else {
                    3
                };
                self.qr.modal_tab = (self.qr.modal_tab + 1) % max_tab;
                return self.handle_qr_tab_change();
            }
            KeyCode::BackTab | KeyCode::Left => {
                let max_tab = if self.qr.pending_signed.is_some() {
                    3
                } else {
                    2
                };
                self.qr.modal_tab = if self.qr.modal_tab == 0 {
                    max_tab
                } else {
                    self.qr.modal_tab - 1
                };
                return self.handle_qr_tab_change();
            }
            KeyCode::Char('s')
                if self.qr.pending_unsigned.is_some() && self.qr.pending_signed.is_none() =>
            {
                self.qr.modal_tab = 2;
                self.camera.status = Some(CameraScanStatus::Initializing);
                self.camera.frames_captured = 0;
                return Some(Action::StartSignatureScan);
            }
            KeyCode::Char('s') | KeyCode::Enter
                if self.qr.pending_signed.is_some() && self.qr.modal_tab == 3 =>
            {
                if let Some(ref tx) = self.qr.pending_signed
                    && matches!(tx.status, TxSubmissionStatus::ReadyToSubmit)
                {
                    return Some(Action::SubmitTransaction);
                }
            }
            _ => {}
        }
        None
    }

    /// Handle tab changes in QR modal (start/stop camera scanning).
    fn handle_qr_tab_change(&mut self) -> Option<Action> {
        if self.qr.modal_tab == 2 && self.qr.pending_unsigned.is_some() && !self.camera.scanning {
            self.camera.status = Some(CameraScanStatus::Initializing);
            self.camera.frames_captured = 0;
            return Some(Action::StartSignatureScan);
        }
        if self.qr.modal_tab != 2 && self.camera.scanning {
            self.camera.status = None;
            self.camera.preview = None;
            self.camera.qr_bounds = None;
            return Some(Action::StopSignatureScan);
        }
        None
    }

    fn handle_normal_key(&mut self, key: KeyEvent) -> Option<Action> {
        // Handle QR modal
        if self.qr.showing {
            return self.handle_qr_modal_key(key);
        }

        // Handle help overlay
        if self.showing_help {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('?') | KeyCode::Enter) {
                self.showing_help = false;
            }
            return None;
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Tab => return self.next_view(),
            KeyCode::BackTab => return self.prev_view(),
            KeyCode::Char('1') => self.current_view = View::AccountStatus,
            KeyCode::Char('2') => self.current_view = View::AccountChanges,
            KeyCode::Char('3') => {
                self.current_view = View::AccountHistory;
                return self.maybe_auto_load_history();
            }
            KeyCode::Char('4') => self.current_view = View::Nominate,
            KeyCode::Char('5') => self.current_view = View::Validators,
            KeyCode::Char('6') => self.current_view = View::Pools,
            KeyCode::Char('n') => return self.next_network(),
            KeyCode::Char('a')
                if self.current_view == View::AccountStatus || self.show_account_prompt =>
            {
                self.input_mode = InputMode::EnteringAccount;
                self.account_input.clear();
                self.show_account_prompt = false;
            }
            // Account Staking Operations (Now in AccountChanges)
            KeyCode::Char('b')
                if self.current_view == View::AccountChanges && self.watched_account.is_some() =>
            {
                self.input_mode = InputMode::Staking;
                self.staking_input_mode = StakingInputMode::Bond;
                self.staking_input_amount.clear();
            }
            KeyCode::Char('u')
                if self.current_view == View::AccountChanges && self.watched_account.is_some() =>
            {
                self.input_mode = InputMode::Staking;
                self.staking_input_mode = StakingInputMode::Unbond;
                self.staking_input_amount.clear();
            }
            KeyCode::Char('+')
                if self.current_view == View::AccountChanges && self.watched_account.is_some() =>
            {
                self.input_mode = InputMode::Staking;
                self.staking_input_mode = StakingInputMode::BondExtra;
                self.staking_input_amount.clear();
            }
            KeyCode::Char('r')
                if self.current_view == View::AccountChanges && self.watched_account.is_some() =>
            {
                self.input_mode = InputMode::Staking;
                self.staking_input_mode = StakingInputMode::SetPayee;
            }
            KeyCode::Char('w')
                if self.current_view == View::AccountChanges && self.watched_account.is_some() =>
            {
                return Some(Action::GenerateWithdrawUnbondedQR);
            }
            KeyCode::Char('x')
                if self.current_view == View::AccountChanges && self.watched_account.is_some() =>
            {
                return Some(Action::GenerateChillQR);
            }
            // Pool Operations
            KeyCode::Char('j')
                if self.current_view == View::Pools && self.watched_account.is_some() =>
            {
                if let Some(idx) = self.pools_table_state.selected() {
                    self.selected_pool_for_join = Some(idx);
                    self.input_mode = InputMode::Staking;
                    self.staking_input_mode = StakingInputMode::PoolJoin;
                    self.pool_input_amount.clear();
                    return Some(Action::SelectPoolForJoin(idx));
                }
            }
            KeyCode::Char('J')
                if self.current_view == View::Pools && self.watched_account.is_some() =>
            {
                self.input_mode = InputMode::Staking;
                self.staking_input_mode = StakingInputMode::PoolBondExtra;
                self.pool_input_amount.clear();
            }
            KeyCode::Char('U')
                if self.current_view == View::Pools && self.watched_account.is_some() =>
            {
                self.input_mode = InputMode::Staking;
                self.staking_input_mode = StakingInputMode::PoolUnbond;
                self.pool_input_amount.clear();
            }
            KeyCode::Char('C')
                if self.current_view == View::Pools && self.watched_account.is_some() =>
            {
                return Some(Action::GeneratePoolClaimQR);
            }
            KeyCode::Char('W')
                if self.current_view == View::Pools && self.watched_account.is_some() =>
            {
                return Some(Action::GeneratePoolWithdrawQR);
            }
            KeyCode::Char('c') if self.current_view == View::AccountStatus => {
                return Some(Action::ClearAccount);
            }
            // Nominate view keys
            KeyCode::Char('o') if self.current_view == View::Nominate => {
                return Some(Action::RunOptimization);
            }
            KeyCode::Char(' ') if self.current_view == View::Nominate => {
                if let Some(idx) = self.nominate_table_state.selected() {
                    return Some(Action::ToggleValidatorSelection(idx));
                }
            }
            KeyCode::Char('c') if self.current_view == View::Nominate => {
                return Some(Action::ClearNominations);
            }
            KeyCode::Char('g') if self.current_view == View::Nominate => {
                return Some(Action::GenerateNominationQR);
            }
            // History view keys
            KeyCode::Char('l') if self.current_view == View::AccountHistory => {
                if !self.history.loading && self.watched_account.is_some() {
                    return Some(Action::LoadStakingHistory);
                }
            }
            KeyCode::Char('c') if self.current_view == View::AccountHistory => {
                if self.history.loading {
                    return Some(Action::CancelLoadingHistory);
                }
            }
            KeyCode::Char('?') => {
                self.showing_help = true;
            }
            // Search with /
            KeyCode::Char('/') if matches!(self.current_view, View::Validators | View::Pools) => {
                self.input_mode = InputMode::Searching;
                self.search_query.clear();
            }
            // Toggle blocked validators with b
            KeyCode::Char('b') if self.current_view == View::Validators => {
                self.show_blocked = !self.show_blocked;
                self.validators_cache_dirty = true;
            }
            // Sort menu with s
            KeyCode::Char('s') if matches!(self.current_view, View::Validators | View::Pools) => {
                self.input_mode = InputMode::SortMenu;
            }
            // Reverse sort with S
            KeyCode::Char('S') if self.current_view == View::Validators => {
                self.validator_sort_asc = !self.validator_sort_asc;
                self.validators_cache_dirty = true;
            }
            KeyCode::Char('S') if self.current_view == View::Pools => {
                self.pool_sort_asc = !self.pool_sort_asc;
                self.pools_cache_dirty = true;
            }
            // Strategy menu in nominate view
            KeyCode::Char('t') if self.current_view == View::Nominate => {
                self.input_mode = InputMode::StrategyMenu;
            }
            // Account view panel switching
            KeyCode::Left if self.current_view == View::AccountStatus => {
                self.account_panel_focus = 0;
            }
            KeyCode::Right if self.current_view == View::AccountStatus => {
                self.account_panel_focus = 1;
                // Auto-select first row if nothing selected
                if self.address_book_state.selected().is_none() && self.address_book_len() > 0 {
                    self.address_book_state.select(Some(0));
                }
            }
            // Select from address book with Enter when focused on address book
            KeyCode::Enter
                if self.current_view == View::AccountStatus && self.account_panel_focus == 1 =>
            {
                if let Some(idx) = self.address_book_state.selected() {
                    tracing::info!(
                        "[KEY] Enter pressed on address book, selected index: {}",
                        idx
                    );
                    return Some(Action::SelectAddressBookEntry(idx));
                }
                tracing::warn!("[KEY] Enter pressed on address book but nothing selected");
            }
            // Navigation
            KeyCode::Up | KeyCode::Char('k') => self.select_previous(),
            KeyCode::Down | KeyCode::Char('j') => self.select_next(),
            // Log scrolling
            KeyCode::PageUp => {
                self.scroll_logs_up();
            }
            KeyCode::PageDown => {
                self.scroll_logs_down();
            }
            KeyCode::End => {
                self.scroll_logs_to_bottom();
            }
            other => {
                if matches!(other, KeyCode::Enter) {
                    tracing::warn!(
                        "[KEY] Enter fell through to wildcard! view={:?}, panel={}, selected={:?}",
                        self.current_view,
                        self.account_panel_focus,
                        self.address_book_state.selected(),
                    );
                }
            }
        }
        None
    }

    /// Handle keyboard input when searching.
    fn handle_search_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Enter | KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Backspace => {
                self.search_query.pop();
                self.validators_cache_dirty = true;
                self.pools_cache_dirty = true;
            }
            KeyCode::Char(c) => {
                self.search_query.push(c);
                self.validators_cache_dirty = true;
                self.pools_cache_dirty = true;
            }
            _ => {}
        }
        None
    }

    /// Handle keyboard input in sort menu.
    fn handle_sort_menu_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Char(c) => match self.current_view {
                View::Validators => {
                    if let Some(field) = ValidatorSortField::from_key(c) {
                        self.validator_sort = field;
                        self.validators_cache_dirty = true;
                        self.input_mode = InputMode::Normal;
                    }
                }
                View::Pools => {
                    if let Some(field) = PoolSortField::from_key(c) {
                        self.pool_sort = field;
                        self.pools_cache_dirty = true;
                        self.input_mode = InputMode::Normal;
                    }
                }
                _ => {}
            },
            _ => {}
        }
        None
    }

    /// Handle keyboard input in strategy menu.
    fn handle_strategy_menu_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.strategy_index > 0 {
                    self.strategy_index -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.strategy_index < 2 {
                    self.strategy_index += 1;
                }
            }
            KeyCode::Enter => {
                self.input_mode = InputMode::Normal;
                return Some(Action::RunOptimizationWithStrategy(self.strategy_index));
            }
            KeyCode::Char('1') => {
                self.strategy_index = 0;
                self.input_mode = InputMode::Normal;
                return Some(Action::RunOptimizationWithStrategy(0));
            }
            KeyCode::Char('2') => {
                self.strategy_index = 1;
                self.input_mode = InputMode::Normal;
                return Some(Action::RunOptimizationWithStrategy(1));
            }
            KeyCode::Char('3') => {
                self.strategy_index = 2;
                self.input_mode = InputMode::Normal;
                return Some(Action::RunOptimizationWithStrategy(2));
            }
            _ => {}
        }
        None
    }

    /// Handle keyboard input during staking operations.
    fn handle_staking_key(&mut self, key: KeyEvent) -> Option<Action> {
        match self.staking_input_mode {
            StakingInputMode::None => {
                self.input_mode = InputMode::Normal;
                None
            }
            StakingInputMode::Bond
            | StakingInputMode::Unbond
            | StakingInputMode::BondExtra
            | StakingInputMode::PoolJoin
            | StakingInputMode::PoolUnbond
            | StakingInputMode::PoolBondExtra => match key.code {
                KeyCode::Esc => {
                    self.input_mode = InputMode::Normal;
                    self.staking_input_mode = StakingInputMode::None;
                    self.staking_input_amount.clear();
                    self.pool_input_amount.clear();
                    None
                }
                KeyCode::Enter => self.confirm_staking_operation(),
                KeyCode::Backspace => {
                    if matches!(
                        self.staking_input_mode,
                        StakingInputMode::PoolJoin
                            | StakingInputMode::PoolUnbond
                            | StakingInputMode::PoolBondExtra
                    ) {
                        self.pool_input_amount.pop();
                    } else {
                        self.staking_input_amount.pop();
                    }
                    None
                }
                KeyCode::Char(c) => {
                    if c.is_ascii_digit() || c == '.' {
                        if matches!(
                            self.staking_input_mode,
                            StakingInputMode::PoolJoin
                                | StakingInputMode::PoolUnbond
                                | StakingInputMode::PoolBondExtra
                        ) {
                            self.pool_input_amount.push(c);
                        } else {
                            self.staking_input_amount.push(c);
                        }
                    }
                    None
                }
                _ => None,
            },
            StakingInputMode::SetPayee => match key.code {
                KeyCode::Esc => {
                    self.input_mode = InputMode::Normal;
                    self.staking_input_mode = StakingInputMode::None;
                    None
                }
                KeyCode::Char('r') | KeyCode::Right | KeyCode::Left => {
                    let next = match self.rewards_destination {
                        RewardDestination::Staked => RewardDestination::Stash,
                        RewardDestination::Stash => RewardDestination::Controller,
                        RewardDestination::Controller => RewardDestination::Account(String::new()),
                        RewardDestination::Account(_) => RewardDestination::None,
                        RewardDestination::None => RewardDestination::Staked,
                    };
                    Some(Action::SetRewardsDestination(next))
                }
                KeyCode::Enter => {
                    self.input_mode = InputMode::Normal;
                    self.staking_input_mode = StakingInputMode::None;
                    Some(Action::GenerateSetPayeeQR {
                        destination: self.rewards_destination.clone(),
                    })
                }
                _ => None,
            },
        }
    }

    /// Confirm staking operation and generate QR.
    fn confirm_staking_operation(&mut self) -> Option<Action> {
        let (amount_str, is_pool) = match self.staking_input_mode {
            StakingInputMode::PoolJoin
            | StakingInputMode::PoolUnbond
            | StakingInputMode::PoolBondExtra => (&self.pool_input_amount, true),
            _ => (&self.staking_input_amount, false),
        };

        let amount = self.parse_amount(amount_str)?;

        // Reset input mode
        self.input_mode = InputMode::Normal;
        let mode = self.staking_input_mode;
        self.staking_input_mode = StakingInputMode::None;
        if is_pool {
            self.pool_input_amount.clear();
        } else {
            self.staking_input_amount.clear();
        }

        match mode {
            StakingInputMode::Bond => Some(Action::GenerateBondQR { value: amount }),
            StakingInputMode::Unbond => Some(Action::GenerateUnbondQR { value: amount }),
            StakingInputMode::BondExtra => Some(Action::GenerateBondExtraQR { value: amount }),
            StakingInputMode::PoolJoin => {
                if let Some(idx) = self.selected_pool_for_join
                    && let Some(pool) = self.pools.get(idx)
                {
                    return Some(Action::GeneratePoolJoinQR {
                        pool_id: pool.id,
                        amount,
                    });
                }
                None
            }
            StakingInputMode::PoolBondExtra => Some(Action::GeneratePoolBondExtraQR { amount }),
            StakingInputMode::PoolUnbond => Some(Action::GeneratePoolUnbondQR { amount }),
            _ => None,
        }
    }

    /// Parse amount string to u128 (planck) based on network decimals.
    fn parse_amount(&self, input: &str) -> Option<u128> {
        stkopt_core::parse_token_amount(input, self.network.token_decimals()).ok()
    }

    /// Handle keyboard input when entering text.
    fn handle_input_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Enter => {
                self.input_mode = InputMode::Normal;
                match self.validate_account_input() {
                    Ok(account) => {
                        self.validation_error = None;
                        let original_input = self.account_input.trim().to_string();
                        return Some(Action::SetWatchedAccount(account, original_input));
                    }
                    Err(error_msg) => {
                        self.validation_error = Some(error_msg);
                        // Keep input mode to let user correct the address
                        self.input_mode = InputMode::EnteringAccount;
                        return None;
                    }
                }
            }
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.account_input.clear();
                self.validation_error = None;
            }
            KeyCode::Backspace => {
                self.account_input.pop();
                // Re-validate on backspace
                if let Err(msg) = self.validate_account_input() {
                    self.validation_error = Some(msg);
                } else {
                    self.validation_error = None;
                }
            }
            KeyCode::Char(c) => {
                self.account_input.push(c);
                // Validate as user types
                if let Err(msg) = self.validate_account_input() {
                    self.validation_error = Some(msg);
                } else {
                    self.validation_error = None;
                }
            }
            _ => {}
        }
        None
    }

    /// Validate the account input and return a helpful error message if invalid.
    fn validate_account_input(&self) -> Result<AccountId32, String> {
        let input = self.account_input.trim();

        if input.is_empty() {
            return Err("Please enter an address".to_string());
        }

        if input.len() < 3 {
            return Err("Address is too short (minimum 3 characters)".to_string());
        }

        <AccountId32 as std::str::FromStr>::from_str(input).map_err(|e| e.to_string())
    }

    /// Switch to next view.
    fn next_view(&mut self) -> Option<Action> {
        let views = View::all();
        let current_idx = self.current_view.index();
        let next_idx = (current_idx + 1) % views.len();
        self.current_view = View::from_index(next_idx);
        if self.current_view == View::AccountHistory {
            return self.maybe_auto_load_history();
        }
        None
    }

    /// Switch to previous view.
    fn prev_view(&mut self) -> Option<Action> {
        let views = View::all();
        let current_idx = self.current_view.index();
        let prev_idx = if current_idx == 0 {
            views.len() - 1
        } else {
            current_idx - 1
        };
        self.current_view = View::from_index(prev_idx);
        if self.current_view == View::AccountHistory {
            return self.maybe_auto_load_history();
        }
        None
    }

    /// Auto-load staking history when switching to History view.
    fn maybe_auto_load_history(&mut self) -> Option<Action> {
        // Only auto-load if:
        // - We have a watched account
        // - Not already loading
        // - Either no history loaded, or loaded for different account
        if let Some(account) = &self.watched_account {
            let account_str = account.to_string();
            let should_load = !self.history.loading
                && (self.history.points.is_empty()
                    || self.history.loaded_for.as_ref() != Some(&account_str));
            if should_load {
                self.history.points.clear();
                self.history.loading = true;
                self.history.loaded_for = Some(account_str);
                return Some(Action::LoadStakingHistory);
            }
        }
        None
    }

    /// Switch to next network.
    fn next_network(&mut self) -> Option<Action> {
        let networks = Network::all();
        let current_idx = networks
            .iter()
            .position(|n| *n == self.network)
            .unwrap_or(0);
        let next_idx = (current_idx + 1) % networks.len();
        Some(Action::SwitchNetwork(networks[next_idx]))
    }

    /// Get tick count for animations.
    pub fn tick_count(&self) -> u64 {
        self.tick_count
    }

    /// Handle an action from the chain client or other sources.
    pub fn handle_action(&mut self, action: Action) {
        match action {
            Action::UpdateConnectionStatus(status) => {
                self.connection_status = status;
            }
            Action::SetChainInfo(info) => {
                self.chain_info = Some(info);
                self.loading.chain = false;
            }
            Action::SetActiveEra(era_info) => {
                self.current_era = Some(era_info.index);
                self.era_pct_complete = era_info.pct_complete;
            }
            Action::SetEraDuration(duration) => {
                self.era_duration_ms = duration;
            }

            Action::SetDisplayValidators(validators) => {
                self.validators = validators;
                self.loading.validators = false;
                self.validators_cache_dirty = true;
                // Select first row if we have validators
                if !self.validators.is_empty() && self.validators_table_state.selected().is_none() {
                    self.validators_table_state.select(Some(0));
                }
            }
            Action::SetDisplayPools(pools) => {
                self.pools = pools;
                self.pools_cache_dirty = true;
                // Select first row if we have pools
                if !self.pools.is_empty() && self.pools_table_state.selected().is_none() {
                    self.pools_table_state.select(Some(0));
                }
            }
            Action::SetLoadingProgress(progress, bytes_loaded, _estimated_total) => {
                self.loading.progress = progress;
                // Track actual bytes transferred
                if let Some(bytes) = bytes_loaded {
                    self.loading.bytes_loaded = bytes;
                    // Compute real bandwidth from actual bytes
                    if let Some(start) = self.loading.start_time {
                        let elapsed = start.elapsed().as_secs_f64();
                        if elapsed > 0.5 {
                            self.loading.bandwidth =
                                Some(self.loading.bytes_loaded as f64 / elapsed);
                        }
                    }
                }
            }
            Action::SetWatchedAccount(account, _original) => {
                self.watched_account = Some(account);
                self.account_status = None; // Will be fetched
                self.account_panel_focus = 0; // Move focus back to account status
            }
            Action::SetAccountStatus(status) => {
                tracing::debug!("SetAccountStatus action received, updating app state");
                self.account_status = Some(*status);
            }
            Action::ClearAccount => {
                self.watched_account = None;
                self.account_status = None;
                self.account_input.clear();
            }
            Action::RunOptimization | Action::RunOptimizationWithStrategy(_) => {
                // Handled in main.rs
            }
            Action::SetOptimizationResult(result) => {
                // Select the optimized validators
                self.selected_validators.clear();
                for candidate in &result.selected {
                    // Find matching validator by address
                    if let Some(idx) = self
                        .validators
                        .iter()
                        .position(|v| v.address == candidate.address)
                    {
                        self.selected_validators.insert(idx);
                    }
                }
                if let Some(first_selected) = self.selected_validators.iter().min().copied() {
                    self.nominate_table_state.select(Some(first_selected));
                }
                self.nomination_status = if result.selected.is_empty() {
                    Some(
                        "No eligible validators found for the current optimizer filters."
                            .to_string(),
                    )
                } else {
                    Some(format!(
                        "Selected {} validators. Press g to generate the signing QR.",
                        result.selected.len()
                    ))
                };
                self.optimization_result = Some(result);
            }
            Action::SetNominationStatus(status) => {
                self.nomination_status = status;
            }
            Action::ToggleValidatorSelection(idx) => {
                if self.selected_validators.contains(&idx) {
                    self.selected_validators.remove(&idx);
                } else if self.selected_validators.len() < 16 {
                    // Max 16 nominations
                    self.selected_validators.insert(idx);
                }
                // Clear optimization result since we're now manually selecting
                self.optimization_result = None;
                self.nomination_status = Some(format!(
                    "{} validators selected. Press g to generate the signing QR.",
                    self.selected_validators.len()
                ));
            }
            Action::ClearNominations => {
                self.selected_validators.clear();
                self.optimization_result = None;
                self.nomination_status = Some("Cleared nomination selection.".to_string());
            }
            Action::GenerateNominationQR => {
                // Handled in main.rs
            }
            Action::SetQRData(data, tx_info) => {
                self.qr.data = data;
                self.qr.tx_info = tx_info;
                self.qr.frame = 0; // Reset animation frame for new QR
                self.qr.modal_tab = 0; // Reset to QR tab
                self.qr.showing = self.qr.data.is_some();
                if self.qr.showing {
                    self.nomination_status =
                        Some("Signing QR ready. Scan it with Polkadot Vault.".to_string());
                }
            }
            Action::SetPendingUnsignedTx(pending) => {
                self.qr.pending_unsigned = pending;
            }
            Action::StartSignatureScan => {
                self.camera.scanning = true;
            }
            Action::StopSignatureScan => {
                self.camera.scanning = false;
            }
            Action::SignatureScanned(_) => {
                // Handled in main.rs - processes the signature and creates pending_tx
            }
            Action::QrScanFailed(ref error) => {
                tracing::error!("QR scan failed: {}", error);
                self.camera.scanning = false;
                self.camera.status = Some(CameraScanStatus::Error);
                self.nomination_status = Some(error.clone());
            }
            Action::UpdateScanStatus(status) => {
                // Increment frame counter on each scan update (activity indicator)
                if matches!(status, QrScanStatus::Scanning | QrScanStatus::Detected) {
                    self.camera.frames_captured = self.camera.frames_captured.saturating_add(1);
                }
                self.camera.status = Some(match status {
                    QrScanStatus::Scanning => CameraScanStatus::Scanning,
                    QrScanStatus::Detected => CameraScanStatus::Detected,
                    QrScanStatus::Success => CameraScanStatus::Success,
                });
            }
            Action::UpdateCameraPreview(pixels, width, height, bounds) => {
                self.camera.preview = Some(pixels);
                self.camera.preview_size = (width, height);
                self.camera.qr_bounds = bounds;
            }
            Action::SubmitTransaction => {
                // Handled in main.rs - submits the pending_tx
            }
            Action::SetTxStatus(status) => {
                if let Some(ref mut tx) = self.qr.pending_signed {
                    tx.status = status;
                }
            }
            Action::ClearPendingTx => {
                self.qr.pending_signed = None;
                self.qr.pending_unsigned = None;
                self.camera.scanning = false;
            }
            Action::SetStakingHistory(history) => {
                self.history.points = history;
                self.history.loading = false;
            }
            Action::AddStakingHistoryPoint(point) => {
                // Check if this era already exists (avoid duplicates)
                if let Some(existing_pos) =
                    self.history.points.iter().position(|p| p.era == point.era)
                {
                    // Replace existing entry with new data
                    self.history.points[existing_pos] = point;
                } else {
                    // Insert in era order (oldest first)
                    let pos = self
                        .history
                        .points
                        .iter()
                        .position(|p| p.era > point.era)
                        .unwrap_or(self.history.points.len());
                    self.history.points.insert(pos, point);
                }
            }
            Action::SetHistoryTotalEras(total) => {
                self.history.total_eras = total.max(1);
            }
            Action::LoadStakingHistory => {
                // Manual load request (clear if needed and start loading)
                if !self.history.loading {
                    self.history.points.clear();
                    self.history.loading = true;
                    if let Some(account) = &self.watched_account {
                        self.history.loaded_for = Some(account.to_string());
                    }
                }
                // Actual loading is handled in main.rs
            }
            Action::CancelLoadingHistory => {
                self.history.loading = false;
            }
            Action::HistoryLoadingComplete => {
                self.history.loading = false;
            }
            Action::SwitchNetwork(network) => {
                self.network = network;
                self.connection_status = ConnectionStatus::Disconnected;
                self.current_era = None;
                self.era_pct_complete = 0.0;
                self.validators.clear();
                self.validators_cache_dirty = true;
                self.selected_validators.clear();
                self.optimization_result = None;
                self.account_status = None;
                self.history.points.clear();
                self.pools.clear();
                self.pools_cache_dirty = true;
            }
            Action::SelectAddressBookEntry(_idx) => {
                // Handled in main.rs where we have access to the address book entries
            }
            Action::RemoveAccount(_addr) => {
                // Handled in main.rs where we have access to config and database
            }

            Action::SetRewardsDestination(dest) => {
                self.rewards_destination = dest;
            }
            Action::SelectPoolForJoin(idx) => {
                self.selected_pool_for_join = Some(idx);
            }
            Action::GenerateBondQR { .. }
            | Action::GenerateUnbondQR { .. }
            | Action::GenerateBondExtraQR { .. }
            | Action::GenerateSetPayeeQR { .. }
            | Action::GenerateWithdrawUnbondedQR
            | Action::GenerateChillQR
            | Action::GeneratePoolJoinQR { .. }
            | Action::GeneratePoolBondExtraQR { .. }
            | Action::GeneratePoolClaimQR
            | Action::GeneratePoolUnbondQR { .. }
            | Action::GeneratePoolWithdrawQR => {
                // Handled in main.rs
            }
        }
    }

    /// Get the number of entries in the address book.
    pub fn address_book_len(&self) -> usize {
        let my_account = if self.watched_account.is_some() { 1 } else { 0 };
        my_account + KNOWN_ADDRESSES.len()
    }

    /// Move selection up in the current list.
    pub fn select_previous(&mut self) {
        match self.current_view {
            View::Validators => {
                let sel = cycle_selection(
                    self.validators_table_state.selected(),
                    self.validators.len(),
                    Direction::Previous,
                );
                self.validators_table_state.select(sel);
            }
            View::Pools => {
                let sel = cycle_selection(
                    self.pools_table_state.selected(),
                    self.pools.len(),
                    Direction::Previous,
                );
                self.pools_table_state.select(sel);
            }
            View::Nominate => {
                let sel = cycle_selection(
                    self.nominate_table_state.selected(),
                    self.validators.len(),
                    Direction::Previous,
                );
                self.nominate_table_state.select(sel);
            }
            View::AccountStatus if self.account_panel_focus == 1 => {
                let sel = cycle_selection(
                    self.address_book_state.selected(),
                    self.address_book_len(),
                    Direction::Previous,
                );
                self.address_book_state.select(sel);
            }
            _ => {}
        }
    }

    /// Get filtered and sorted validators.
    pub fn filtered_validators(&self) -> Vec<&DisplayValidator> {
        let mut result: Vec<_> = self
            .validators
            .iter()
            .filter(|v| {
                // Filter by blocked status
                if !self.show_blocked && v.blocked {
                    return false;
                }
                // Filter by search query
                if !self.search_query.is_empty() {
                    let query = self.search_query.to_lowercase();
                    let name_match = v
                        .name
                        .as_ref()
                        .map(|n| n.to_lowercase().contains(&query))
                        .unwrap_or(false);
                    let addr_match = v.address.to_lowercase().contains(&query);
                    if !name_match && !addr_match {
                        return false;
                    }
                }
                true
            })
            .collect();

        // Sort
        result.sort_by(|a, b| {
            let cmp = match self.validator_sort {
                ValidatorSortField::Name => {
                    let a_name = a.name.as_deref().unwrap_or("");
                    let b_name = b.name.as_deref().unwrap_or("");
                    a_name.cmp(b_name)
                }
                ValidatorSortField::Address => a.address.cmp(&b.address),
                ValidatorSortField::Commission => a
                    .commission
                    .partial_cmp(&b.commission)
                    .unwrap_or(std::cmp::Ordering::Equal),
                ValidatorSortField::TotalStake => a.total_stake.cmp(&b.total_stake),
                ValidatorSortField::OwnStake => a.own_stake.cmp(&b.own_stake),
                ValidatorSortField::Points => a.points.cmp(&b.points),
                ValidatorSortField::Nominators => a.nominator_count.cmp(&b.nominator_count),
                ValidatorSortField::Apy => a
                    .apy
                    .partial_cmp(&b.apy)
                    .unwrap_or(std::cmp::Ordering::Equal),
                ValidatorSortField::Blocked => a.blocked.cmp(&b.blocked),
            };
            if self.validator_sort_asc {
                cmp
            } else {
                cmp.reverse()
            }
        });

        result
    }

    /// Get filtered and sorted pools.
    pub fn filtered_pools(&self) -> Vec<&DisplayPool> {
        let mut result: Vec<_> = self
            .pools
            .iter()
            .filter(|p| {
                // Filter by search query
                if !self.search_query.is_empty() {
                    let query = self.search_query.to_lowercase();
                    let name_match = p.name.to_lowercase().contains(&query);
                    let id_match = p.id.to_string().contains(&query);
                    if !name_match && !id_match {
                        return false;
                    }
                }
                true
            })
            .collect();

        // Sort
        result.sort_by(|a, b| {
            let cmp = match self.pool_sort {
                PoolSortField::Id => a.id.cmp(&b.id),
                PoolSortField::Name => a.name.cmp(&b.name),
                PoolSortField::State => format!("{:?}", a.state).cmp(&format!("{:?}", b.state)),
                PoolSortField::Members => a.member_count.cmp(&b.member_count),
                PoolSortField::Points => a.total_bonded.cmp(&b.total_bonded),
                PoolSortField::Apy => {
                    let a_apy = a.apy.unwrap_or(0.0);
                    let b_apy = b.apy.unwrap_or(0.0);
                    a_apy
                        .partial_cmp(&b_apy)
                        .unwrap_or(std::cmp::Ordering::Equal)
                }
            };
            if self.pool_sort_asc {
                cmp
            } else {
                cmp.reverse()
            }
        });

        result
    }

    /// Get cached filtered and sorted validators, recomputing only when inputs changed.
    pub fn filtered_validators_cached(&mut self) -> Arc<Vec<DisplayValidator>> {
        if self.validators_cache_dirty {
            self.cached_filtered_validators =
                Arc::new(self.filtered_validators().into_iter().cloned().collect());
            self.validators_cache_dirty = false;
        }
        self.cached_filtered_validators.clone()
    }

    /// Get cached filtered and sorted pools, recomputing only when inputs changed.
    pub fn filtered_pools_cached(&mut self) -> Arc<Vec<DisplayPool>> {
        if self.pools_cache_dirty {
            self.cached_filtered_pools =
                Arc::new(self.filtered_pools().into_iter().cloned().collect());
            self.pools_cache_dirty = false;
        }
        self.cached_filtered_pools.clone()
    }

    /// Move selection down in the current list.
    pub fn select_next(&mut self) {
        match self.current_view {
            View::Validators => {
                let sel = cycle_selection(
                    self.validators_table_state.selected(),
                    self.validators.len(),
                    Direction::Next,
                );
                self.validators_table_state.select(sel);
            }
            View::Pools => {
                let sel = cycle_selection(
                    self.pools_table_state.selected(),
                    self.pools.len(),
                    Direction::Next,
                );
                self.pools_table_state.select(sel);
            }
            View::Nominate => {
                let sel = cycle_selection(
                    self.nominate_table_state.selected(),
                    self.validators.len(),
                    Direction::Next,
                );
                self.nominate_table_state.select(sel);
            }
            View::AccountStatus if self.account_panel_focus == 1 => {
                let sel = cycle_selection(
                    self.address_book_state.selected(),
                    self.address_book_len(),
                    Direction::Next,
                );
                self.address_book_state.select(sel);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log_buffer::{LogLevel, LogLine};
    use crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};
    use stkopt_chain::{AccountBalance, UnsignedPayload};
    use stkopt_core::optimizer::ValidatorCandidate;
    use stkopt_core::types::PoolState;

    fn create_app() -> App {
        App::new(Network::Polkadot, LogBuffer::new(), Theme::Dark)
    }

    // === validate_account_input ===

    #[test]
    fn test_validate_account_input_kusama_accepted() {
        let mut app = create_app();
        app.account_input = "HNZata7iMYWmk5RvZRTiAsSDhV8366zq2YGb3tLH5Upf74F".to_string();
        assert!(app.validate_account_input().is_ok());
    }

    #[test]
    fn test_validate_account_input_paseo_accepted() {
        let mut app = create_app();
        app.account_input = "15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5".to_string();
        assert!(app.validate_account_input().is_ok());
    }

    #[test]
    fn test_validate_account_input_westend_accepted() {
        let mut app = create_app();
        app.account_input = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY".to_string();
        assert!(app.validate_account_input().is_ok());
    }

    #[test]
    fn test_validate_account_input_invalid_chars_rejected() {
        let mut app = create_app();
        app.account_input = "!not-a-valid@address#".to_string();
        assert!(app.validate_account_input().is_err());
    }

    #[test]
    fn test_validate_account_input_too_short_rejected() {
        let mut app = create_app();
        app.account_input = "ab".to_string();
        let result = app.validate_account_input();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("too short"));
    }

    fn make_validator(
        address: &str,
        name: Option<&str>,
        commission: f64,
        blocked: bool,
        apy: Option<f64>,
    ) -> DisplayValidator {
        DisplayValidator::new(
            address.to_string(),
            name.map(|s| s.to_string()),
            commission,
            blocked,
            1_000_000,
            100_000,
            10,
            100,
            apy,
        )
    }

    fn make_pool(id: u32, name: &str, state: PoolState, apy: Option<f64>) -> DisplayPool {
        DisplayPool::new(id, name.to_string(), state, 10, 1_000_000, None, apy)
    }

    // === ValidatorSortField ===

    #[test]
    fn test_validator_sort_field_all() {
        let all = ValidatorSortField::all();
        assert_eq!(all.len(), 9);
        assert!(all.contains(&ValidatorSortField::Apy));
    }

    #[test]
    fn test_validator_sort_field_label() {
        assert_eq!(ValidatorSortField::Name.label(), "Name");
        assert_eq!(ValidatorSortField::Apy.label(), "APY");
        assert_eq!(ValidatorSortField::Blocked.label(), "Blocked");
    }

    #[test]
    fn test_validator_sort_field_key() {
        assert_eq!(ValidatorSortField::Name.key(), 'n');
        assert_eq!(ValidatorSortField::Apy.key(), 'y');
    }

    #[test]
    fn test_validator_sort_field_from_key_valid() {
        assert_eq!(
            ValidatorSortField::from_key('n'),
            Some(ValidatorSortField::Name)
        );
        assert_eq!(
            ValidatorSortField::from_key('y'),
            Some(ValidatorSortField::Apy)
        );
    }

    #[test]
    fn test_validator_sort_field_from_key_invalid() {
        assert_eq!(ValidatorSortField::from_key('z'), None);
    }

    // === PoolSortField ===

    #[test]
    fn test_pool_sort_field_all() {
        let all = PoolSortField::all();
        assert_eq!(all.len(), 6);
        assert!(all.contains(&PoolSortField::Apy));
    }

    #[test]
    fn test_pool_sort_field_label() {
        assert_eq!(PoolSortField::Id.label(), "ID");
        assert_eq!(PoolSortField::Apy.label(), "APY");
    }

    #[test]
    fn test_pool_sort_field_key() {
        assert_eq!(PoolSortField::Id.key(), 'i');
        assert_eq!(PoolSortField::Apy.key(), 'y');
    }

    #[test]
    fn test_pool_sort_field_from_key_valid() {
        assert_eq!(PoolSortField::from_key('i'), Some(PoolSortField::Id));
        assert_eq!(PoolSortField::from_key('y'), Some(PoolSortField::Apy));
    }

    #[test]
    fn test_pool_sort_field_from_key_invalid() {
        assert_eq!(PoolSortField::from_key('z'), None);
    }

    // === View ===

    #[test]
    fn test_view_all() {
        let all = View::all();
        assert_eq!(all.len(), 6);
        assert!(all.contains(&View::AccountStatus));
    }

    #[test]
    fn test_view_label() {
        assert_eq!(View::AccountStatus.label(), "Account Status");
        assert_eq!(View::Validators.label(), "Validators");
    }

    #[test]
    fn test_view_index() {
        assert_eq!(View::AccountStatus.index(), 0);
        assert_eq!(View::Pools.index(), 5);
    }

    #[test]
    fn test_view_from_index() {
        assert_eq!(View::from_index(0), View::AccountStatus);
        assert_eq!(View::from_index(5), View::Pools);
    }

    #[test]
    fn test_view_from_index_out_of_bounds() {
        assert_eq!(View::from_index(99), View::AccountStatus);
    }

    // === cycle_selection ===

    #[test]
    fn test_cycle_selection_next() {
        assert_eq!(cycle_selection(Some(0), 3, Direction::Next), Some(1));
        assert_eq!(cycle_selection(Some(1), 3, Direction::Next), Some(2));
    }

    #[test]
    fn test_cycle_selection_previous() {
        assert_eq!(cycle_selection(Some(1), 3, Direction::Previous), Some(0));
        assert_eq!(cycle_selection(Some(2), 3, Direction::Previous), Some(1));
    }

    #[test]
    fn test_cycle_selection_wrap_next() {
        assert_eq!(cycle_selection(Some(2), 3, Direction::Next), Some(0));
    }

    #[test]
    fn test_cycle_selection_wrap_previous() {
        assert_eq!(cycle_selection(Some(0), 3, Direction::Previous), Some(2));
    }

    #[test]
    fn test_cycle_selection_empty() {
        assert_eq!(cycle_selection(Some(0), 0, Direction::Next), None);
    }

    #[test]
    fn test_cycle_selection_none_start_next() {
        assert_eq!(cycle_selection(None, 3, Direction::Next), Some(0));
    }

    #[test]
    fn test_cycle_selection_none_start_previous() {
        assert_eq!(cycle_selection(None, 3, Direction::Previous), Some(0));
    }

    // === App::new ===

    #[test]
    fn test_app_new_defaults() {
        let app = create_app();
        assert_eq!(app.network, Network::Polkadot);
        assert_eq!(app.current_view, View::AccountStatus);
        assert_eq!(app.tick_count(), 0);
        assert!(!app.should_quit);
        assert!(app.validators.is_empty());
        assert!(app.pools.is_empty());
        assert_eq!(app.address_book_len(), KNOWN_ADDRESSES.len());
    }

    // === App::tick ===

    #[test]
    fn test_app_tick_increments() {
        let mut app = create_app();
        app.tick();
        assert_eq!(app.tick_count(), 1);
    }

    #[test]
    fn test_app_tick_qr_animation() {
        let mut app = create_app();
        app.qr.showing = true;
        app.qr.frame = 0;
        app.tick();
        assert_eq!(app.qr.frame, 1);
    }

    #[test]
    fn test_app_tick_no_qr_when_hidden() {
        let mut app = create_app();
        app.qr.showing = false;
        app.qr.frame = 0;
        app.tick();
        assert_eq!(app.qr.frame, 0);
    }

    #[test]
    fn test_app_tick_spinner() {
        let mut app = create_app();
        app.loading.validators = true;
        let before = app.loading.spinner_tick;
        app.tick();
        assert_eq!(app.loading.spinner_tick, before + 1);
    }

    #[test]
    fn test_app_tick_no_spinner_when_idle() {
        let mut app = create_app();
        app.loading.chain = false;
        app.loading.validators = false;
        let before = app.loading.spinner_tick;
        app.tick();
        assert_eq!(app.loading.spinner_tick, before);
    }

    // === App::spinner_char ===

    #[test]
    fn test_spinner_char() {
        let mut app = create_app();
        app.loading.spinner_tick = 0;
        assert_eq!(app.spinner_char(), '⠋');
        app.loading.spinner_tick = 9;
        assert_eq!(app.spinner_char(), '⠏');
        app.loading.spinner_tick = 10;
        assert_eq!(app.spinner_char(), '⠋');
    }

    // === App::estimated_remaining_secs ===

    #[test]
    fn test_estimated_remaining_secs() {
        let mut app = create_app();
        app.loading.progress = 0.5;
        app.loading.bandwidth = Some(1_000_000.0);
        // progress_rate = 1_000_000 / 10_000_000 = 0.1
        // remaining = 0.5
        // secs = 0.5 / 0.1 = 5.0
        assert_eq!(app.estimated_remaining_secs(), Some(5.0));
    }

    #[test]
    fn test_estimated_remaining_secs_no_bandwidth() {
        let app = create_app();
        assert_eq!(app.estimated_remaining_secs(), None);
    }

    #[test]
    fn test_estimated_remaining_secs_zero_bandwidth() {
        let mut app = create_app();
        app.loading.bandwidth = Some(0.0);
        assert_eq!(app.estimated_remaining_secs(), None);
    }

    #[test]
    fn test_estimated_remaining_secs_negative_bandwidth() {
        let mut app = create_app();
        app.loading.bandwidth = Some(-1.0);
        assert_eq!(app.estimated_remaining_secs(), None);
    }

    // === App::format_eta ===

    #[test]
    fn test_format_eta_seconds() {
        let mut app = create_app();
        app.loading.progress = 0.9;
        app.loading.bandwidth = Some(1_000_000.0);
        // remaining = 0.1, rate = 0.1, secs = 1.0
        assert_eq!(app.format_eta(), Some("~1s".to_string()));
    }

    #[test]
    fn test_format_eta_minutes() {
        let mut app = create_app();
        app.loading.progress = 0.0;
        app.loading.bandwidth = Some(100_000.0);
        // remaining = 1.0, rate = 0.01, secs = 100.0
        assert_eq!(app.format_eta(), Some("~2m 40s".to_string()));
    }

    #[test]
    fn test_format_eta_hours() {
        let mut app = create_app();
        app.loading.progress = 0.0;
        app.loading.bandwidth = Some(1_000.0);
        // remaining = 1.0, rate = 0.0001, secs = 10000.0 (~2.78h)
        assert_eq!(app.format_eta(), Some("~3h".to_string()));
    }

    #[test]
    fn test_format_eta_none() {
        let app = create_app();
        assert_eq!(app.format_eta(), None);
    }

    // === App::parse_amount ===

    #[test]
    fn test_parse_amount_whole() {
        let app = create_app();
        assert_eq!(app.parse_amount("1"), Some(10_000_000_000u128));
    }

    #[test]
    fn test_parse_amount_with_decimals() {
        let app = create_app();
        // 1.5 DOT = 15_000_000_000 planck (10 decimals)
        assert_eq!(app.parse_amount("1.5"), Some(15_000_000_000u128));
    }

    #[test]
    fn test_parse_amount_fractional() {
        let app = create_app();
        // 0.1 DOT = 1_000_000_000 planck
        assert_eq!(app.parse_amount("0.1"), Some(1_000_000_000u128));
    }

    #[test]
    fn test_parse_amount_empty() {
        let app = create_app();
        assert_eq!(app.parse_amount(""), None);
    }

    #[test]
    fn test_parse_amount_invalid() {
        let app = create_app();
        assert_eq!(app.parse_amount("abc"), None);
    }

    #[test]
    fn test_parse_amount_too_many_dots() {
        let app = create_app();
        assert_eq!(app.parse_amount("1.2.3"), None);
    }

    #[test]
    fn test_parse_amount_rejects_too_many_decimals() {
        let app = create_app();
        // 11 decimals provided, but Polkadot has 10 -> reject
        assert_eq!(app.parse_amount("1.12345678901"), None);
    }

    #[test]
    fn test_parse_amount_pad_decimals() {
        let app = create_app();
        // 2 decimals provided, pad with 8 zeros
        assert_eq!(app.parse_amount("1.12"), Some(11_200_000_000u128));
    }

    #[test]
    fn test_parse_amount_zero() {
        let app = create_app();
        assert_eq!(app.parse_amount("0"), None);
    }

    #[test]
    fn test_parse_amount_kusama_decimals() {
        let mut app = create_app();
        app.network = Network::Kusama; // 12 decimals
        assert_eq!(app.parse_amount("1"), Some(1_000_000_000_000u128));
        assert_eq!(app.parse_amount("0.001"), Some(1_000_000_000u128));
    }

    // === App::address_book_len ===

    #[test]
    fn test_address_book_len_no_account() {
        let app = create_app();
        assert_eq!(app.address_book_len(), KNOWN_ADDRESSES.len());
    }

    #[test]
    fn test_address_book_len_with_account() {
        let mut app = create_app();
        app.watched_account = Some(AccountId32::from([0u8; 32]));
        assert_eq!(app.address_book_len(), KNOWN_ADDRESSES.len() + 1);
    }

    // === App::filtered_validators ===

    #[test]
    fn test_filtered_validators_empty() {
        let app = create_app();
        assert!(app.filtered_validators().is_empty());
    }

    #[test]
    fn test_filtered_validators_blocked_filter() {
        let mut app = create_app();
        app.show_blocked = false;
        app.validators = vec![
            make_validator("addr1", None, 0.1, false, Some(0.15)),
            make_validator("addr2", None, 0.1, true, Some(0.20)),
        ];
        let filtered = app.filtered_validators();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].address, "addr1");
    }

    #[test]
    fn test_filtered_validators_show_blocked() {
        let mut app = create_app();
        app.show_blocked = true;
        app.validators = vec![
            make_validator("addr1", None, 0.1, false, Some(0.15)),
            make_validator("addr2", None, 0.1, true, Some(0.20)),
        ];
        let filtered = app.filtered_validators();
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filtered_validators_search_by_name() {
        let mut app = create_app();
        app.search_query = "alice".to_string();
        app.validators = vec![
            make_validator("addr1", Some("AliceValidator"), 0.1, false, None),
            make_validator("addr2", Some("BobValidator"), 0.1, false, None),
        ];
        let filtered = app.filtered_validators();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].address, "addr1");
    }

    #[test]
    fn test_filtered_validators_search_by_address() {
        let mut app = create_app();
        app.search_query = "addr2".to_string();
        app.validators = vec![
            make_validator("addr1", None, 0.1, false, None),
            make_validator("addr2", None, 0.1, false, None),
        ];
        let filtered = app.filtered_validators();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].address, "addr2");
    }

    #[test]
    fn test_filtered_validators_search_no_match() {
        let mut app = create_app();
        app.search_query = "zzz".to_string();
        app.validators = vec![make_validator("addr1", Some("Alice"), 0.1, false, None)];
        let filtered = app.filtered_validators();
        assert!(filtered.is_empty());
    }

    #[test]
    fn test_filtered_validators_sort_apy_desc() {
        let mut app = create_app();
        app.validator_sort = ValidatorSortField::Apy;
        app.validator_sort_asc = false;
        app.validators = vec![
            make_validator("low", None, 0.1, false, Some(0.10)),
            make_validator("high", None, 0.1, false, Some(0.20)),
        ];
        let filtered = app.filtered_validators();
        assert_eq!(filtered[0].address, "high");
        assert_eq!(filtered[1].address, "low");
    }

    #[test]
    fn test_filtered_validators_sort_apy_asc() {
        let mut app = create_app();
        app.validator_sort = ValidatorSortField::Apy;
        app.validator_sort_asc = true;
        app.validators = vec![
            make_validator("low", None, 0.1, false, Some(0.10)),
            make_validator("high", None, 0.1, false, Some(0.20)),
        ];
        let filtered = app.filtered_validators();
        assert_eq!(filtered[0].address, "low");
        assert_eq!(filtered[1].address, "high");
    }

    #[test]
    fn test_filtered_validators_sort_name() {
        let mut app = create_app();
        app.validator_sort = ValidatorSortField::Name;
        app.validator_sort_asc = true;
        app.validators = vec![
            make_validator("addr1", Some("Bob"), 0.1, false, None),
            make_validator("addr2", Some("Alice"), 0.1, false, None),
        ];
        let filtered = app.filtered_validators();
        assert_eq!(filtered[0].address, "addr2");
        assert_eq!(filtered[1].address, "addr1");
    }

    // === App::filtered_pools ===

    #[test]
    fn test_filtered_pools_empty() {
        let app = create_app();
        assert!(app.filtered_pools().is_empty());
    }

    #[test]
    fn test_filtered_pools_search_by_name() {
        let mut app = create_app();
        app.search_query = "alpha".to_string();
        app.pools = vec![
            make_pool(1, "Alpha Pool", PoolState::Open, None),
            make_pool(2, "Beta Pool", PoolState::Open, None),
        ];
        let filtered = app.filtered_pools();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, 1);
    }

    #[test]
    fn test_filtered_pools_search_by_id() {
        let mut app = create_app();
        app.search_query = "2".to_string();
        app.pools = vec![
            make_pool(1, "Alpha Pool", PoolState::Open, None),
            make_pool(2, "Beta Pool", PoolState::Open, None),
        ];
        let filtered = app.filtered_pools();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, 2);
    }

    #[test]
    fn test_filtered_pools_search_no_match() {
        let mut app = create_app();
        app.search_query = "zzz".to_string();
        app.pools = vec![make_pool(1, "Alpha", PoolState::Open, None)];
        let filtered = app.filtered_pools();
        assert!(filtered.is_empty());
    }

    #[test]
    fn test_filtered_pools_sort_apy_desc() {
        let mut app = create_app();
        app.pool_sort = PoolSortField::Apy;
        app.pool_sort_asc = false;
        app.pools = vec![
            make_pool(1, "low", PoolState::Open, Some(0.10)),
            make_pool(2, "high", PoolState::Open, Some(0.20)),
        ];
        let filtered = app.filtered_pools();
        assert_eq!(filtered[0].id, 2);
        assert_eq!(filtered[1].id, 1);
    }

    #[test]
    fn test_filtered_pools_sort_apy_asc() {
        let mut app = create_app();
        app.pool_sort = PoolSortField::Apy;
        app.pool_sort_asc = true;
        app.pools = vec![
            make_pool(1, "low", PoolState::Open, Some(0.10)),
            make_pool(2, "high", PoolState::Open, Some(0.20)),
        ];
        let filtered = app.filtered_pools();
        assert_eq!(filtered[0].id, 1);
        assert_eq!(filtered[1].id, 2);
    }

    #[test]
    fn test_filtered_pools_sort_id() {
        let mut app = create_app();
        app.pool_sort = PoolSortField::Id;
        app.pool_sort_asc = true;
        app.pools = vec![
            make_pool(2, "b", PoolState::Open, None),
            make_pool(1, "a", PoolState::Open, None),
        ];
        let filtered = app.filtered_pools();
        assert_eq!(filtered[0].id, 1);
        assert_eq!(filtered[1].id, 2);
    }

    // === cached filter lists ===

    #[test]
    fn test_filtered_validators_cached_reuses_result() {
        let mut app = create_app();
        app.validators = vec![make_validator(
            "addr1",
            Some("Alice"),
            0.1,
            false,
            Some(0.15),
        )];
        app.validators_cache_dirty = true;
        let first = app.filtered_validators_cached();
        let second = app.filtered_validators_cached();
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn test_filtered_validators_cached_updates_on_search_change() {
        let mut app = create_app();
        app.validators = vec![
            make_validator("addr1", Some("Alice"), 0.1, false, Some(0.15)),
            make_validator("addr2", Some("Bob"), 0.1, false, Some(0.20)),
        ];
        app.search_query = "Alice".to_string();
        app.validators_cache_dirty = true;
        let first = app.filtered_validators_cached();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].address, "addr1");

        app.search_query = "Bob".to_string();
        app.validators_cache_dirty = true;
        let second = app.filtered_validators_cached();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].address, "addr2");
    }

    #[test]
    fn test_filtered_pools_cached_reuses_result() {
        let mut app = create_app();
        app.pools = vec![make_pool(1, "Alpha", PoolState::Open, None)];
        app.pools_cache_dirty = true;
        let first = app.filtered_pools_cached();
        let second = app.filtered_pools_cached();
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn test_filtered_pools_cached_updates_on_search_change() {
        let mut app = create_app();
        app.pools = vec![
            make_pool(1, "Alpha", PoolState::Open, None),
            make_pool(2, "Beta", PoolState::Open, None),
        ];
        app.search_query = "Alpha".to_string();
        app.pools_cache_dirty = true;
        let first = app.filtered_pools_cached();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].id, 1);

        app.search_query = "Beta".to_string();
        app.pools_cache_dirty = true;
        let second = app.filtered_pools_cached();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].id, 2);
    }

    // === scroll methods ===

    #[test]
    fn test_scroll_logs_up() {
        let mut app = create_app();
        for i in 0..10 {
            app.log_buffer.push(LogLine {
                level: LogLevel::Info,
                target: "test".to_string(),
                message: format!("msg {}", i),
            });
        }
        app.scroll_logs_up();
        assert_eq!(app.log_scroll, 1);
        app.scroll_logs_up();
        assert_eq!(app.log_scroll, 2);
    }

    #[test]
    fn test_scroll_logs_down() {
        let mut app = create_app();
        for i in 0..10 {
            app.log_buffer.push(LogLine {
                level: LogLevel::Info,
                target: "test".to_string(),
                message: format!("msg {}", i),
            });
        }
        app.scroll_logs_up();
        app.scroll_logs_up();
        assert_eq!(app.log_scroll, 2);
        app.scroll_logs_down();
        assert_eq!(app.log_scroll, 1);
    }

    #[test]
    fn test_scroll_logs_to_bottom() {
        let mut app = create_app();
        for i in 0..10 {
            app.log_buffer.push(LogLine {
                level: LogLevel::Info,
                target: "test".to_string(),
                message: format!("msg {}", i),
            });
        }
        app.scroll_logs_up();
        app.scroll_logs_to_bottom();
        assert_eq!(app.log_scroll, 0);
    }

    #[test]
    fn test_scroll_logs_up_few_entries() {
        let mut app = create_app();
        app.log_buffer.push(LogLine {
            level: LogLevel::Info,
            target: "test".to_string(),
            message: "msg".to_string(),
        });
        app.scroll_logs_up();
        // log_count = 1, not > 3, so no change
        assert_eq!(app.log_scroll, 0);
    }

    #[test]
    fn test_scroll_logs_down_at_bottom() {
        let mut app = create_app();
        app.scroll_logs_down();
        assert_eq!(app.log_scroll, 0);
    }

    // === next_view / prev_view ===

    #[test]
    fn test_next_view() {
        let mut app = create_app();
        app.next_view();
        assert_eq!(app.current_view, View::AccountChanges);
    }

    #[test]
    fn test_prev_view() {
        let mut app = create_app();
        app.prev_view();
        assert_eq!(app.current_view, View::Pools);
    }

    #[test]
    fn test_next_view_wraps() {
        let mut app = create_app();
        for _ in 0..6 {
            app.next_view();
        }
        assert_eq!(app.current_view, View::AccountStatus);
    }

    #[test]
    fn test_prev_view_wraps() {
        let mut app = create_app();
        for _ in 0..6 {
            app.prev_view();
        }
        assert_eq!(app.current_view, View::AccountStatus);
    }

    // === next_network ===

    #[test]
    fn test_next_network() {
        let mut app = create_app();
        let action = app.next_network();
        assert!(matches!(
            action,
            Some(Action::SwitchNetwork(Network::Kusama))
        ));
    }

    #[test]
    fn test_next_network_wraps() {
        let mut app = create_app();
        app.network = Network::Paseo;
        let action = app.next_network();
        assert!(matches!(
            action,
            Some(Action::SwitchNetwork(Network::Polkadot))
        ));
    }

    // === select_next / select_previous ===

    #[test]
    fn test_select_next_validators() {
        let mut app = create_app();
        app.current_view = View::Validators;
        app.validators = vec![
            make_validator("v1", None, 0.1, false, None),
            make_validator("v2", None, 0.1, false, None),
        ];
        app.select_next();
        assert_eq!(app.validators_table_state.selected(), Some(0));
        app.select_next();
        assert_eq!(app.validators_table_state.selected(), Some(1));
    }

    #[test]
    fn test_select_previous_validators() {
        let mut app = create_app();
        app.current_view = View::Validators;
        app.validators = vec![
            make_validator("v1", None, 0.1, false, None),
            make_validator("v2", None, 0.1, false, None),
        ];
        app.validators_table_state.select(Some(1));
        app.select_previous();
        assert_eq!(app.validators_table_state.selected(), Some(0));
    }

    #[test]
    fn test_select_next_validators_wraps() {
        let mut app = create_app();
        app.current_view = View::Validators;
        app.validators = vec![
            make_validator("v1", None, 0.1, false, None),
            make_validator("v2", None, 0.1, false, None),
        ];
        app.validators_table_state.select(Some(1));
        app.select_next();
        assert_eq!(app.validators_table_state.selected(), Some(0));
    }

    #[test]
    fn test_select_previous_validators_wraps() {
        let mut app = create_app();
        app.current_view = View::Validators;
        app.validators = vec![
            make_validator("v1", None, 0.1, false, None),
            make_validator("v2", None, 0.1, false, None),
        ];
        app.validators_table_state.select(Some(0));
        app.select_previous();
        assert_eq!(app.validators_table_state.selected(), Some(1));
    }

    #[test]
    fn test_select_next_pools() {
        let mut app = create_app();
        app.current_view = View::Pools;
        app.pools = vec![
            make_pool(1, "p1", PoolState::Open, None),
            make_pool(2, "p2", PoolState::Open, None),
        ];
        app.select_next();
        assert_eq!(app.pools_table_state.selected(), Some(0));
        app.select_next();
        assert_eq!(app.pools_table_state.selected(), Some(1));
    }

    #[test]
    fn test_select_previous_pools() {
        let mut app = create_app();
        app.current_view = View::Pools;
        app.pools = vec![
            make_pool(1, "p1", PoolState::Open, None),
            make_pool(2, "p2", PoolState::Open, None),
        ];
        app.pools_table_state.select(Some(1));
        app.select_previous();
        assert_eq!(app.pools_table_state.selected(), Some(0));
    }

    #[test]
    fn test_select_next_nominate() {
        let mut app = create_app();
        app.current_view = View::Nominate;
        app.validators = vec![
            make_validator("v1", None, 0.1, false, None),
            make_validator("v2", None, 0.1, false, None),
        ];
        app.select_next();
        assert_eq!(app.nominate_table_state.selected(), Some(0));
        app.select_next();
        assert_eq!(app.nominate_table_state.selected(), Some(1));
    }

    #[test]
    fn test_select_next_address_book() {
        let mut app = create_app();
        app.current_view = View::AccountStatus;
        app.account_panel_focus = 1;
        app.select_next();
        assert_eq!(app.address_book_state.selected(), Some(0));
    }

    #[test]
    fn test_select_previous_address_book() {
        let mut app = create_app();
        app.current_view = View::AccountStatus;
        app.account_panel_focus = 1;
        app.address_book_state.select(Some(1));
        app.select_previous();
        assert_eq!(app.address_book_state.selected(), Some(0));
    }

    #[test]
    fn test_select_next_empty_list() {
        let mut app = create_app();
        app.current_view = View::Validators;
        app.validators = vec![];
        app.select_next();
        assert_eq!(app.validators_table_state.selected(), None);
    }

    // === handle_action ===

    #[test]
    fn test_handle_action_set_display_validators() {
        let mut app = create_app();
        app.loading.validators = true;
        let validators = vec![make_validator(
            "addr1",
            Some("Alice"),
            0.1,
            false,
            Some(0.15),
        )];
        app.handle_action(Action::SetDisplayValidators(validators));
        assert_eq!(app.validators.len(), 1);
        assert!(!app.loading.validators);
        assert_eq!(app.validators_table_state.selected(), Some(0));
    }

    #[test]
    fn test_handle_action_set_display_validators_empty() {
        let mut app = create_app();
        app.validators_table_state.select(Some(0));
        app.handle_action(Action::SetDisplayValidators(vec![]));
        assert_eq!(app.validators_table_state.selected(), Some(0));
    }

    #[test]
    fn test_handle_action_set_display_pools() {
        let mut app = create_app();
        let pools = vec![make_pool(1, "Alpha", PoolState::Open, Some(0.12))];
        app.handle_action(Action::SetDisplayPools(pools));
        assert_eq!(app.pools.len(), 1);
        assert_eq!(app.pools_table_state.selected(), Some(0));
    }

    #[test]
    fn test_handle_action_set_watched_account() {
        let mut app = create_app();
        app.account_panel_focus = 1;
        let account = AccountId32::from([1u8; 32]);
        app.handle_action(Action::SetWatchedAccount(
            account.clone(),
            "addr".to_string(),
        ));
        assert_eq!(app.watched_account, Some(account));
        assert!(app.account_status.is_none());
        assert_eq!(app.account_panel_focus, 0);
    }

    #[test]
    fn test_handle_action_clear_account() {
        let mut app = create_app();
        app.watched_account = Some(AccountId32::from([1u8; 32]));
        app.account_status = Some(AccountStatus {
            address: AccountId32::from([1u8; 32]),
            balance: AccountBalance {
                free: 0,
                reserved: 0,
                frozen: 0,
            },
            staking_ledger: None,
            nominations: None,
            pool_membership: None,
        });
        app.handle_action(Action::ClearAccount);
        assert!(app.watched_account.is_none());
        assert!(app.account_status.is_none());
        assert!(app.account_input.is_empty());
    }

    #[test]
    fn test_handle_action_set_optimization_result() {
        let mut app = create_app();
        app.validators = vec![
            make_validator("addr1", Some("Alice"), 0.1, false, Some(0.15)),
            make_validator("addr2", Some("Bob"), 0.1, false, Some(0.20)),
        ];
        let result = OptimizationResult {
            selected: vec![ValidatorCandidate {
                address: "addr1".to_string(),
                commission: 0.1,
                blocked: false,
                apy: 0.15,
                total_stake: 1_000_000,
                nominator_count: 10,
            }],
            estimated_apy_min: 0.15,
            estimated_apy_max: 0.15,
            estimated_apy_avg: 0.15,
            total_stake: 1_000_000,
            avg_commission: 0.1,
        };
        app.handle_action(Action::SetOptimizationResult(result));
        assert!(app.selected_validators.contains(&0));
        assert_eq!(app.nominate_table_state.selected(), Some(0));
        assert!(app.optimization_result.is_some());
        assert!(
            app.nomination_status
                .as_ref()
                .is_some_and(|s| s.contains("Selected 1 validators"))
        );
    }

    #[test]
    fn test_handle_action_set_optimization_result_empty() {
        let mut app = create_app();
        let result = OptimizationResult {
            selected: vec![],
            estimated_apy_min: 0.0,
            estimated_apy_max: 0.0,
            estimated_apy_avg: 0.0,
            total_stake: 0,
            avg_commission: 0.0,
        };
        app.handle_action(Action::SetOptimizationResult(result));
        assert!(app.selected_validators.is_empty());
        assert_eq!(
            app.nomination_status,
            Some("No eligible validators found for the current optimizer filters.".to_string())
        );
    }

    #[test]
    fn test_handle_action_toggle_validator_selection_add() {
        let mut app = create_app();
        app.validators = vec![make_validator("addr1", None, 0.1, false, None)];
        app.optimization_result = Some(OptimizationResult {
            selected: vec![],
            estimated_apy_min: 0.0,
            estimated_apy_max: 0.0,
            estimated_apy_avg: 0.0,
            total_stake: 0,
            avg_commission: 0.0,
        });
        app.handle_action(Action::ToggleValidatorSelection(0));
        assert!(app.selected_validators.contains(&0));
        assert!(app.optimization_result.is_none());
    }

    #[test]
    fn test_handle_action_toggle_validator_selection_remove() {
        let mut app = create_app();
        app.selected_validators.insert(0);
        app.handle_action(Action::ToggleValidatorSelection(0));
        assert!(!app.selected_validators.contains(&0));
    }

    #[test]
    fn test_handle_action_toggle_validator_selection_max() {
        let mut app = create_app();
        app.validators = (0..17)
            .map(|i| make_validator(&format!("addr{}", i), None, 0.1, false, None))
            .collect();
        for i in 0..16 {
            app.selected_validators.insert(i);
        }
        app.handle_action(Action::ToggleValidatorSelection(16));
        assert!(!app.selected_validators.contains(&16));
        assert_eq!(app.selected_validators.len(), 16);
    }

    #[test]
    fn test_handle_action_clear_nominations() {
        let mut app = create_app();
        app.selected_validators.insert(0);
        app.optimization_result = Some(OptimizationResult {
            selected: vec![],
            estimated_apy_min: 0.0,
            estimated_apy_max: 0.0,
            estimated_apy_avg: 0.0,
            total_stake: 0,
            avg_commission: 0.0,
        });
        app.handle_action(Action::ClearNominations);
        assert!(app.selected_validators.is_empty());
        assert!(app.optimization_result.is_none());
    }

    #[test]
    fn test_handle_action_set_qr_data_some() {
        let mut app = create_app();
        app.qr.showing = false;
        app.qr.frame = 5;
        app.qr.modal_tab = 2;
        app.handle_action(Action::SetQRData(Some(vec![1, 2, 3]), None));
        assert!(app.qr.showing);
        assert_eq!(app.qr.frame, 0);
        assert_eq!(app.qr.modal_tab, 0);
        assert_eq!(app.qr.data, Some(vec![1, 2, 3]));
    }

    #[test]
    fn test_handle_action_set_qr_data_none() {
        let mut app = create_app();
        app.qr.showing = true;
        app.handle_action(Action::SetQRData(None, None));
        assert!(!app.qr.showing);
    }

    #[test]
    fn test_handle_action_set_pending_unsigned_tx() {
        let mut app = create_app();
        let pending = PendingUnsignedTx {
            payload: UnsignedPayload {
                call_data: vec![],
                description: "test".to_string(),
                metadata_hash: [0u8; 32],
                genesis_hash: [0u8; 32],
                block_hash: [0u8; 32],
                spec_version: 0,
                tx_version: 0,
                nonce: 0,
                era: stkopt_chain::Era::Immortal,
                include_metadata_hash: false,
                use_asset_payment: false,
                extension_ids: vec![],
            },
            signer: AccountId32::from([0u8; 32]),
        };
        app.handle_action(Action::SetPendingUnsignedTx(Some(pending)));
        assert!(app.qr.pending_unsigned.is_some());
    }

    #[test]
    fn test_handle_action_start_stop_signature_scan() {
        let mut app = create_app();
        app.handle_action(Action::StartSignatureScan);
        assert!(app.camera.scanning);
        app.handle_action(Action::StopSignatureScan);
        assert!(!app.camera.scanning);
    }

    #[test]
    fn test_handle_action_qr_scan_failed() {
        let mut app = create_app();
        app.camera.scanning = true;
        app.handle_action(Action::QrScanFailed("oops".to_string()));
        assert!(!app.camera.scanning);
        assert_eq!(app.camera.status, Some(CameraScanStatus::Error));
        assert_eq!(app.nomination_status, Some("oops".to_string()));
    }

    #[test]
    fn test_handle_action_update_scan_status_scanning() {
        let mut app = create_app();
        app.camera.frames_captured = 0;
        app.handle_action(Action::UpdateScanStatus(QrScanStatus::Scanning));
        assert_eq!(app.camera.frames_captured, 1);
        assert_eq!(app.camera.status, Some(CameraScanStatus::Scanning));
    }

    #[test]
    fn test_handle_action_update_scan_status_success() {
        let mut app = create_app();
        app.camera.frames_captured = 5;
        app.handle_action(Action::UpdateScanStatus(QrScanStatus::Success));
        assert_eq!(app.camera.frames_captured, 5);
        assert_eq!(app.camera.status, Some(CameraScanStatus::Success));
    }

    #[test]
    fn test_handle_action_update_camera_preview() {
        let mut app = create_app();
        app.handle_action(Action::UpdateCameraPreview(vec![1, 2], 100, 200, None));
        assert_eq!(app.camera.preview, Some(vec![1, 2]));
        assert_eq!(app.camera.preview_size, (100, 200));
    }

    #[test]
    fn test_handle_action_set_tx_status() {
        let mut app = create_app();
        app.qr.pending_signed = Some(PendingTransaction {
            signed_extrinsic: vec![],
            tx_hash: [0u8; 32],
            status: TxSubmissionStatus::ReadyToSubmit,
        });
        app.handle_action(Action::SetTxStatus(TxSubmissionStatus::Submitting));
        assert!(
            app.qr
                .pending_signed
                .as_ref()
                .is_some_and(|tx| matches!(tx.status, TxSubmissionStatus::Submitting))
        );
    }

    #[test]
    fn test_handle_action_clear_pending_tx() {
        let mut app = create_app();
        app.qr.pending_signed = Some(PendingTransaction {
            signed_extrinsic: vec![],
            tx_hash: [0u8; 32],
            status: TxSubmissionStatus::ReadyToSubmit,
        });
        app.qr.pending_unsigned = Some(PendingUnsignedTx {
            payload: UnsignedPayload {
                call_data: vec![],
                description: "test".to_string(),
                metadata_hash: [0u8; 32],
                genesis_hash: [0u8; 32],
                block_hash: [0u8; 32],
                spec_version: 0,
                tx_version: 0,
                nonce: 0,
                era: stkopt_chain::Era::Immortal,
                include_metadata_hash: false,
                use_asset_payment: false,
                extension_ids: vec![],
            },
            signer: AccountId32::from([0u8; 32]),
        });
        app.camera.scanning = true;
        app.handle_action(Action::ClearPendingTx);
        assert!(app.qr.pending_signed.is_none());
        assert!(app.qr.pending_unsigned.is_none());
        assert!(!app.camera.scanning);
    }

    #[test]
    fn test_handle_action_set_staking_history() {
        let mut app = create_app();
        app.history.loading = true;
        let points = vec![StakingHistoryPoint::new_without_date(1, 100, 1000, 0.15)];
        app.handle_action(Action::SetStakingHistory(points));
        assert_eq!(app.history.points.len(), 1);
        assert!(!app.history.loading);
    }

    #[test]
    fn test_handle_action_add_staking_history_point_new() {
        let mut app = create_app();
        let p1 = StakingHistoryPoint::new_without_date(5, 100, 1000, 0.15);
        app.handle_action(Action::AddStakingHistoryPoint(p1));
        assert_eq!(app.history.points.len(), 1);
        assert_eq!(app.history.points[0].era, 5);
    }

    #[test]
    fn test_handle_action_add_staking_history_point_duplicate() {
        let mut app = create_app();
        let p1 = StakingHistoryPoint::new_without_date(5, 100, 1000, 0.15);
        let p2 = StakingHistoryPoint::new_without_date(5, 200, 2000, 0.20);
        app.handle_action(Action::AddStakingHistoryPoint(p1));
        app.handle_action(Action::AddStakingHistoryPoint(p2));
        assert_eq!(app.history.points.len(), 1);
        assert_eq!(app.history.points[0].reward, 200);
    }

    #[test]
    fn test_handle_action_add_staking_history_point_sorted() {
        let mut app = create_app();
        let p1 = StakingHistoryPoint::new_without_date(10, 100, 1000, 0.15);
        let p2 = StakingHistoryPoint::new_without_date(5, 200, 2000, 0.20);
        app.handle_action(Action::AddStakingHistoryPoint(p1));
        app.handle_action(Action::AddStakingHistoryPoint(p2));
        assert_eq!(app.history.points.len(), 2);
        assert_eq!(app.history.points[0].era, 5);
        assert_eq!(app.history.points[1].era, 10);
    }

    #[test]
    fn test_handle_action_load_staking_history() {
        let mut app = create_app();
        app.watched_account = Some(AccountId32::from([1u8; 32]));
        app.history.points = vec![StakingHistoryPoint::new_without_date(1, 100, 1000, 0.15)];
        app.history.loading = false;
        app.handle_action(Action::LoadStakingHistory);
        assert!(app.history.loading);
        assert!(app.history.points.is_empty());
        assert_eq!(
            app.history.loaded_for,
            Some(AccountId32::from([1u8; 32]).to_string())
        );
    }

    #[test]
    fn test_handle_action_cancel_loading_history() {
        let mut app = create_app();
        app.history.loading = true;
        app.handle_action(Action::CancelLoadingHistory);
        assert!(!app.history.loading);
    }

    #[test]
    fn test_handle_action_history_loading_complete() {
        let mut app = create_app();
        app.history.loading = true;
        app.handle_action(Action::HistoryLoadingComplete);
        assert!(!app.history.loading);
    }

    #[test]
    fn test_handle_action_switch_network() {
        let mut app = create_app();
        app.validators = vec![make_validator("addr1", None, 0.1, false, None)];
        app.selected_validators.insert(0);
        app.optimization_result = Some(OptimizationResult {
            selected: vec![],
            estimated_apy_min: 0.0,
            estimated_apy_max: 0.0,
            estimated_apy_avg: 0.0,
            total_stake: 0,
            avg_commission: 0.0,
        });
        app.account_status = Some(AccountStatus {
            address: AccountId32::from([1u8; 32]),
            balance: stkopt_chain::AccountBalance {
                free: 0,
                reserved: 0,
                frozen: 0,
            },
            staking_ledger: None,
            nominations: None,
            pool_membership: None,
        });
        app.history
            .points
            .push(stkopt_core::StakingHistoryPoint::new_without_date(
                1, 1, 1, 0.1,
            ));
        app.pools.push(stkopt_core::DisplayPool::new(
            1,
            "Pool".to_string(),
            stkopt_core::PoolState::Open,
            0,
            0,
            None,
            None,
        ));
        app.connection_status = ConnectionStatus::Connected;
        app.current_era = Some(100);
        app.handle_action(Action::SwitchNetwork(Network::Kusama));
        assert_eq!(app.network, Network::Kusama);
        assert_eq!(app.connection_status, ConnectionStatus::Disconnected);
        assert!(app.current_era.is_none());
        assert_eq!(app.era_pct_complete, 0.0);
        assert!(app.validators.is_empty());
        assert!(app.selected_validators.is_empty());
        assert!(app.optimization_result.is_none());
        assert!(app.account_status.is_none());
        assert!(app.history.points.is_empty());
        assert!(app.pools.is_empty());
    }

    #[test]
    fn test_handle_action_set_rewards_destination() {
        let mut app = create_app();
        app.handle_action(Action::SetRewardsDestination(RewardDestination::Stash));
        assert!(matches!(app.rewards_destination, RewardDestination::Stash));
    }

    #[test]
    fn test_handle_action_select_pool_for_join() {
        let mut app = create_app();
        app.handle_action(Action::SelectPoolForJoin(3));
        assert_eq!(app.selected_pool_for_join, Some(3));
    }

    #[test]
    fn test_handle_action_update_connection_status() {
        let mut app = create_app();
        app.handle_action(Action::UpdateConnectionStatus(ConnectionStatus::Connected));
        assert_eq!(app.connection_status, ConnectionStatus::Connected);
    }

    #[test]
    fn test_handle_action_set_chain_info() {
        let mut app = create_app();
        app.loading.chain = true;
        let info = ChainInfo {
            chain_name: "Polkadot".to_string(),
            spec_name: "polkadot".to_string(),
            spec_version: 1000,
            tx_version: 20,
        };
        app.handle_action(Action::SetChainInfo(info));
        assert!(app.chain_info.is_some());
        assert!(!app.loading.chain);
    }

    #[test]
    fn test_handle_action_set_loading_progress_with_bytes() {
        let mut app = create_app();
        app.loading.start_time =
            Some(std::time::Instant::now() - std::time::Duration::from_secs(1));
        app.handle_action(Action::SetLoadingProgress(
            0.5,
            Some(1_000_000),
            Some(2_000_000),
        ));
        assert_eq!(app.loading.progress, 0.5);
        assert_eq!(app.loading.bytes_loaded, 1_000_000);
        assert!(app.loading.bandwidth.is_some());
    }

    #[test]
    fn test_handle_action_set_loading_progress_no_bytes() {
        let mut app = create_app();
        app.handle_action(Action::SetLoadingProgress(0.5, None, None));
        assert_eq!(app.loading.progress, 0.5);
        assert_eq!(app.loading.bytes_loaded, 0);
        assert!(app.loading.bandwidth.is_none());
    }

    // === handle_staking_key ===

    #[test]
    fn test_handle_staking_key_none_mode() {
        let mut app = create_app();
        app.input_mode = InputMode::Staking;
        app.staking_input_mode = StakingInputMode::None;
        let action = app.handle_staking_key(KeyEvent::from(KeyCode::Char('x')));
        assert!(action.is_none());
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn test_handle_staking_key_bond_digit_input() {
        let mut app = create_app();
        app.input_mode = InputMode::Staking;
        app.staking_input_mode = StakingInputMode::Bond;
        let action = app.handle_staking_key(KeyEvent::from(KeyCode::Char('1')));
        assert!(action.is_none());
        assert_eq!(app.staking_input_amount, "1");
    }

    #[test]
    fn test_handle_staking_key_bond_decimal_input() {
        let mut app = create_app();
        app.input_mode = InputMode::Staking;
        app.staking_input_mode = StakingInputMode::Bond;
        let action = app.handle_staking_key(KeyEvent::from(KeyCode::Char('.')));
        assert!(action.is_none());
        assert_eq!(app.staking_input_amount, ".");
    }

    #[test]
    fn test_handle_staking_key_bond_backspace() {
        let mut app = create_app();
        app.input_mode = InputMode::Staking;
        app.staking_input_mode = StakingInputMode::Bond;
        app.staking_input_amount = "12".to_string();
        let action = app.handle_staking_key(KeyEvent::from(KeyCode::Backspace));
        assert!(action.is_none());
        assert_eq!(app.staking_input_amount, "1");
    }

    #[test]
    fn test_handle_staking_key_bond_escape() {
        let mut app = create_app();
        app.input_mode = InputMode::Staking;
        app.staking_input_mode = StakingInputMode::Bond;
        app.staking_input_amount = "10".to_string();
        let action = app.handle_staking_key(KeyEvent::from(KeyCode::Esc));
        assert!(action.is_none());
        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(app.staking_input_mode, StakingInputMode::None);
        assert!(app.staking_input_amount.is_empty());
    }

    #[test]
    fn test_handle_staking_key_bond_enter_valid() {
        let mut app = create_app();
        app.input_mode = InputMode::Staking;
        app.staking_input_mode = StakingInputMode::Bond;
        app.staking_input_amount = "1".to_string();
        let action = app.handle_staking_key(KeyEvent::from(KeyCode::Enter));
        assert!(matches!(
            action,
            Some(Action::GenerateBondQR {
                value: 10_000_000_000
            })
        ));
        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(app.staking_input_mode, StakingInputMode::None);
    }

    #[test]
    fn test_handle_staking_key_bond_enter_invalid() {
        let mut app = create_app();
        app.input_mode = InputMode::Staking;
        app.staking_input_mode = StakingInputMode::Bond;
        app.staking_input_amount = "abc".to_string();
        let action = app.handle_staking_key(KeyEvent::from(KeyCode::Enter));
        assert!(action.is_none());
        // Input mode stays Staking so user can correct the amount
        assert_eq!(app.input_mode, InputMode::Staking);
    }

    #[test]
    fn test_handle_staking_key_pool_join_digit_input() {
        let mut app = create_app();
        app.input_mode = InputMode::Staking;
        app.staking_input_mode = StakingInputMode::PoolJoin;
        let action = app.handle_staking_key(KeyEvent::from(KeyCode::Char('5')));
        assert!(action.is_none());
        assert_eq!(app.pool_input_amount, "5");
    }

    #[test]
    fn test_handle_staking_key_pool_join_backspace() {
        let mut app = create_app();
        app.input_mode = InputMode::Staking;
        app.staking_input_mode = StakingInputMode::PoolJoin;
        app.pool_input_amount = "50".to_string();
        let action = app.handle_staking_key(KeyEvent::from(KeyCode::Backspace));
        assert!(action.is_none());
        assert_eq!(app.pool_input_amount, "5");
    }

    #[test]
    fn test_handle_staking_key_pool_join_enter_with_pool() {
        let mut app = create_app();
        app.input_mode = InputMode::Staking;
        app.staking_input_mode = StakingInputMode::PoolJoin;
        app.pool_input_amount = "1".to_string();
        app.selected_pool_for_join = Some(0);
        app.pools = vec![make_pool(42, "Alpha", PoolState::Open, None)];
        let action = app.handle_staking_key(KeyEvent::from(KeyCode::Enter));
        assert!(matches!(
            action,
            Some(Action::GeneratePoolJoinQR {
                pool_id: 42,
                amount: 10_000_000_000
            })
        ));
    }

    #[test]
    fn test_handle_staking_key_pool_join_enter_no_pool() {
        let mut app = create_app();
        app.input_mode = InputMode::Staking;
        app.staking_input_mode = StakingInputMode::PoolJoin;
        app.pool_input_amount = "1".to_string();
        app.selected_pool_for_join = None;
        let action = app.handle_staking_key(KeyEvent::from(KeyCode::Enter));
        assert!(action.is_none());
    }

    #[test]
    fn test_handle_staking_key_set_payee_escape() {
        let mut app = create_app();
        app.input_mode = InputMode::Staking;
        app.staking_input_mode = StakingInputMode::SetPayee;
        let action = app.handle_staking_key(KeyEvent::from(KeyCode::Esc));
        assert!(action.is_none());
        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(app.staking_input_mode, StakingInputMode::None);
    }

    #[test]
    fn test_handle_staking_key_set_payee_cycle() {
        let mut app = create_app();
        app.input_mode = InputMode::Staking;
        app.staking_input_mode = StakingInputMode::SetPayee;
        app.rewards_destination = RewardDestination::Staked;
        let action = app.handle_staking_key(KeyEvent::from(KeyCode::Char('r')));
        assert!(matches!(
            action,
            Some(Action::SetRewardsDestination(RewardDestination::Stash))
        ));
    }

    #[test]
    fn test_handle_staking_key_set_payee_enter() {
        let mut app = create_app();
        app.input_mode = InputMode::Staking;
        app.staking_input_mode = StakingInputMode::SetPayee;
        app.rewards_destination = RewardDestination::Staked;
        let action = app.handle_staking_key(KeyEvent::from(KeyCode::Enter));
        assert!(matches!(
            action,
            Some(Action::GenerateSetPayeeQR {
                destination: RewardDestination::Staked
            })
        ));
        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(app.staking_input_mode, StakingInputMode::None);
    }

    // === confirm_staking_operation ===

    #[test]
    fn test_confirm_staking_operation_bond() {
        let mut app = create_app();
        app.staking_input_mode = StakingInputMode::Bond;
        app.staking_input_amount = "1".to_string();
        let action = app.confirm_staking_operation();
        assert!(matches!(
            action,
            Some(Action::GenerateBondQR {
                value: 10_000_000_000
            })
        ));
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(app.staking_input_amount.is_empty());
    }

    #[test]
    fn test_confirm_staking_operation_unbond() {
        let mut app = create_app();
        app.staking_input_mode = StakingInputMode::Unbond;
        app.staking_input_amount = "2".to_string();
        let action = app.confirm_staking_operation();
        assert!(matches!(
            action,
            Some(Action::GenerateUnbondQR {
                value: 20_000_000_000
            })
        ));
    }

    #[test]
    fn test_confirm_staking_operation_bond_extra() {
        let mut app = create_app();
        app.staking_input_mode = StakingInputMode::BondExtra;
        app.staking_input_amount = "0.5".to_string();
        let action = app.confirm_staking_operation();
        assert!(matches!(
            action,
            Some(Action::GenerateBondExtraQR {
                value: 5_000_000_000
            })
        ));
    }

    #[test]
    fn test_confirm_staking_operation_pool_join() {
        let mut app = create_app();
        app.staking_input_mode = StakingInputMode::PoolJoin;
        app.pool_input_amount = "1".to_string();
        app.selected_pool_for_join = Some(0);
        app.pools = vec![make_pool(7, "Alpha", PoolState::Open, None)];
        let action = app.confirm_staking_operation();
        assert!(matches!(
            action,
            Some(Action::GeneratePoolJoinQR {
                pool_id: 7,
                amount: 10_000_000_000
            })
        ));
    }

    #[test]
    fn test_confirm_staking_operation_pool_join_no_pool() {
        let mut app = create_app();
        app.staking_input_mode = StakingInputMode::PoolJoin;
        app.pool_input_amount = "1".to_string();
        app.selected_pool_for_join = None;
        let action = app.confirm_staking_operation();
        assert!(action.is_none());
    }

    #[test]
    fn test_confirm_staking_operation_pool_bond_extra() {
        let mut app = create_app();
        app.staking_input_mode = StakingInputMode::PoolBondExtra;
        app.pool_input_amount = "1".to_string();
        let action = app.confirm_staking_operation();
        assert!(matches!(
            action,
            Some(Action::GeneratePoolBondExtraQR {
                amount: 10_000_000_000
            })
        ));
    }

    #[test]
    fn test_confirm_staking_operation_pool_unbond() {
        let mut app = create_app();
        app.staking_input_mode = StakingInputMode::PoolUnbond;
        app.pool_input_amount = "1".to_string();
        let action = app.confirm_staking_operation();
        assert!(matches!(
            action,
            Some(Action::GeneratePoolUnbondQR {
                amount: 10_000_000_000
            })
        ));
    }

    #[test]
    fn test_confirm_staking_operation_invalid_amount() {
        let mut app = create_app();
        app.staking_input_mode = StakingInputMode::Bond;
        app.staking_input_amount = "invalid".to_string();
        let action = app.confirm_staking_operation();
        assert!(action.is_none());
    }

    // === maybe_auto_load_history ===

    #[test]
    fn test_maybe_auto_load_history_should_load() {
        let mut app = create_app();
        app.watched_account = Some(AccountId32::from([1u8; 32]));
        app.history.loading = false;
        app.history.points.clear();
        let action = app.maybe_auto_load_history();
        assert!(matches!(action, Some(Action::LoadStakingHistory)));
        assert!(app.history.loading);
        assert_eq!(
            app.history.loaded_for,
            Some(AccountId32::from([1u8; 32]).to_string())
        );
    }

    #[test]
    fn test_maybe_auto_load_history_already_loaded_same_account() {
        let mut app = create_app();
        let account = AccountId32::from([1u8; 32]);
        app.watched_account = Some(account.clone());
        app.history.loading = false;
        app.history.loaded_for = Some(account.to_string());
        app.history.points = vec![StakingHistoryPoint::new_without_date(1, 100, 1000, 0.15)];
        let action = app.maybe_auto_load_history();
        assert!(action.is_none());
    }

    #[test]
    fn test_maybe_auto_load_history_different_account() {
        let mut app = create_app();
        app.watched_account = Some(AccountId32::from([2u8; 32]));
        app.history.loading = false;
        app.history.loaded_for = Some(AccountId32::from([1u8; 32]).to_string());
        app.history.points = vec![StakingHistoryPoint::new_without_date(1, 100, 1000, 0.15)];
        let action = app.maybe_auto_load_history();
        assert!(matches!(action, Some(Action::LoadStakingHistory)));
    }

    #[test]
    fn test_maybe_auto_load_history_already_loading() {
        let mut app = create_app();
        app.watched_account = Some(AccountId32::from([1u8; 32]));
        app.history.loading = true;
        let action = app.maybe_auto_load_history();
        assert!(action.is_none());
    }

    #[test]
    fn test_maybe_auto_load_history_no_account() {
        let mut app = create_app();
        app.watched_account = None;
        let action = app.maybe_auto_load_history();
        assert!(action.is_none());
    }

    #[test]
    fn test_maybe_auto_load_history_empty_points() {
        let mut app = create_app();
        app.watched_account = Some(AccountId32::from([1u8; 32]));
        app.history.loading = false;
        app.history.points.clear();
        app.history.loaded_for = Some("other".to_string());
        let action = app.maybe_auto_load_history();
        assert!(matches!(action, Some(Action::LoadStakingHistory)));
    }

    // === KeyEvent helpers ===

    fn key_char(c: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn key_code(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    // === handle_normal_key ===

    #[test]
    fn test_handle_normal_key_quit() {
        let mut app = create_app();
        app.handle_normal_key(key_char('q'));
        assert!(app.should_quit);
    }

    #[test]
    fn test_handle_normal_key_next_view() {
        let mut app = create_app();
        let action = app.handle_normal_key(key_code(KeyCode::Tab));
        assert!(action.is_none());
        assert_eq!(app.current_view, View::AccountChanges);
    }

    #[test]
    fn test_handle_normal_key_prev_view() {
        let mut app = create_app();
        let action = app.handle_normal_key(key_code(KeyCode::BackTab));
        assert!(action.is_none());
        assert_eq!(app.current_view, View::Pools);
    }

    #[test]
    fn test_handle_normal_key_view_numbers() {
        let mut app = create_app();
        app.handle_normal_key(key_char('5'));
        assert_eq!(app.current_view, View::Validators);
        app.handle_normal_key(key_char('2'));
        assert_eq!(app.current_view, View::AccountChanges);
    }

    #[test]
    fn test_handle_normal_key_account_history_auto_load() {
        let mut app = create_app();
        app.watched_account = Some(AccountId32::from([0u8; 32]));
        let action = app.handle_normal_key(key_char('3'));
        assert_eq!(app.current_view, View::AccountHistory);
        assert!(matches!(action, Some(Action::LoadStakingHistory)));
        assert!(app.history.loading);
    }

    #[test]
    fn test_handle_normal_key_search_toggle() {
        let mut app = create_app();
        app.current_view = View::Validators;
        app.handle_normal_key(key_char('/'));
        assert_eq!(app.input_mode, InputMode::Searching);
        assert!(app.search_query.is_empty());
    }

    #[test]
    fn test_handle_normal_key_sort_menu() {
        let mut app = create_app();
        app.current_view = View::Validators;
        app.handle_normal_key(key_char('s'));
        assert_eq!(app.input_mode, InputMode::SortMenu);
    }

    #[test]
    fn test_handle_normal_key_strategy_menu() {
        let mut app = create_app();
        app.current_view = View::Nominate;
        app.handle_normal_key(key_char('t'));
        assert_eq!(app.input_mode, InputMode::StrategyMenu);
    }

    #[test]
    fn test_handle_normal_key_toggle_blocked() {
        let mut app = create_app();
        app.current_view = View::Validators;
        app.show_blocked = true;
        app.handle_normal_key(key_char('b'));
        assert!(!app.show_blocked);
    }

    #[test]
    fn test_handle_normal_key_reverse_sort_validators() {
        let mut app = create_app();
        app.current_view = View::Validators;
        app.validator_sort_asc = false;
        app.handle_normal_key(key_char('S'));
        assert!(app.validator_sort_asc);
    }

    #[test]
    fn test_handle_normal_key_enter_account() {
        let mut app = create_app();
        app.current_view = View::AccountStatus;
        app.handle_normal_key(key_char('a'));
        assert_eq!(app.input_mode, InputMode::EnteringAccount);
        assert!(app.account_input.is_empty());
    }

    #[test]
    fn test_handle_normal_key_staking_bond() {
        let mut app = create_app();
        app.current_view = View::AccountChanges;
        app.watched_account = Some(AccountId32::from([0u8; 32]));
        app.handle_normal_key(key_char('b'));
        assert_eq!(app.input_mode, InputMode::Staking);
        assert_eq!(app.staking_input_mode, StakingInputMode::Bond);
    }

    #[test]
    fn test_handle_normal_key_pool_join() {
        let mut app = create_app();
        app.current_view = View::Pools;
        app.watched_account = Some(AccountId32::from([0u8; 32]));
        app.pools = vec![make_pool(1, "Alpha", PoolState::Open, None)];
        app.pools_table_state.select(Some(0));
        let action = app.handle_normal_key(key_char('j'));
        assert_eq!(app.input_mode, InputMode::Staking);
        assert_eq!(app.staking_input_mode, StakingInputMode::PoolJoin);
        assert_eq!(app.selected_pool_for_join, Some(0));
        assert!(matches!(action, Some(Action::SelectPoolForJoin(0))));
    }

    #[test]
    fn test_handle_normal_key_pool_join_no_selection() {
        let mut app = create_app();
        app.current_view = View::Pools;
        app.watched_account = Some(AccountId32::from([0u8; 32]));
        app.pools = vec![make_pool(1, "Alpha", PoolState::Open, None)];
        let action = app.handle_normal_key(key_char('j'));
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(action.is_none());
    }

    #[test]
    fn test_handle_normal_key_generate_nomination_qr() {
        let mut app = create_app();
        app.current_view = View::Nominate;
        let action = app.handle_normal_key(key_char('g'));
        assert!(matches!(action, Some(Action::GenerateNominationQR)));
    }

    #[test]
    fn test_handle_normal_key_help() {
        let mut app = create_app();
        app.handle_normal_key(key_char('?'));
        assert!(app.showing_help);
    }

    #[test]
    fn test_handle_normal_key_help_esc() {
        let mut app = create_app();
        app.showing_help = true;
        app.handle_normal_key(key_code(KeyCode::Esc));
        assert!(!app.showing_help);
    }

    #[test]
    fn test_handle_normal_key_qr_modal_esc() {
        let mut app = create_app();
        app.qr.showing = true;
        app.qr.data = Some(vec![1, 2, 3]);
        let action = app.handle_normal_key(key_code(KeyCode::Esc));
        assert!(!app.qr.showing);
        assert!(app.qr.data.is_none());
        assert!(action.is_none());
    }

    #[test]
    fn test_handle_normal_key_qr_modal_esc_while_scanning() {
        let mut app = create_app();
        app.qr.showing = true;
        app.camera.scanning = true;
        let action = app.handle_normal_key(key_code(KeyCode::Esc));
        assert!(!app.qr.showing);
        assert!(matches!(action, Some(Action::StopSignatureScan)));
    }

    #[test]
    fn test_handle_normal_key_network_switch() {
        let mut app = create_app();
        let action = app.handle_normal_key(key_char('n'));
        assert!(matches!(
            action,
            Some(Action::SwitchNetwork(Network::Kusama))
        ));
    }

    #[test]
    fn test_handle_normal_key_clear_account() {
        let mut app = create_app();
        app.current_view = View::AccountStatus;
        let action = app.handle_normal_key(key_char('c'));
        assert!(matches!(action, Some(Action::ClearAccount)));
    }

    #[test]
    fn test_handle_normal_key_run_optimization() {
        let mut app = create_app();
        app.current_view = View::Nominate;
        let action = app.handle_normal_key(key_char('o'));
        assert!(matches!(action, Some(Action::RunOptimization)));
    }

    #[test]
    fn test_handle_normal_key_load_history() {
        let mut app = create_app();
        app.current_view = View::AccountHistory;
        app.watched_account = Some(AccountId32::from([0u8; 32]));
        let action = app.handle_normal_key(key_char('l'));
        assert!(matches!(action, Some(Action::LoadStakingHistory)));
    }

    #[test]
    fn test_handle_normal_key_cancel_history() {
        let mut app = create_app();
        app.current_view = View::AccountHistory;
        app.history.loading = true;
        let action = app.handle_normal_key(key_char('c'));
        assert!(matches!(action, Some(Action::CancelLoadingHistory)));
    }

    #[test]
    fn test_handle_normal_key_unknown() {
        let mut app = create_app();
        let action = app.handle_normal_key(key_char('z'));
        assert!(action.is_none());
        assert!(!app.should_quit);
    }

    #[test]
    fn test_handle_normal_key_address_book_enter_no_selection() {
        let mut app = create_app();
        app.current_view = View::AccountStatus;
        app.account_panel_focus = 1;
        let action = app.handle_normal_key(key_code(KeyCode::Enter));
        assert!(action.is_none());
    }

    // === handle_search_key ===

    #[test]
    fn test_handle_search_key_char_input() {
        let mut app = create_app();
        app.input_mode = InputMode::Searching;
        app.handle_search_key(key_char('a'));
        app.handle_search_key(key_char('b'));
        assert_eq!(app.search_query, "ab");
    }

    #[test]
    fn test_handle_search_key_backspace() {
        let mut app = create_app();
        app.input_mode = InputMode::Searching;
        app.search_query = "ab".to_string();
        app.handle_search_key(key_code(KeyCode::Backspace));
        assert_eq!(app.search_query, "a");
    }

    #[test]
    fn test_handle_search_key_enter() {
        let mut app = create_app();
        app.input_mode = InputMode::Searching;
        app.handle_search_key(key_code(KeyCode::Enter));
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn test_handle_search_key_esc() {
        let mut app = create_app();
        app.input_mode = InputMode::Searching;
        app.search_query = "test".to_string();
        app.handle_search_key(key_code(KeyCode::Esc));
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    // === handle_sort_menu_key ===

    #[test]
    fn test_handle_sort_menu_key_valid_validator() {
        let mut app = create_app();
        app.input_mode = InputMode::SortMenu;
        app.current_view = View::Validators;
        app.handle_sort_menu_key(key_char('n'));
        assert_eq!(app.validator_sort, ValidatorSortField::Name);
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn test_handle_sort_menu_key_valid_pool() {
        let mut app = create_app();
        app.input_mode = InputMode::SortMenu;
        app.current_view = View::Pools;
        app.handle_sort_menu_key(key_char('i'));
        assert_eq!(app.pool_sort, PoolSortField::Id);
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn test_handle_sort_menu_key_invalid() {
        let mut app = create_app();
        app.input_mode = InputMode::SortMenu;
        app.current_view = View::Validators;
        app.handle_sort_menu_key(key_char('z'));
        assert_eq!(app.input_mode, InputMode::SortMenu);
    }

    #[test]
    fn test_handle_sort_menu_key_esc() {
        let mut app = create_app();
        app.input_mode = InputMode::SortMenu;
        app.handle_sort_menu_key(key_code(KeyCode::Esc));
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    // === handle_strategy_menu_key ===

    #[test]
    fn test_handle_strategy_menu_key_up_down() {
        let mut app = create_app();
        app.input_mode = InputMode::StrategyMenu;
        app.strategy_index = 1;
        app.handle_strategy_menu_key(key_code(KeyCode::Up));
        assert_eq!(app.strategy_index, 0);
        app.handle_strategy_menu_key(key_code(KeyCode::Up));
        assert_eq!(app.strategy_index, 0); // clamped at 0
        app.handle_strategy_menu_key(key_code(KeyCode::Down));
        assert_eq!(app.strategy_index, 1);
        app.handle_strategy_menu_key(key_code(KeyCode::Down));
        assert_eq!(app.strategy_index, 2);
        app.handle_strategy_menu_key(key_code(KeyCode::Down));
        assert_eq!(app.strategy_index, 2); // clamped at 2
    }

    #[test]
    fn test_handle_strategy_menu_key_enter() {
        let mut app = create_app();
        app.input_mode = InputMode::StrategyMenu;
        app.strategy_index = 1;
        let action = app.handle_strategy_menu_key(key_code(KeyCode::Enter));
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(matches!(
            action,
            Some(Action::RunOptimizationWithStrategy(1))
        ));
    }

    #[test]
    fn test_handle_strategy_menu_key_number() {
        let mut app = create_app();
        app.input_mode = InputMode::StrategyMenu;
        let action = app.handle_strategy_menu_key(key_char('2'));
        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(app.strategy_index, 1);
        assert!(matches!(
            action,
            Some(Action::RunOptimizationWithStrategy(1))
        ));
    }

    #[test]
    fn test_handle_strategy_menu_key_esc() {
        let mut app = create_app();
        app.input_mode = InputMode::StrategyMenu;
        app.handle_strategy_menu_key(key_code(KeyCode::Esc));
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    // === handle_input_key ===

    #[test]
    fn test_handle_input_key_char() {
        let mut app = create_app();
        app.input_mode = InputMode::EnteringAccount;
        app.handle_input_key(key_char('1'));
        assert_eq!(app.account_input, "1");
    }

    #[test]
    fn test_handle_input_key_backspace() {
        let mut app = create_app();
        app.input_mode = InputMode::EnteringAccount;
        app.account_input = "12".to_string();
        app.handle_input_key(key_code(KeyCode::Backspace));
        assert_eq!(app.account_input, "1");
    }

    #[test]
    fn test_handle_input_key_enter_valid() {
        let mut app = create_app();
        app.input_mode = InputMode::EnteringAccount;
        app.account_input = "5C4hrfjw9DjXZTzV3MwzrrAr9P1MJhSrvWGWqi1eSuyUpnhM".to_string();
        let action = app.handle_input_key(key_code(KeyCode::Enter));
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(matches!(action, Some(Action::SetWatchedAccount(_, _))));
        assert!(app.validation_error.is_none());
    }

    #[test]
    fn test_handle_input_key_enter_invalid() {
        let mut app = create_app();
        app.input_mode = InputMode::EnteringAccount;
        app.account_input = "bad".to_string();
        let action = app.handle_input_key(key_code(KeyCode::Enter));
        assert_eq!(app.input_mode, InputMode::EnteringAccount);
        assert!(action.is_none());
        assert!(app.validation_error.is_some());
    }

    #[test]
    fn test_handle_input_key_esc() {
        let mut app = create_app();
        app.input_mode = InputMode::EnteringAccount;
        app.account_input = "test".to_string();
        app.validation_error = Some("err".to_string());
        app.handle_input_key(key_code(KeyCode::Esc));
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(app.account_input.is_empty());
        assert!(app.validation_error.is_none());
    }

    // === validate_account_input ===

    #[test]
    fn test_validate_account_input_empty() {
        let app = create_app();
        assert_eq!(
            app.validate_account_input(),
            Err("Please enter an address".to_string())
        );
    }

    #[test]
    fn test_validate_account_input_too_short() {
        let mut app = create_app();
        app.account_input = "12".to_string();
        assert_eq!(
            app.validate_account_input(),
            Err("Address is too short (minimum 3 characters)".to_string())
        );
    }

    #[test]
    fn test_validate_account_input_invalid_non_base58() {
        let mut app = create_app();
        app.account_input = "abc".to_string();
        assert!(app.validate_account_input().is_err());
    }

    #[test]
    fn test_validate_account_input_invalid_ss58() {
        let mut app = create_app();
        app.account_input =
            "123456789012345678901234567890123456789012345678901234567890".to_string();
        let result = app.validate_account_input();
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_account_input_valid() {
        let mut app = create_app();
        app.account_input = "5C4hrfjw9DjXZTzV3MwzrrAr9P1MJhSrvWGWqi1eSuyUpnhM".to_string();
        let result = app.validate_account_input();
        assert!(result.is_ok());
    }

    // === handle_qr_modal_key ===

    #[test]
    fn test_handle_qr_modal_key_esc() {
        let mut app = create_app();
        app.qr.showing = true;
        app.qr.data = Some(vec![1]);
        app.qr.modal_tab = 1;
        app.handle_qr_modal_key(key_code(KeyCode::Esc));
        assert!(!app.qr.showing);
        assert_eq!(app.qr.modal_tab, 0);
    }

    #[test]
    fn test_handle_qr_modal_key_tab_forward() {
        let mut app = create_app();
        app.qr.showing = true;
        app.qr.pending_signed = None;
        app.qr.modal_tab = 0;
        app.handle_qr_modal_key(key_code(KeyCode::Tab));
        assert_eq!(app.qr.modal_tab, 1);
    }

    #[test]
    fn test_handle_qr_modal_key_backtab_backward() {
        let mut app = create_app();
        app.qr.showing = true;
        app.qr.pending_signed = None;
        app.qr.modal_tab = 0;
        app.handle_qr_modal_key(key_code(KeyCode::BackTab));
        assert_eq!(app.qr.modal_tab, 2);
    }

    #[test]
    fn test_handle_qr_modal_key_s_start_scan() {
        let mut app = create_app();
        app.qr.showing = true;
        app.qr.pending_unsigned = Some(PendingUnsignedTx {
            payload: UnsignedPayload {
                call_data: vec![],
                description: "test".to_string(),
                metadata_hash: [0u8; 32],
                genesis_hash: [0u8; 32],
                block_hash: [0u8; 32],
                nonce: 0,
                spec_version: 0,
                tx_version: 0,
                era: stkopt_chain::Era::Immortal,
                include_metadata_hash: false,
                use_asset_payment: false,
                extension_ids: vec![],
            },
            signer: AccountId32::from([0u8; 32]),
        });
        app.qr.pending_signed = None;
        let action = app.handle_qr_modal_key(key_char('s'));
        assert_eq!(app.qr.modal_tab, 2);
        assert!(matches!(
            app.camera.status,
            Some(CameraScanStatus::Initializing)
        ));
        assert!(matches!(action, Some(Action::StartSignatureScan)));
    }

    #[test]
    fn test_handle_qr_modal_key_enter_submit() {
        let mut app = create_app();
        app.qr.showing = true;
        app.qr.pending_signed = Some(PendingTransaction {
            signed_extrinsic: vec![],
            tx_hash: [0u8; 32],
            status: TxSubmissionStatus::ReadyToSubmit,
        });
        app.qr.modal_tab = 3;
        let action = app.handle_qr_modal_key(key_code(KeyCode::Enter));
        assert!(matches!(action, Some(Action::SubmitTransaction)));
    }

    // === handle_qr_tab_change ===

    #[test]
    fn test_handle_qr_tab_change_to_scan() {
        let mut app = create_app();
        app.qr.modal_tab = 2;
        app.qr.pending_unsigned = Some(PendingUnsignedTx {
            payload: UnsignedPayload {
                call_data: vec![],
                description: "test".to_string(),
                metadata_hash: [0u8; 32],
                genesis_hash: [0u8; 32],
                block_hash: [0u8; 32],
                nonce: 0,
                spec_version: 0,
                tx_version: 0,
                era: stkopt_chain::Era::Immortal,
                include_metadata_hash: false,
                use_asset_payment: false,
                extension_ids: vec![],
            },
            signer: AccountId32::from([0u8; 32]),
        });
        app.camera.scanning = false;
        let action = app.handle_qr_tab_change();
        assert!(matches!(action, Some(Action::StartSignatureScan)));
        assert!(matches!(
            app.camera.status,
            Some(CameraScanStatus::Initializing)
        ));
    }

    #[test]
    fn test_handle_qr_tab_change_from_scan() {
        let mut app = create_app();
        app.qr.modal_tab = 1;
        app.camera.scanning = true;
        let action = app.handle_qr_tab_change();
        assert!(matches!(action, Some(Action::StopSignatureScan)));
        assert!(app.camera.status.is_none());
    }

    #[test]
    fn test_handle_qr_tab_change_no_op() {
        let mut app = create_app();
        app.qr.modal_tab = 0;
        app.camera.scanning = false;
        let action = app.handle_qr_tab_change();
        assert!(action.is_none());
    }
}
