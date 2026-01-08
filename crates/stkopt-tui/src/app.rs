//! Application state and logic.

use crate::action::{
    AccountStatus, Action, DisplayPool, DisplayValidator, PendingTransaction, PendingUnsignedTx,
    QrScanStatus, StakingHistoryPoint, TransactionInfo, TxSubmissionStatus,
};
use crate::log_buffer::LogBuffer;
use crate::theme::{Palette, Theme};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::TableState;
use std::collections::HashSet;
use stkopt_chain::ChainInfo;
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
    Account,
    History,
    Nominate,
    Pools,
    Validators,
}

impl View {
    pub fn all() -> &'static [View] {
        &[
            View::Account,
            View::History,
            View::Nominate,
            View::Pools,
            View::Validators,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            View::Account => "Account",
            View::History => "History",
            View::Nominate => "Nominate",
            View::Pools => "Pools",
            View::Validators => "Validators",
        }
    }

    pub fn index(&self) -> usize {
        match self {
            View::Account => 0,
            View::History => 1,
            View::Nominate => 2,
            View::Pools => 3,
            View::Validators => 4,
        }
    }

    pub fn from_index(index: usize) -> View {
        match index {
            0 => View::Account,
            1 => View::History,
            2 => View::Nominate,
            3 => View::Pools,
            4 => View::Validators,
            _ => View::Account,
        }
    }
}

/// Application state.
pub struct App {
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
    /// Current era index.
    pub current_era: Option<u32>,
    /// Era completion percentage.
    pub era_pct_complete: f64,
    /// Era duration in milliseconds.
    pub era_duration_ms: u64,
    /// Display validators (aggregated data).
    pub validators: Vec<DisplayValidator>,
    /// Validators table state.
    pub validators_table_state: TableState,
    /// Display nomination pools (aggregated data).
    pub pools: Vec<DisplayPool>,
    /// Pools table state.
    pub pools_table_state: TableState,
    /// Loading progress (0.0 - 1.0).
    pub loading_progress: f32,
    /// Whether validators are loading.
    pub loading_validators: bool,
    /// Current input mode.
    pub input_mode: InputMode,
    /// Input buffer for entering account address.
    pub account_input: String,
    /// Watched account address.
    pub watched_account: Option<AccountId32>,
    /// Account status (balance, staking, nominations).
    pub account_status: Option<AccountStatus>,
    /// Nomination optimization result.
    pub optimization_result: Option<OptimizationResult>,
    /// Manually selected validators (indices into validators list).
    pub selected_validators: HashSet<usize>,
    /// Nominate table state.
    pub nominate_table_state: TableState,
    /// QR code data to display (if any).
    pub qr_data: Option<Vec<u8>>,
    /// Transaction info for QR code display.
    pub qr_tx_info: Option<TransactionInfo>,
    /// Current QR frame for animated multipart display.
    pub qr_frame: usize,
    /// Whether showing QR code modal.
    pub showing_qr: bool,
    /// Current tab in QR modal (0=QR, 1=Details, 2=Scan).
    pub qr_modal_tab: usize,
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
    /// Staking history for the watched account (last 30 eras).
    pub staking_history: Vec<StakingHistoryPoint>,
    /// Whether staking history is currently loading.
    pub loading_history: bool,
    /// Total eras to load for history.
    pub history_total_eras: u32,
    /// Account for which history was loaded (to detect account changes).
    pub history_loaded_for: Option<String>,
    /// Search query for filtering.
    pub search_query: String,
    /// Whether to show blocked validators.
    pub show_blocked: bool,
    /// Validator sort field.
    pub validator_sort: ValidatorSortField,
    /// Validator sort ascending (false = descending).
    pub validator_sort_asc: bool,
    /// Pool sort field.
    pub pool_sort: PoolSortField,
    /// Pool sort ascending (false = descending).
    pub pool_sort_asc: bool,
    /// Currently focused panel in account view (0 = status, 1 = address book).
    pub account_panel_focus: usize,
    /// Address book table state.
    pub address_book_state: TableState,
    /// Optimization strategy selection index.
    pub strategy_index: usize,
    /// Pending unsigned transaction (waiting for signature from Vault).
    pub pending_unsigned_tx: Option<PendingUnsignedTx>,
    /// Pending signed transaction (ready for/in-progress submission).
    pub pending_tx: Option<PendingTransaction>,
    /// Whether currently scanning for signature QR code.
    pub scanning_signature: bool,
    /// Camera scan status for visual feedback (None=not scanning, Some=latest status).
    pub camera_scan_status: Option<CameraScanStatus>,
    /// Whether the chain is still connecting/syncing.
    pub loading_chain: bool,
    /// Whether to show the account input prompt popup.
    pub show_account_prompt: bool,
    /// Tick counter for loading spinner animation.
    pub spinner_tick: usize,
    /// Whether data was loaded from cache (validators, etc).
    pub using_cached_data: bool,
    /// Bytes loaded so far (for bandwidth estimation).
    pub bytes_loaded: u64,
    /// Estimated total bytes to load.
    pub estimated_total_bytes: Option<u64>,
    /// Load start time (for bandwidth calculation).
    pub load_start_time: Option<std::time::Instant>,
    /// Estimated bandwidth in bytes per second (computed after initial period).
    pub estimated_bandwidth: Option<f64>,
    /// Validation error message for account input (None if valid or not validating).
    pub validation_error: Option<String>,
}

impl App {
    /// Create a new application instance.
    pub fn new(network: Network, log_buffer: LogBuffer, theme: Theme) -> Self {
        let palette = theme.palette();
        Self {
            theme,
            palette,
            network,
            connection_status: ConnectionStatus::Disconnected,
            chain_info: None,
            current_view: View::default(),
            current_era: None,
            era_pct_complete: 0.0,
            era_duration_ms: 0,
            validators: Vec::new(),
            validators_table_state: TableState::default(),
            pools: Vec::new(),
            pools_table_state: TableState::default(),
            loading_progress: 0.0,
            loading_validators: false,
            input_mode: InputMode::default(),
            account_input: String::new(),
            watched_account: None,
            account_status: None,
            optimization_result: None,
            selected_validators: HashSet::new(),
            nominate_table_state: TableState::default(),
            qr_data: None,
            qr_tx_info: None,
            qr_frame: 0,
            showing_qr: false,
            qr_modal_tab: 0,
            showing_help: false,
            should_quit: false,
            tick_count: 0,
            log_buffer,
            log_scroll: 0,
            staking_history: Vec::new(),
            loading_history: false,
            history_total_eras: 30,
            history_loaded_for: None,
            search_query: String::new(),
            show_blocked: true,
            validator_sort: ValidatorSortField::default(),
            validator_sort_asc: false, // Default descending (highest APY first)
            pool_sort: PoolSortField::default(),
            pool_sort_asc: false,
            account_panel_focus: 0,
            address_book_state: TableState::default(),
            strategy_index: 0,
            pending_unsigned_tx: None,
            pending_tx: None,
            scanning_signature: false,
            camera_scan_status: None,
            loading_chain: true, // Start in loading state
            show_account_prompt: false,
            spinner_tick: 0,
            using_cached_data: false,
            bytes_loaded: 0,
            estimated_total_bytes: None,
            load_start_time: Some(std::time::Instant::now()),
            estimated_bandwidth: None,
            validation_error: None,
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
        if self.showing_qr {
            self.qr_frame = self.qr_frame.wrapping_add(1);
        }

        // Advance spinner for loading animation
        if self.loading_chain || self.loading_validators {
            self.spinner_tick = self.spinner_tick.wrapping_add(1);
        }
    }

    /// Get the current spinner character for loading animation.
    pub fn spinner_char(&self) -> char {
        const SPINNER_CHARS: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        SPINNER_CHARS[self.spinner_tick % SPINNER_CHARS.len()]
    }

    /// Estimate remaining time in seconds based on bandwidth and progress.
    pub fn estimated_remaining_secs(&self) -> Option<f64> {
        let bw = self.estimated_bandwidth?;
        if bw <= 0.0 {
            return None;
        }

        let total = self.estimated_total_bytes?;
        let remaining = total.saturating_sub(self.bytes_loaded);
        Some(remaining as f64 / bw)
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
        }
    }

    /// Handle keyboard input in normal mode.
    fn handle_normal_key(&mut self, key: KeyEvent) -> Option<Action> {
        // Close QR modal if showing
        if self.showing_qr {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.showing_qr = false;
                    self.qr_data = None;
                    self.qr_tx_info = None;
                    self.qr_frame = 0;
                    self.qr_modal_tab = 0;
                    self.camera_scan_status = None;
                    // Stop scanning if active
                    if self.scanning_signature {
                        return Some(Action::StopSignatureScan);
                    }
                }
                KeyCode::Tab | KeyCode::Right => {
                    // Cycle through tabs: QR -> Details -> Scan -> QR
                    self.qr_modal_tab = (self.qr_modal_tab + 1) % 3;
                    // Start scanning when entering Scan tab
                    if self.qr_modal_tab == 2
                        && self.pending_unsigned_tx.is_some()
                        && !self.scanning_signature
                    {
                        self.camera_scan_status = Some(CameraScanStatus::Initializing);
                        return Some(Action::StartSignatureScan);
                    }
                    // Stop scanning when leaving Scan tab
                    if self.qr_modal_tab != 2 && self.scanning_signature {
                        self.camera_scan_status = None;
                        return Some(Action::StopSignatureScan);
                    }
                }
                KeyCode::BackTab | KeyCode::Left => {
                    // Cycle backwards
                    self.qr_modal_tab = if self.qr_modal_tab == 0 {
                        2
                    } else {
                        self.qr_modal_tab - 1
                    };
                    // Start scanning when entering Scan tab
                    if self.qr_modal_tab == 2
                        && self.pending_unsigned_tx.is_some()
                        && !self.scanning_signature
                    {
                        self.camera_scan_status = Some(CameraScanStatus::Initializing);
                        return Some(Action::StartSignatureScan);
                    }
                    // Stop scanning when leaving Scan tab
                    if self.qr_modal_tab != 2 && self.scanning_signature {
                        self.camera_scan_status = None;
                        return Some(Action::StopSignatureScan);
                    }
                }
                // 's' to start scanning for signature (after Vault has signed) - shortcut to scan tab
                KeyCode::Char('s') if self.pending_unsigned_tx.is_some() => {
                    self.qr_modal_tab = 2;
                    self.camera_scan_status = Some(CameraScanStatus::Initializing);
                    return Some(Action::StartSignatureScan);
                }
                _ => {}
            }
            return None;
        }

        // Handle pending transaction state
        if let Some(ref tx) = self.pending_tx {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    return Some(Action::ClearPendingTx);
                }
                // 's' to submit the signed transaction
                KeyCode::Char('s') | KeyCode::Enter => {
                    if matches!(tx.status, TxSubmissionStatus::ReadyToSubmit) {
                        return Some(Action::SubmitTransaction);
                    }
                }
                _ => {}
            }
            return None;
        }

        // Handle help overlay
        if self.showing_help {
            match key.code {
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Enter => {
                    self.showing_help = false;
                }
                _ => {}
            }
            return None;
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Tab => return self.next_view(),
            KeyCode::BackTab => return self.prev_view(),
            KeyCode::Char('1') => self.current_view = View::Validators,
            KeyCode::Char('2') => self.current_view = View::Pools,
            KeyCode::Char('3') => self.current_view = View::Nominate,
            KeyCode::Char('4') => self.current_view = View::Account,
            KeyCode::Char('5') => {
                self.current_view = View::History;
                return self.maybe_auto_load_history();
            }
            KeyCode::Char('n') => self.next_network(),
            KeyCode::Up | KeyCode::Char('k') => self.select_previous(),
            KeyCode::Down | KeyCode::Char('j') => self.select_next(),
            KeyCode::Char('a')
                if self.current_view == View::Account || self.show_account_prompt =>
            {
                self.input_mode = InputMode::EnteringAccount;
                self.account_input.clear();
                self.show_account_prompt = false;
            }
            KeyCode::Char('c') if self.current_view == View::Account => {
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
            KeyCode::Char('l') if self.current_view == View::History => {
                if !self.loading_history && self.watched_account.is_some() {
                    return Some(Action::LoadStakingHistory);
                }
            }
            KeyCode::Char('c') if self.current_view == View::History => {
                if self.loading_history {
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
            KeyCode::Left if self.current_view == View::Account => {
                self.account_panel_focus = 0;
            }
            KeyCode::Right if self.current_view == View::Account => {
                self.account_panel_focus = 1;
            }
            // Select from address book with Enter when focused on address book
            KeyCode::Enter
                if self.current_view == View::Account && self.account_panel_focus == 1 =>
            {
                if let Some(idx) = self.address_book_state.selected() {
                    // Return action to select this address
                    return Some(Action::SelectAddressBookEntry(idx));
                }
            }
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

    /// Handle keyboard input when entering text.
    fn handle_input_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Enter => {
                self.input_mode = InputMode::Normal;
                match self.validate_account_input() {
                    Ok(account) => {
                        self.validation_error = None;
                        return Some(Action::SetWatchedAccount(account));
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
        if !input.chars().next().map_or(false, |c| c.is_ascii_digit()) {
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
    fn parse_account_input(&self) -> Option<AccountId32> {
        self.validate_account_input().ok()
    }

    /// Switch to next view.
    fn next_view(&mut self) -> Option<Action> {
        let views = View::all();
        let current_idx = self.current_view.index();
        let next_idx = (current_idx + 1) % views.len();
        self.current_view = View::from_index(next_idx);
        if self.current_view == View::History {
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
        if self.current_view == View::History {
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
            let should_load = !self.loading_history
                && (self.staking_history.is_empty()
                    || self.history_loaded_for.as_ref() != Some(&account_str));
            if should_load {
                self.staking_history.clear();
                self.loading_history = true;
                self.history_loaded_for = Some(account_str);
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
                self.loading_chain = false;
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
                self.loading_validators = true;
            }
            Action::SetEraExposures(_era, _exposures) => {
                // Will be combined with validators to compute APY
            }
            Action::SetEraReward(_era, _reward) => {
                // Will be used for APY calculation
            }
            Action::SetDisplayValidators(validators) => {
                self.validators = validators;
                self.loading_validators = false;
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
            Action::SetLoadingProgress(progress, bytes_loaded, estimated_total) => {
                self.loading_progress = progress;

                // Update bytes tracking
                if let Some(bytes) = bytes_loaded {
                    self.bytes_loaded = bytes;
                }
                if let Some(total) = estimated_total {
                    self.estimated_total_bytes = Some(total);
                }

                // Compute bandwidth after 10 seconds of loading
                if self.estimated_bandwidth.is_none()
                    && let Some(start) = self.load_start_time
                {
                    let elapsed = start.elapsed();
                    if elapsed.as_secs() >= 10 && self.bytes_loaded > 0 {
                        let bw = self.bytes_loaded as f64 / elapsed.as_secs_f64();
                        self.estimated_bandwidth = Some(bw);
                        tracing::info!("Estimated bandwidth: {:.1} KB/s", bw / 1024.0);
                    }
                }
            }
            Action::SetWatchedAccount(account) => {
                self.watched_account = Some(account);
                self.account_status = None; // Will be fetched
            }
            Action::SetAccountStatus(status) => {
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
                self.qr_data = data;
                self.qr_tx_info = tx_info;
                self.qr_frame = 0; // Reset animation frame for new QR
                self.qr_modal_tab = 0; // Reset to QR tab
                self.showing_qr = self.qr_data.is_some();
            }
            Action::SetPendingUnsignedTx(pending) => {
                self.pending_unsigned_tx = pending;
            }
            Action::StartSignatureScan => {
                self.scanning_signature = true;
            }
            Action::StopSignatureScan => {
                self.scanning_signature = false;
            }
            Action::SignatureScanned(_) => {
                // Handled in main.rs - processes the signature and creates pending_tx
            }
            Action::QrScanFailed(ref error) => {
                tracing::error!("QR scan failed: {}", error);
                self.scanning_signature = false;
                self.camera_scan_status = Some(CameraScanStatus::Error);
            }
            Action::UpdateScanStatus(status) => {
                self.camera_scan_status = Some(match status {
                    QrScanStatus::Scanning => CameraScanStatus::Scanning,
                    QrScanStatus::Detected => CameraScanStatus::Detected,
                    QrScanStatus::Success => CameraScanStatus::Success,
                });
            }
            Action::SubmitTransaction => {
                // Handled in main.rs - submits the pending_tx
            }
            Action::SetTxStatus(status) => {
                if let Some(ref mut tx) = self.pending_tx {
                    tx.status = status;
                }
            }
            Action::ClearPendingTx => {
                self.pending_tx = None;
                self.pending_unsigned_tx = None;
                self.scanning_signature = false;
            }
            Action::SetStakingHistory(history) => {
                self.staking_history = history;
                self.loading_history = false;
            }
            Action::AddStakingHistoryPoint(point) => {
                // Insert in era order (oldest first)
                let pos = self
                    .staking_history
                    .iter()
                    .position(|p| p.era > point.era)
                    .unwrap_or(self.staking_history.len());
                self.staking_history.insert(pos, point);
            }
            Action::LoadStakingHistory => {
                // Manual load request (clear if needed and start loading)
                if !self.loading_history {
                    self.staking_history.clear();
                    self.loading_history = true;
                    if let Some(account) = &self.watched_account {
                        self.history_loaded_for = Some(account.to_string());
                    }
                }
                // Actual loading is handled in main.rs
            }
            Action::CancelLoadingHistory => {
                self.loading_history = false;
            }
            Action::HistoryLoadingComplete => {
                self.loading_history = false;
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
            View::Account if self.account_panel_focus == 1 => {
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
                PoolSortField::Points => a.points.cmp(&b.points),
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
            View::Account if self.account_panel_focus == 1 => {
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
