//! Application state and logic.

use crate::action::{
    AccountStatus, Action, DisplayPool, DisplayValidator, PendingTransaction, PendingUnsignedTx,
    PoolOperation, QrScanStatus, StakingHistoryPoint, StakingInputMode, TransactionInfo,
    TxSubmissionStatus,
};
use crate::log_buffer::LogBuffer;
use crate::theme::{Palette, Theme};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::TableState;
use std::collections::HashSet;
use stkopt_chain::{ChainInfo, RewardDestination};
use stkopt_core::{ConnectionStatus, Network, OptimizationResult};
use subxt::utils::AccountId32;

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
    /// Total eras to load for history.
    pub total_eras: u32,
    /// Account for which history was loaded (to detect account changes).
    pub loaded_for: Option<String>,
}

impl HistoryState {
    fn new() -> Self {
        Self {
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
    #[allow(dead_code)]
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
    /// Optimization strategy selection index.
    pub strategy_index: usize,

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
    /// Current pool operation.
    pub pool_operation: PoolOperation,
    /// Input buffer for pool amount.
    pub pool_input_amount: String,
    /// Selected pool for join operation.
    pub selected_pool_for_join: Option<usize>,
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
            strategy_index: 0,

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
            pool_operation: PoolOperation::default(),
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
            KeyCode::Char('n') => self.next_network(),
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
                if !self.selected_validators.is_empty() && self.watched_account.is_some() {
                    return Some(Action::GenerateNominationQR);
                }
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
            }
            // Sort menu with s
            KeyCode::Char('s') if matches!(self.current_view, View::Validators | View::Pools) => {
                self.input_mode = InputMode::SortMenu;
            }
            // Reverse sort with S
            KeyCode::Char('S') if self.current_view == View::Validators => {
                self.validator_sort_asc = !self.validator_sort_asc;
            }
            KeyCode::Char('S') if self.current_view == View::Pools => {
                self.pool_sort_asc = !self.pool_sort_asc;
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
            }
            // Select from address book with Enter when focused on address book
            KeyCode::Enter
                if self.current_view == View::AccountStatus && self.account_panel_focus == 1 =>
            {
                if let Some(idx) = self.address_book_state.selected() {
                    // Return action to select this address
                    return Some(Action::SelectAddressBookEntry(idx));
                }
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
            _ => {}
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
            }
            KeyCode::Char(c) => {
                self.search_query.push(c);
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
                        self.input_mode = InputMode::Normal;
                    }
                }
                View::Pools => {
                    if let Some(field) = PoolSortField::from_key(c) {
                        self.pool_sort = field;
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
        let decimals = self.network.token_decimals() as u32;
        let parts: Vec<&str> = input.split('.').collect();
        if parts.len() > 2 {
            return None;
        }

        let whole = parts[0].parse::<u128>().ok()?;
        let frac_part = if parts.len() == 2 { parts[1] } else { "" };

        let mut frac = 0u128;
        if !frac_part.is_empty() {
            // Pad or truncate to decimals
            let mut s = frac_part.to_string();
            if s.len() > decimals as usize {
                s.truncate(decimals as usize);
            } else {
                while s.len() < decimals as usize {
                    s.push('0');
                }
            }
            frac = s.parse::<u128>().ok()?;
        }

        let whole_units = whole.checked_mul(10u128.pow(decimals))?;
        whole_units.checked_add(frac)
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

        // SS58 addresses typically start with a prefix number (network identifier)
        // Valid prefixes: 0-99 followed by the base58-encoded address
        if !input.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return Err("Address should start with a network prefix (0-9)".to_string());
        }

        // Try to parse as SS58 address
        match std::str::FromStr::from_str(input) {
            Ok(account) => Ok(account),
            Err(_) => {
                let base58_hint =
                    if input.contains(|c: char| !c.is_ascii_alphanumeric() && c != '-') {
                        "Remove special characters from the address".to_string()
                    } else if input.len() < 47 {
                        format!(
                            "SS58 address too short (got {}, expected ~47 characters)",
                            input.len()
                        )
                    } else if input.len() > 49 {
                        format!(
                            "SS58 address too long (got {}, expected ~47 characters)",
                            input.len()
                        )
                    } else {
                        "Invalid SS58 format - check for typos".to_string()
                    };
                Err(format!("Invalid address: {}", base58_hint))
            }
        }
    }

    /// Parse the account input as SS58 address (legacy method for backward compatibility).
    #[allow(dead_code)]
    fn parse_account_input(&self) -> Option<AccountId32> {
        self.validate_account_input().ok()
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
    fn next_network(&mut self) {
        let networks = Network::all();
        let current_idx = networks
            .iter()
            .position(|n| *n == self.network)
            .unwrap_or(0);
        let next_idx = (current_idx + 1) % networks.len();
        self.network = networks[next_idx];
        self.connection_status = ConnectionStatus::Disconnected;
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
            Action::SetValidators(_validators) => {
                // Raw validators - will be processed with exposures
                self.loading.validators = true;
            }
            Action::SetEraExposures(_era, _exposures) => {
                // Will be combined with validators to compute APY
            }
            Action::SetEraReward(_era, _reward) => {
                // Will be used for APY calculation
            }
            Action::SetDisplayValidators(validators) => {
                self.validators = validators;
                self.loading.validators = false;
                // Select first row if we have validators
                if !self.validators.is_empty() && self.validators_table_state.selected().is_none() {
                    self.validators_table_state.select(Some(0));
                }
            }
            Action::SetDisplayPools(pools) => {
                self.pools = pools;
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
                self.optimization_result = Some(result);
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
            }
            Action::ClearNominations => {
                self.selected_validators.clear();
                self.optimization_result = None;
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
            }
            Action::SelectAddressBookEntry(_idx) => {
                // Handled in main.rs where we have access to the address book entries
            }
            Action::RemoveAccount(_addr) => {
                // Handled in main.rs where we have access to config and database
            }
            Action::ValidateAccount(_addr) => {
                // Validation is done during input, this action is no longer used
            }
            Action::ClearValidationError => {
                self.validation_error = None;
            }
            // Staking actions
            Action::SetStakingInputMode(mode) => {
                self.staking_input_mode = mode;
            }
            Action::UpdateStakingAmount(amount) => {
                if matches!(
                    self.staking_input_mode,
                    crate::action::StakingInputMode::PoolJoin
                        | crate::action::StakingInputMode::PoolUnbond
                        | crate::action::StakingInputMode::PoolBondExtra
                ) {
                    self.pool_input_amount = amount;
                } else {
                    self.staking_input_amount = amount;
                }
            }
            Action::SetRewardsDestination(dest) => {
                self.rewards_destination = dest;
            }
            Action::SetPoolOperation(op) => {
                self.pool_operation = op;
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
            Action::Quit => {
                self.should_quit = true;
            }
        }
    }

    /// Get the number of entries in the address book.
    pub fn address_book_len(&self) -> usize {
        // 1 for "My Account" if set, plus 3 hardcoded entries
        let my_account = if self.watched_account.is_some() { 1 } else { 0 };
        my_account + 3
    }

    /// Move selection up in the current list.
    pub fn select_previous(&mut self) {
        match self.current_view {
            View::Validators if !self.validators.is_empty() => {
                let i = match self.validators_table_state.selected() {
                    Some(i) => {
                        if i == 0 {
                            self.validators.len() - 1
                        } else {
                            i - 1
                        }
                    }
                    None => 0,
                };
                self.validators_table_state.select(Some(i));
            }
            View::Pools if !self.pools.is_empty() => {
                let i = match self.pools_table_state.selected() {
                    Some(i) => {
                        if i == 0 {
                            self.pools.len() - 1
                        } else {
                            i - 1
                        }
                    }
                    None => 0,
                };
                self.pools_table_state.select(Some(i));
            }
            View::Nominate if !self.validators.is_empty() => {
                let i = match self.nominate_table_state.selected() {
                    Some(i) => {
                        if i == 0 {
                            self.validators.len() - 1
                        } else {
                            i - 1
                        }
                    }
                    None => 0,
                };
                self.nominate_table_state.select(Some(i));
            }
            View::AccountStatus if self.account_panel_focus == 1 => {
                let len = self.address_book_len();
                if len > 0 {
                    let i = match self.address_book_state.selected() {
                        Some(i) => {
                            if i == 0 {
                                len - 1
                            } else {
                                i - 1
                            }
                        }
                        None => 0,
                    };
                    self.address_book_state.select(Some(i));
                }
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

    /// Move selection down in the current list.
    pub fn select_next(&mut self) {
        match self.current_view {
            View::Validators if !self.validators.is_empty() => {
                let i = match self.validators_table_state.selected() {
                    Some(i) => {
                        if i >= self.validators.len() - 1 {
                            0
                        } else {
                            i + 1
                        }
                    }
                    None => 0,
                };
                self.validators_table_state.select(Some(i));
            }
            View::Pools if !self.pools.is_empty() => {
                let i = match self.pools_table_state.selected() {
                    Some(i) => {
                        if i >= self.pools.len() - 1 {
                            0
                        } else {
                            i + 1
                        }
                    }
                    None => 0,
                };
                self.pools_table_state.select(Some(i));
            }
            View::Nominate if !self.validators.is_empty() => {
                let i = match self.nominate_table_state.selected() {
                    Some(i) => {
                        if i >= self.validators.len() - 1 {
                            0
                        } else {
                            i + 1
                        }
                    }
                    None => 0,
                };
                self.nominate_table_state.select(Some(i));
            }
            View::AccountStatus if self.account_panel_focus == 1 => {
                let len = self.address_book_len();
                if len > 0 {
                    let i = match self.address_book_state.selected() {
                        Some(i) => {
                            if i >= len - 1 {
                                0
                            } else {
                                i + 1
                            }
                        }
                        None => 0,
                    };
                    self.address_book_state.select(Some(i));
                }
            }
            _ => {}
        }
    }
}
