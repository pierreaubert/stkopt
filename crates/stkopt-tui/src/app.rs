//! Application state and logic.

use crate::action::{AccountStatus, Action, DisplayPool, DisplayValidator, StakingHistoryPoint};
use crate::log_buffer::LogBuffer;
use crate::theme::{Palette, Theme};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::TableState;
use stkopt_core::{ConnectionStatus, Network, OptimizationResult};
use std::collections::HashSet;
use subxt::utils::AccountId32;

/// Input mode for the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    #[default]
    Normal,
    /// Entering account address.
    EnteringAccount,
}

/// Current view/tab in the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Validators,
    Pools,
    Nominate,
    Account,
    History,
}

impl View {
    pub fn all() -> &'static [View] {
        &[View::Validators, View::Pools, View::Nominate, View::Account, View::History]
    }

    pub fn label(&self) -> &'static str {
        match self {
            View::Validators => "Validators",
            View::Pools => "Pools",
            View::Nominate => "Nominate",
            View::Account => "Account",
            View::History => "History",
        }
    }

    pub fn index(&self) -> usize {
        match self {
            View::Validators => 0,
            View::Pools => 1,
            View::Nominate => 2,
            View::Account => 3,
            View::History => 4,
        }
    }

    pub fn from_index(index: usize) -> View {
        match index {
            0 => View::Validators,
            1 => View::Pools,
            2 => View::Nominate,
            3 => View::Account,
            4 => View::History,
            _ => View::Validators,
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
    /// Current QR frame for animated multipart display.
    pub qr_frame: usize,
    /// Whether showing QR code modal.
    pub showing_qr: bool,
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
            qr_frame: 0,
            showing_qr: false,
            showing_help: false,
            should_quit: false,
            tick_count: 0,
            log_buffer,
            log_scroll: 0,
            staking_history: Vec::new(),
            loading_history: false,
            history_total_eras: 30,
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

        // Advance QR animation frame every ~500ms (5 ticks at 10fps)
        if self.showing_qr && self.tick_count.is_multiple_of(5) {
            self.qr_frame = self.qr_frame.wrapping_add(1);
        }
    }

    /// Handle keyboard input.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        match self.input_mode {
            InputMode::Normal => self.handle_normal_key(key),
            InputMode::EnteringAccount => self.handle_input_key(key),
        }
    }

    /// Handle keyboard input in normal mode.
    fn handle_normal_key(&mut self, key: KeyEvent) -> Option<Action> {
        // Close QR modal if showing
        if self.showing_qr {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => {
                    self.showing_qr = false;
                    self.qr_data = None;
                    self.qr_frame = 0;
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
            KeyCode::Tab => self.next_view(),
            KeyCode::BackTab => self.prev_view(),
            KeyCode::Char('1') => self.current_view = View::Validators,
            KeyCode::Char('2') => self.current_view = View::Pools,
            KeyCode::Char('3') => self.current_view = View::Nominate,
            KeyCode::Char('4') => self.current_view = View::Account,
            KeyCode::Char('5') => self.current_view = View::History,
            KeyCode::Char('n') => self.next_network(),
            KeyCode::Up | KeyCode::Char('k') => self.select_previous(),
            KeyCode::Down | KeyCode::Char('j') => self.select_next(),
            KeyCode::Char('a') if self.current_view == View::Account => {
                self.input_mode = InputMode::EnteringAccount;
                self.account_input.clear();
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

    /// Handle keyboard input when entering text.
    fn handle_input_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Enter => {
                self.input_mode = InputMode::Normal;
                // Try to parse the account address
                if let Some(account) = self.parse_account_input() {
                    return Some(Action::SetWatchedAccount(account));
                }
            }
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.account_input.clear();
            }
            KeyCode::Backspace => {
                self.account_input.pop();
            }
            KeyCode::Char(c) => {
                self.account_input.push(c);
            }
            _ => {}
        }
        None
    }

    /// Parse the account input as SS58 address.
    fn parse_account_input(&self) -> Option<AccountId32> {
        // Try to parse as SS58 address
        use std::str::FromStr;
        AccountId32::from_str(&self.account_input).ok()
    }

    /// Switch to next view.
    fn next_view(&mut self) {
        let views = View::all();
        let current_idx = self.current_view.index();
        let next_idx = (current_idx + 1) % views.len();
        self.current_view = View::from_index(next_idx);
    }

    /// Switch to previous view.
    fn prev_view(&mut self) {
        let views = View::all();
        let current_idx = self.current_view.index();
        let prev_idx = if current_idx == 0 {
            views.len() - 1
        } else {
            current_idx - 1
        };
        self.current_view = View::from_index(prev_idx);
    }

    /// Switch to next network.
    fn next_network(&mut self) {
        let networks = Network::all();
        let current_idx = networks.iter().position(|n| *n == self.network).unwrap_or(0);
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
            Action::SetLoadingProgress(progress) => {
                self.loading_progress = progress;
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
            Action::RunOptimization => {
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
            Action::SetQRData(data) => {
                self.qr_data = data;
                self.qr_frame = 0; // Reset animation frame for new QR
                self.showing_qr = self.qr_data.is_some();
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
                // Clear previous history and start loading
                self.staking_history.clear();
                self.loading_history = true;
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
            Action::Quit => {
                self.should_quit = true;
            }
        }
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
            _ => {}
        }
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
            _ => {}
        }
    }
}
