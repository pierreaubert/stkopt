//! Main application state and view for the Staking Optimizer desktop app.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::prelude::*;
use gpui::*;
use gpui_ui_kit::theme::ThemeExt;
use gpui_ui_kit::*;

use crate::views::{
    AccountSection, DashboardSection, HelpOverlay, HistorySection, LogsView, OptimizationSection,
    PoolsSection, SettingsSection, ValidatorsSection,
};

const LOG_PANE_DEFAULT_HEIGHT: f32 = 180.0;
const LOG_PANE_MIN_HEIGHT: f32 = 120.0;
const LOG_PANE_MIN_APP_HEIGHT: f32 = 220.0;

pub(crate) fn clamp_log_pane_height(height: f32, viewport_height: f32) -> f32 {
    let max_height = (viewport_height - LOG_PANE_MIN_APP_HEIGHT).max(LOG_PANE_MIN_HEIGHT);
    height.clamp(LOG_PANE_MIN_HEIGHT, max_height)
}

pub(crate) fn progress_steps_complete(
    status: ConnectionStatus,
    steps: &[(&'static str, bool)],
) -> bool {
    status == ConnectionStatus::Connected && steps.iter().all(|(_, done)| *done)
}

pub(crate) fn parse_token_amount(input: &str, decimals: u8) -> Result<u128, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Enter an amount".to_string());
    }
    if trimmed.starts_with('-') {
        return Err("Amount must be greater than 0".to_string());
    }

    let normalized = trimmed.replace(',', ".");
    let mut parts = normalized.split('.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next();
    if parts.next().is_some() {
        return Err("Invalid amount format".to_string());
    }

    if whole.is_empty() && fraction.unwrap_or_default().is_empty() {
        return Err("Invalid amount format".to_string());
    }

    if !whole.chars().all(|c| c.is_ascii_digit())
        || !fraction
            .unwrap_or_default()
            .chars()
            .all(|c| c.is_ascii_digit())
    {
        return Err("Invalid amount format".to_string());
    }

    let whole_value = if whole.is_empty() {
        0
    } else {
        whole
            .parse::<u128>()
            .map_err(|_| "Amount is too large".to_string())?
    };

    let fraction = fraction.unwrap_or_default();
    if fraction.len() > decimals as usize {
        return Err(format!(
            "Amount supports at most {} decimal places",
            decimals
        ));
    }

    let divisor = 10u128.pow(decimals as u32);
    let fraction_value = if fraction.is_empty() {
        0
    } else {
        let parsed = fraction
            .parse::<u128>()
            .map_err(|_| "Amount is too large".to_string())?;
        parsed * 10u128.pow(decimals as u32 - fraction.len() as u32)
    };

    let amount = whole_value
        .checked_mul(divisor)
        .and_then(|whole| whole.checked_add(fraction_value))
        .ok_or_else(|| "Amount is too large".to_string())?;

    if amount == 0 {
        return Err("Amount must be greater than 0".to_string());
    }

    Ok(amount)
}

/// Navigation sections in the app
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Section {
    #[default]
    Dashboard,
    Account,
    Validators,
    Optimization,
    Pools,
    History,
}

impl Section {
    pub fn all() -> &'static [Section] {
        &[
            Section::Account,
            Section::Dashboard,
            Section::Validators,
            Section::Optimization,
            Section::Pools,
            Section::History,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Section::Dashboard => "Dashboard",
            Section::Account => "Account",
            Section::Validators => "Validators",
            Section::Optimization => "Optimization",
            Section::Pools => "Pools",
            Section::History => "History",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Section::Dashboard => "📊",
            Section::Account => "👤",
            Section::Validators => "✓",
            Section::Optimization => "⚡",
            Section::Pools => "🏊",
            Section::History => "📈",
        }
    }
}

/// Main application state
#[allow(dead_code)]
pub struct StkoptApp {
    /// Current navigation section
    pub current_section: Section,
    /// Entity handle for self-updates
    pub entity: Entity<Self>,
    /// Focus handle for keyboard input
    pub focus_handle: FocusHandle,
    /// Account address input
    pub account_input: String,
    /// Currently watched account address
    pub watched_account: Option<String>,
    /// Account validation error message
    pub account_error: Option<String>,
    /// Connection status
    pub connection_status: ConnectionStatus,
    /// Connection mode (RPC or Light Client)
    pub connection_mode: ConnectionMode,
    /// Selected network
    pub network: Network,
    /// Staking info for the watched account
    pub staking_info: Option<StakingInfo>,
    /// Whether watched account data is currently loading
    pub account_loading: bool,
    /// List of validators
    pub validators: Vec<ValidatorInfo>,
    /// Selected validators for nomination
    pub selected_validators: Vec<usize>,
    /// Validator sort column
    pub validator_sort: crate::actions::ValidatorSortColumn,
    /// Validator sort ascending
    pub validator_sort_asc: bool,
    /// Validator search query
    pub validator_search: String,
    /// Whether validators are currently loading
    pub validators_loading: bool,
    /// Staking history data points
    pub staking_history: Vec<HistoryPoint>,
    /// Whether history is currently loading
    pub history_loading: bool,
    /// Available nomination pools
    pub pools: Vec<PoolInfo>,
    /// Whether pools are currently loading
    pub pools_loading: bool,
    /// Whether connection/sync operations are in progress
    pub operations_loading: bool,
    /// Whether settings panel is visible
    pub show_settings: bool,
    /// Settings: theme preference
    pub settings_theme: crate::persistence::ThemeConfig,
    /// Settings: default network
    pub settings_network: crate::persistence::NetworkConfig,
    /// Settings: connection mode
    pub settings_connection_mode: crate::persistence::ConnectionModeConfig,
    /// Settings: auto-connect on startup
    pub settings_auto_connect: bool,
    /// Settings: show testnet networks
    pub settings_show_testnets: bool,
    /// Whether help overlay is visible
    pub show_help: bool,
    /// Optimization result (estimated avg APY)
    pub optimization_result: Option<f64>,
    /// Selected optimization strategy
    pub optimization_strategy: crate::optimization::SelectionStrategy,
    /// Optimization max commission (0.0 - 1.0)
    pub optimization_max_commission: f64,
    /// Optimization target validator count
    pub optimization_target_count: usize,
    /// Chain handle for async operations
    pub chain_handle: Option<crate::chain::ChainHandle>,
    /// Connection error message
    pub connection_error: Option<String>,
    /// Pending chain updates (shared with async tasks)
    pub pending_updates: Arc<Mutex<Vec<crate::chain::ChainUpdate>>>,
    /// Database service for caching
    pub db: crate::db_service::DbService,
    /// Shared log buffer (LogBuffer is already Arc<Mutex<...>> internally)
    pub log_buffer: crate::log::LogBuffer,
    /// Whether log window is visible
    pub show_logs: bool,
    /// Minimum log level to display in the log pane.
    pub log_level_filter: crate::log::LogLevel,
    /// Current bottom log pane height in pixels.
    pub log_pane_height: f32,
    /// Whether the bottom log pane divider is currently being dragged.
    pub log_pane_dragging: bool,
    /// Mouse Y position when log pane divider dragging started.
    pub log_pane_drag_start_y: f32,
    /// Log pane height when divider dragging started.
    pub log_pane_drag_start_height: f32,
    /// Saved accounts (address book)
    pub address_book: Vec<SavedAccount>,
    /// Whether staking modal is visible
    pub show_staking_modal: bool,
    /// Current staking operation type
    pub staking_operation: StakingOperation,
    /// Amount input for staking operations
    pub staking_amount_input: String,
    /// Inline status/error for the staking operation modal
    pub staking_action_message: Option<String>,
    /// Whether the staking operation modal is building a QR payload
    pub staking_action_generating: bool,
    /// Pending transaction payload (for QR display/signing)
    pub pending_tx_payload: Option<crate::chain::TransactionPayload>,
    /// Whether QR modal is visible
    pub show_qr_modal: bool,
    /// Current tab in QR modal
    pub qr_modal_tab: QrModalTab,
    /// Transaction submission status message
    pub tx_status_message: Option<String>,
    /// Transaction submission state for the QR signing flow
    pub tx_status: QrTxStatus,
    /// Whether pool operations modal is visible
    pub show_pool_modal: bool,
    /// Current pool operation type
    pub pool_operation: PoolOperation,
    /// Selected pool ID for operation
    pub selected_pool_id: Option<u32>,
    /// Amount input for pool operations
    pub pool_amount_input: String,
    /// Inline status/error for the pool operation modal
    pub pool_action_message: Option<String>,
    /// Whether the pool operation modal is building a QR payload
    pub pool_action_generating: bool,
    /// Active QR reader (camera)
    pub qr_reader: Option<crate::qr_reader::QrReader>,
    /// Latest camera preview frame
    pub camera_preview: Option<crate::qr_reader::CameraPreview>,
    /// Scanned signature data (from Vault QR)
    pub scanned_signature: Option<Vec<u8>>,
    /// Signed extrinsic built from the scanned Vault signature
    pub signed_extrinsic: Option<Vec<u8>>,
    /// Whether a signed extrinsic submission is in progress
    pub tx_submitting: bool,
    /// Reward destination for SetPayee operation
    pub rewards_destination: stkopt_chain::RewardDestination,
    /// Whether to show blocked validators in the validators view
    pub show_blocked: bool,
    /// Current viewport width in pixels (updated each render)
    pub viewport_width: f32,
    /// Current viewport height in pixels (updated each render)
    pub viewport_height: f32,
}

/// Type of pool operation being performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PoolOperation {
    #[default]
    Join,
    BondExtra,
    ClaimPayout,
    Unbond,
    Withdraw,
}

impl PoolOperation {
    pub fn label(&self) -> &'static str {
        match self {
            PoolOperation::Join => "Join Pool",
            PoolOperation::BondExtra => "Bond Extra",
            PoolOperation::ClaimPayout => "Claim Payout",
            PoolOperation::Unbond => "Unbond",
            PoolOperation::Withdraw => "Withdraw",
        }
    }

    pub fn requires_amount(&self) -> bool {
        match self {
            PoolOperation::Join | PoolOperation::BondExtra | PoolOperation::Unbond => true,
            PoolOperation::ClaimPayout | PoolOperation::Withdraw => false,
        }
    }
}

/// Type of staking operation being performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StakingOperation {
    #[default]
    Bond,
    Unbond,
    BondExtra,
    Rebond,
    WithdrawUnbonded,
    Nominate,
    Chill,
    ClaimRewards,
    SetPayee,
}

impl StakingOperation {
    pub fn label(&self) -> &'static str {
        match self {
            StakingOperation::Bond => "Bond",
            StakingOperation::Unbond => "Unbond",
            StakingOperation::BondExtra => "Bond Extra",
            StakingOperation::Rebond => "Rebond",
            StakingOperation::WithdrawUnbonded => "Withdraw",
            StakingOperation::Nominate => "Nominate",
            StakingOperation::Chill => "Stop Nominating",
            StakingOperation::ClaimRewards => "Claim Rewards",
            StakingOperation::SetPayee => "Set Payee",
        }
    }

    pub fn requires_amount(&self) -> bool {
        match self {
            StakingOperation::Bond
            | StakingOperation::Unbond
            | StakingOperation::BondExtra
            | StakingOperation::Rebond => true,
            StakingOperation::WithdrawUnbonded
            | StakingOperation::Nominate
            | StakingOperation::Chill
            | StakingOperation::ClaimRewards
            | StakingOperation::SetPayee => false,
        }
    }
}

/// Tabs in the QR modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QrModalTab {
    #[default]
    QrCode,
    ScanSignature,
    Submit,
}

impl QrModalTab {
    pub fn all() -> &'static [QrModalTab] {
        &[
            QrModalTab::QrCode,
            QrModalTab::ScanSignature,
            QrModalTab::Submit,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            QrModalTab::QrCode => "QR Code",
            QrModalTab::ScanSignature => "Scan Signature",
            QrModalTab::Submit => "Submit",
        }
    }
}

/// Submission state for a scanned signed transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QrTxStatus {
    #[default]
    NotReady,
    Ready,
    Submitting,
    Submitted,
    Failed,
}

/// A saved account in the address book.
#[derive(Debug, Clone)]
pub struct SavedAccount {
    /// Account address
    pub address: String,
    /// Optional label/name for the account
    pub label: Option<String>,
    /// Network this account is for
    pub network: Network,
}

/// Connection status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(dead_code)]
pub enum ConnectionStatus {
    #[default]
    Disconnected,
    Connecting,
    Connected,
}

/// Connection mode - how to connect to the network
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnectionMode {
    /// Connect via RPC endpoint
    #[default]
    Rpc,
    /// Connect via embedded light client (smoldot)
    LightClient,
}

impl ConnectionMode {
    pub fn from_config(config: crate::persistence::ConnectionModeConfig) -> Self {
        match config {
            crate::persistence::ConnectionModeConfig::Rpc => ConnectionMode::Rpc,
            crate::persistence::ConnectionModeConfig::LightClient => ConnectionMode::LightClient,
        }
    }

    pub fn to_config(self) -> crate::persistence::ConnectionModeConfig {
        match self {
            ConnectionMode::Rpc => crate::persistence::ConnectionModeConfig::Rpc,
            ConnectionMode::LightClient => crate::persistence::ConnectionModeConfig::LightClient,
        }
    }

    pub fn uses_light_client(self) -> bool {
        self == ConnectionMode::LightClient
    }

    pub fn label(&self) -> &'static str {
        match self {
            ConnectionMode::Rpc => "RPC",
            ConnectionMode::LightClient => "Light Client",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            ConnectionMode::Rpc => "Connect via RPC endpoint (faster, requires trust)",
            ConnectionMode::LightClient => "Embedded light client (trustless, slower startup)",
        }
    }
}

/// Supported networks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(dead_code)]
pub enum Network {
    #[default]
    Polkadot,
    Kusama,
    Westend,
    Paseo,
}

impl Network {
    pub fn label(&self) -> &'static str {
        match self {
            Network::Polkadot => "Polkadot",
            Network::Kusama => "Kusama",
            Network::Westend => "Westend",
            Network::Paseo => "Paseo",
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            Network::Polkadot => "DOT",
            Network::Kusama => "KSM",
            Network::Westend => "WND",
            Network::Paseo => "PAS",
        }
    }

    pub fn token_decimals(&self) -> u8 {
        match self {
            Network::Polkadot => 10,
            Network::Kusama => 12,
            Network::Westend => 12,
            Network::Paseo => 10,
        }
    }

    /// Convert to stkopt_core::Network
    pub fn to_core(&self) -> stkopt_core::Network {
        match self {
            Network::Polkadot => stkopt_core::Network::Polkadot,
            Network::Kusama => stkopt_core::Network::Kusama,
            Network::Westend => stkopt_core::Network::Westend,
            Network::Paseo => stkopt_core::Network::Paseo,
        }
    }
}

/// Type alias for staking information (from stkopt-core).
pub type StakingInfo = stkopt_core::display::StakingInfo;

/// Type alias for validator information (from stkopt-core).
pub type ValidatorInfo = stkopt_core::display::DisplayValidator;

/// Type alias for historical staking data point (from stkopt-core).
pub type HistoryPoint = stkopt_core::display::StakingHistoryPoint;

/// Type alias for nomination pool information (from stkopt-core).
pub type PoolInfo = stkopt_core::display::DisplayPool;

/// Type alias for pool state (from stkopt-core).
pub type PoolState = stkopt_core::PoolState;

/// Generate mock pools for demo/testing
pub fn generate_mock_pools(count: usize) -> Vec<PoolInfo> {
    let pool_names = [
        "Polkadot Community Pool",
        "Kusama Validators",
        "Web3 Foundation",
        "Parity Pool",
        "Decentralized Staking",
        "Community Validators",
        "Stake Together",
        "DOT Maximizers",
        "Secure Staking",
        "Validator Alliance",
    ];

    (0..count)
        .map(|i| {
            let name = pool_names[i % pool_names.len()];
            PoolInfo {
                id: (i + 1) as u32,
                name: format!("{} #{}", name, i + 1),
                state: if i % 10 == 9 {
                    PoolState::Blocked
                } else {
                    PoolState::Open
                },
                member_count: 50 + (i * 17 % 500) as u32,
                total_bonded: 100_000_000_000_000u128 + (i as u128 * 50_000_000_000_000),
                commission: if i % 3 == 0 {
                    Some(5.0 + (i % 10) as f64)
                } else {
                    None
                },
                apy: Some(12.0 + (i % 8) as f64 * 0.5), // Mock APY
            }
        })
        .collect()
}

impl StkoptApp {
    pub fn new(cx: &mut Context<Self>, log_buffer: crate::log::LogBuffer) -> Self {
        // Load saved config from disk
        let config = crate::persistence::load_config().unwrap_or_default();

        // Convert network config to app network
        let network = match config.network {
            crate::persistence::NetworkConfig::Polkadot => Network::Polkadot,
            crate::persistence::NetworkConfig::Kusama => Network::Kusama,
            crate::persistence::NetworkConfig::Westend => Network::Westend,
            crate::persistence::NetworkConfig::Paseo => Network::Paseo,
            crate::persistence::NetworkConfig::Custom => Network::Polkadot,
        };

        let connection_mode = ConnectionMode::from_config(config.connection_mode);

        // Initialize database and chain worker
        let handle = crate::gpui_tokio::Tokio::handle(cx);
        let db = crate::db_service::DbService::new(handle.clone())
            .expect("Failed to initialize database");
        let (chain_handle, mut update_rx) =
            crate::chain::spawn_chain_worker(Some(db.clone()), handle);

        // Spawn chain update listener
        // We initialize pending_updates early to share with the listener
        let pending_updates = Arc::new(Mutex::new(Vec::new()));
        let listener_updates = pending_updates.clone();

        let mut async_cx = cx.to_async();
        cx.spawn(
            move |this: WeakEntity<Self>, _cx: &mut gpui::AsyncApp| async move {
                while let Some(update) = update_rx.recv().await {
                    if let Ok(mut queue) = listener_updates.lock() {
                        queue.push(update);
                    }
                    // Notify the view to process updates in the next frame
                    // We use process_pending_updates in render() to pick these up
                    if let Err(e) =
                        this.update(&mut async_cx, |_, cx: &mut Context<Self>| cx.notify())
                    {
                        tracing::warn!("Failed to notify UI of chain update: {:?}", e);
                    }
                }
            },
        )
        .detach();

        // Auto-connect if enabled
        if config.auto_connect {
            let handle = chain_handle.clone();
            let net = network; // Copy
            let use_light = connection_mode.uses_light_client();
            cx.spawn(move |_, _cx: &mut gpui::AsyncApp| async move {
                if let Err(e) = handle.connect(net.to_core(), use_light).await {
                    eprintln!("Failed to auto-connect: {}", e);
                }
            })
            .detach();
        }

        let mut instance = Self {
            // Start on Account tab since no address is set yet
            current_section: Section::Account,
            entity: cx.entity().clone(),
            focus_handle: cx.focus_handle(),
            account_input: config.last_account.clone().unwrap_or_default(),
            watched_account: config.last_account,
            account_error: None,
            connection_status: ConnectionStatus::Disconnected,
            connection_mode,
            network,
            staking_info: None,
            account_loading: false,
            validators: Vec::new(),
            selected_validators: Vec::new(),
            validator_sort: crate::actions::ValidatorSortColumn::default(),
            validator_sort_asc: false,
            validator_search: String::new(),
            validators_loading: false,
            staking_history: Vec::new(),
            history_loading: false,
            pools: Vec::new(),
            pools_loading: false,
            operations_loading: false,
            show_settings: false,
            settings_theme: config.theme,
            settings_network: config.network,
            settings_connection_mode: config.connection_mode,
            settings_auto_connect: config.auto_connect,
            settings_show_testnets: config.show_testnets,
            show_help: false,
            optimization_result: None,
            optimization_strategy: crate::optimization::SelectionStrategy::default(),
            optimization_max_commission: 0.15,
            optimization_target_count: 16,
            chain_handle: Some(chain_handle),
            connection_error: None,
            pending_updates,
            db,
            log_buffer,
            show_logs: true,
            log_level_filter: crate::log::LogLevel::Trace,
            log_pane_height: LOG_PANE_DEFAULT_HEIGHT,
            log_pane_dragging: false,
            log_pane_drag_start_y: 0.0,
            log_pane_drag_start_height: LOG_PANE_DEFAULT_HEIGHT,
            address_book: Vec::new(),
            show_staking_modal: false,
            staking_operation: StakingOperation::default(),
            staking_amount_input: String::new(),
            staking_action_message: None,
            staking_action_generating: false,
            pending_tx_payload: None,
            show_qr_modal: false,
            qr_modal_tab: QrModalTab::default(),
            tx_status_message: None,
            tx_status: QrTxStatus::NotReady,
            show_pool_modal: false,
            pool_operation: PoolOperation::default(),
            selected_pool_id: None,
            pool_amount_input: String::new(),
            pool_action_message: None,
            pool_action_generating: false,
            qr_reader: None,
            camera_preview: None,
            scanned_signature: None,
            signed_extrinsic: None,
            tx_submitting: false,
            rewards_destination: stkopt_chain::RewardDestination::Staked,
            show_blocked: true,
            viewport_width: 1400.0,
            viewport_height: 900.0,
        };

        // Load address book from disk
        if let Ok(book) = crate::persistence::load_address_book() {
            let net_config = crate::persistence::NetworkConfig::from(instance.network.to_core());
            for entry in book.for_network(net_config) {
                instance.address_book.push(SavedAccount {
                    address: entry.address.clone(),
                    label: Some(entry.label.clone()),
                    network: instance.network,
                });
            }
        }

        instance.load_cache(cx);
        instance
    }

    /// Load cached data from the database.
    pub fn load_cache(&self, cx: &mut Context<Self>) {
        let db = self.db.clone();
        let network = self.network.to_core();
        let pending_updates = self.pending_updates.clone();

        let mut async_cx = cx.to_async();
        cx.spawn(
            move |this: WeakEntity<Self>, _cx: &mut gpui::AsyncApp| async move {
                let validators_res = db.get_cached_validators(network).await;
                let pools_res = db.get_cached_pools(network).await;

                let mut updates = Vec::new();
                if let Ok(validators) = validators_res
                    && !validators.is_empty()
                {
                    updates.push(crate::chain::ChainUpdate::ValidatorsLoaded(validators));
                }
                if let Ok(pools) = pools_res
                    && !pools.is_empty()
                {
                    updates.push(crate::chain::ChainUpdate::PoolsLoaded(pools));
                }

                if !updates.is_empty() {
                    if let Ok(mut queue) = pending_updates.lock() {
                        queue.extend(updates);
                    }
                    // Notify the view to process updates
                    if let Err(e) =
                        this.update(&mut async_cx, |_, cx: &mut Context<Self>| cx.notify())
                    {
                        tracing::warn!("Failed to notify UI of DB update: {:?}", e);
                    }
                }
            },
        )
        .detach();
    }

    /// Process any pending chain updates. Called during render.
    pub fn process_pending_updates(&mut self, cx: &mut Context<Self>) {
        let updates: Vec<crate::chain::ChainUpdate> = {
            match self.pending_updates.lock() {
                Ok(mut pending) => std::mem::take(&mut *pending),
                Err(poisoned) => {
                    // Mutex was poisoned (a thread panicked while holding it)
                    // Recover by taking the data anyway
                    tracing::warn!("pending_updates mutex was poisoned, recovering");
                    std::mem::take(&mut *poisoned.into_inner())
                }
            }
        };

        for update in updates {
            self.apply_chain_update(update, cx);
        }
    }

    /// Apply a single chain update to the app state.
    pub fn apply_chain_update(
        &mut self,
        update: crate::chain::ChainUpdate,
        cx: &mut Context<Self>,
    ) {
        use crate::chain::ChainUpdate;

        match update {
            ChainUpdate::ConnectionStatus(status) => {
                self.connection_status = match status {
                    stkopt_core::ConnectionStatus::Disconnected => {
                        self.operations_loading = false;
                        self.validators_loading = false;
                        self.pools_loading = false;
                        self.account_loading = false;
                        self.history_loading = false;
                        ConnectionStatus::Disconnected
                    }
                    stkopt_core::ConnectionStatus::Connecting => {
                        self.operations_loading = true;
                        self.validators_loading = true;
                        self.pools_loading = true;
                        self.account_loading = false;
                        self.history_loading = false;
                        ConnectionStatus::Connecting
                    }
                    stkopt_core::ConnectionStatus::Connected => {
                        self.operations_loading = false;
                        if self.watched_account.is_none() {
                            self.history_loading = false;
                        }
                        ConnectionStatus::Connected
                    }
                    stkopt_core::ConnectionStatus::Syncing { .. } => {
                        self.operations_loading = true;
                        self.validators_loading = true;
                        self.pools_loading = true;
                        ConnectionStatus::Connecting
                    }
                    stkopt_core::ConnectionStatus::Error(e) => {
                        self.connection_error = Some(e);
                        self.operations_loading = false;
                        self.validators_loading = false;
                        self.pools_loading = false;
                        self.account_loading = false;
                        self.history_loading = false;
                        ConnectionStatus::Disconnected
                    }
                };
                if self.connection_status == ConnectionStatus::Connected
                    && self.watched_account.is_some()
                    && self.staking_info.is_none()
                    && !self.account_loading
                {
                    self.fetch_watched_account(cx);
                }
            }
            ChainUpdate::ValidatorsLoaded(validators) => {
                self.validators = validators;
                self.validators_loading = false;
                tracing::info!("Loaded {} validators from chain", self.validators.len());
            }
            ChainUpdate::PoolsLoaded(pools) => {
                self.pools = pools;
                self.pools_loading = false;
                tracing::info!("Loaded {} pools from chain", self.pools.len());
            }
            ChainUpdate::AccountLoaded(account_data) => {
                self.account_loading = false;
                // Update staking info from account data
                self.staking_info = Some(StakingInfo {
                    total_balance: account_data.free_balance + account_data.reserved_balance,
                    transferable: account_data.free_balance,
                    bonded: account_data.staked_balance.unwrap_or(0),
                    unbonding: account_data.unbonding_balance,
                    rewards_pending: account_data.pool_pending_rewards,
                    is_nominating: account_data.is_nominating,
                    nomination_count: account_data.nominations.len(),
                });
                tracing::info!(
                    "Account data loaded: balance={}, unbonding={}, pool_pending_rewards={}",
                    account_data.free_balance,
                    account_data.unbonding_balance,
                    account_data.pool_pending_rewards
                );
                // Auto-load history if empty
                if self.staking_history.is_empty() && !self.history_loading {
                    self.load_history(cx);
                }
            }
            ChainUpdate::HistoryLoaded(history) => {
                self.staking_history = history;
                self.history_loading = false;
            }
            ChainUpdate::QrPayloadGenerated(payload) => {
                // Store the generated QR payload for display in QR modal
                tracing::info!("QR payload generated: {}", payload.description);
                self.clear_qr_signature_state();
                self.pending_tx_payload = Some(payload);
                self.show_staking_modal = false;
                self.show_pool_modal = false;
                self.show_qr_modal = true;
                self.qr_modal_tab = QrModalTab::QrCode;
            }
            ChainUpdate::TxSubmissionUpdate(result) => {
                // Handle transaction submission result
                use crate::chain::TxSubmissionResult;
                match result {
                    TxSubmissionResult::InBlock { block_hash } => {
                        tracing::info!("Transaction in block: 0x{}", hex::encode(block_hash));
                    }
                    TxSubmissionResult::Finalized { block_hash } => {
                        tracing::info!(
                            "Transaction finalized in block: 0x{}",
                            hex::encode(block_hash)
                        );
                    }
                    TxSubmissionResult::Dropped(reason) => {
                        tracing::warn!("Transaction dropped: {}", reason);
                        self.connection_error = Some(format!("Transaction dropped: {}", reason));
                    }
                }
            }
            ChainUpdate::Error(e) => {
                tracing::error!("Chain error: {}", e);
                self.connection_error = Some(e);
                self.operations_loading = false;
                self.validators_loading = false;
                self.pools_loading = false;
                self.account_loading = false;
                self.history_loading = false;
            }
        }
        cx.notify();
    }

    /// Save current settings to disk.
    pub fn save_config(&self) {
        let config = crate::persistence::AppConfig {
            last_account: self.watched_account.clone(),
            network: self.settings_network,
            connection_mode: self.settings_connection_mode,
            custom_rpc: None,
            theme: self.settings_theme,
            auto_connect: self.settings_auto_connect,
            show_testnets: self.settings_show_testnets,
            accounts: Vec::new(), // Legacy TUI format, not used in GPUI
        };

        if let Err(e) = crate::persistence::save_config(&config) {
            eprintln!("Failed to save config: {}", e);
        }
    }

    /// Change the active connection mode and reconnect if the app was connected.
    pub fn set_connection_mode(&mut self, mode: ConnectionMode, cx: &mut Context<Self>) {
        if self.connection_mode == mode && self.settings_connection_mode == mode.to_config() {
            return;
        }

        let should_reconnect = matches!(
            self.connection_status,
            ConnectionStatus::Connected | ConnectionStatus::Connecting
        ) || self.settings_auto_connect;

        self.connection_mode = mode;
        self.settings_connection_mode = mode.to_config();
        self.connection_error = None;
        self.save_config();

        if should_reconnect {
            if let Some(ref handle) = self.chain_handle {
                let handle = handle.clone();
                let network = self.network.to_core();
                let use_light_client = mode.uses_light_client();

                self.connection_status = ConnectionStatus::Connecting;
                self.operations_loading = true;
                self.validators_loading = true;
                self.pools_loading = true;
                self.account_loading = false;
                self.history_loading = false;
                self.validators.clear();
                self.pools.clear();
                self.staking_info = None;
                self.staking_history.clear();

                cx.spawn(
                    move |_: gpui::WeakEntity<StkoptApp>, _: &mut gpui::AsyncApp| async move {
                        let _ = handle.disconnect().await;
                        let _ = handle.connect(network, use_light_client).await;
                    },
                )
                .detach();
            } else {
                self.connection_status = ConnectionStatus::Disconnected;
                self.operations_loading = false;
                self.validators_loading = false;
                self.pools_loading = false;
                self.account_loading = false;
                self.history_loading = false;
                self.connection_error = Some("Connection worker is not available".to_string());
            }
        }

        cx.notify();
    }

    pub fn set_logs_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.show_logs = visible;
        self.log_pane_dragging = false;
        self.log_pane_height = clamp_log_pane_height(self.log_pane_height, self.viewport_height);
        cx.notify();
    }

    pub fn set_log_level_filter(&mut self, level: crate::log::LogLevel, cx: &mut Context<Self>) {
        self.log_level_filter = level;
        cx.notify();
    }

    fn begin_log_pane_drag(&mut self, y: f32, cx: &mut Context<Self>) {
        if !self.show_logs {
            return;
        }

        self.log_pane_dragging = true;
        self.log_pane_drag_start_y = y;
        self.log_pane_drag_start_height = self.log_pane_height;
        cx.notify();
    }

    fn update_log_pane_drag(&mut self, y: f32, cx: &mut Context<Self>) {
        if !self.log_pane_dragging {
            return;
        }

        let delta = y - self.log_pane_drag_start_y;
        self.log_pane_height = clamp_log_pane_height(
            self.log_pane_drag_start_height - delta,
            self.viewport_height,
        );
        cx.notify();
    }

    fn end_log_pane_drag(&mut self, cx: &mut Context<Self>) {
        if self.log_pane_dragging {
            self.log_pane_dragging = false;
            cx.notify();
        }
    }

    /// Add an account to the address book if not already present.
    pub fn add_to_address_book(&mut self, address: String) {
        // Check if already exists
        if self
            .address_book
            .iter()
            .any(|a| a.address == address && a.network == self.network)
        {
            return;
        }
        self.address_book.push(SavedAccount {
            address,
            label: None,
            network: self.network,
        });
        self.persist_address_book();
    }

    /// Remove an account from the address book.
    pub fn remove_from_address_book(&mut self, address: &str) {
        self.address_book
            .retain(|a| a.address != address || a.network != self.network);
        self.persist_address_book();
    }

    /// Persist the address book to disk.
    fn persist_address_book(&self) {
        let net_config = crate::persistence::NetworkConfig::from(self.network.to_core());
        let mut book = crate::persistence::load_address_book().unwrap_or_default();

        // Remove existing entries for this network and re-add from current state
        book.entries.retain(|e| e.network != net_config);

        for saved in &self.address_book {
            if saved.network == self.network {
                let entry = crate::persistence::AddressBookEntry {
                    address: saved.address.clone(),
                    label: saved.label.clone().unwrap_or_default(),
                    network: net_config,
                    notes: None,
                    created_at: 0,
                };
                let _ = book.add(entry);
            }
        }

        if let Err(e) = crate::persistence::save_address_book(&book) {
            tracing::warn!("Failed to persist address book: {}", e);
        }
    }

    /// Get the token decimals for the current network.
    pub fn token_decimals(&self) -> u8 {
        self.network.token_decimals()
    }

    /// Get the decimal divisor (10^decimals) for the current network.
    pub fn decimal_divisor(&self) -> u128 {
        10u128.pow(self.network.token_decimals() as u32)
    }

    /// Get the token symbol for the current network.
    pub fn token_symbol(&self) -> &'static str {
        self.network.symbol()
    }

    /// Whether all initial chain data needed by the UI has finished loading.
    pub fn data_download_complete(&self) -> bool {
        progress_steps_complete(self.connection_status, &self.connection_progress_steps())
    }

    /// Whether transaction-oriented commands can run.
    pub fn commands_available(&self) -> bool {
        self.data_download_complete() && self.watched_account.is_some()
    }

    fn connection_progress_steps(&self) -> [(&'static str, bool); 4] {
        let connected = self.connection_status == ConnectionStatus::Connected;
        [
            ("Operations", connected && !self.operations_loading),
            ("Validators", connected && !self.validators_loading),
            ("Pools", connected && !self.pools_loading),
            ("History", connected && self.account_history_ready()),
        ]
    }

    fn account_history_ready(&self) -> bool {
        if self.watched_account.is_none() {
            return !self.history_loading;
        }

        !self.account_loading && !self.history_loading && self.staking_info.is_some()
    }

    /// Select a watched account and reset stale account-specific data.
    pub fn set_watched_account(&mut self, address: String) {
        self.watched_account = Some(address.clone());
        self.account_input = address;
        self.account_error = None;
        self.staking_info = None;
        self.staking_history.clear();
        self.account_loading = false;
        self.history_loading = false;
    }

    /// Fetch account data for the currently watched account.
    pub fn fetch_watched_account(&mut self, cx: &mut Context<Self>) {
        let Some(ref handle) = self.chain_handle else {
            return;
        };
        let Some(address) = self.watched_account.clone() else {
            return;
        };
        if self.connection_status != ConnectionStatus::Connected || self.account_loading {
            return;
        }

        self.account_loading = true;
        self.history_loading = false;
        cx.notify();

        let handle = handle.clone();
        let entity = self.entity.clone();
        let mut async_cx = cx.to_async();
        cx.spawn(
            move |_this: gpui::WeakEntity<StkoptApp>, _cx: &mut gpui::AsyncApp| async move {
                let result = handle.fetch_account(address).await;
                let _ =
                    entity.update(
                        &mut async_cx,
                        |this, cx: &mut Context<StkoptApp>| match result {
                            Ok(account_data) => {
                                this.apply_chain_update(
                                    crate::chain::ChainUpdate::AccountLoaded(account_data),
                                    cx,
                                );
                            }
                            Err(e) => {
                                tracing::error!("Failed to fetch account: {}", e);
                                this.account_loading = false;
                                this.history_loading = false;
                                this.connection_error =
                                    Some(format!("Failed to fetch account: {}", e));
                                cx.notify();
                            }
                        },
                    );
            },
        )
        .detach();
    }

    /// Open the staking modal with a specific operation.
    pub fn open_staking_modal(&mut self, operation: StakingOperation, cx: &mut Context<Self>) {
        if !self.commands_available() {
            return;
        }
        self.staking_operation = operation;
        self.staking_amount_input.clear();
        self.staking_action_message = None;
        self.staking_action_generating = false;
        self.show_staking_modal = true;
        cx.notify();
    }

    /// Generate QR payload for the current staking operation.
    pub fn generate_staking_qr(&mut self, cx: &mut Context<Self>) {
        if !self.commands_available() {
            self.staking_action_generating = false;
            self.staking_action_message = Some("Wait for all chain data to finish loading".into());
            cx.notify();
            return;
        }

        let Some(ref chain_handle) = self.chain_handle else {
            self.staking_action_generating = false;
            self.staking_action_message = Some("Not connected".to_string());
            cx.notify();
            return;
        };

        let Some(ref address) = self.watched_account else {
            self.staking_action_generating = false;
            self.staking_action_message = Some("No account selected".to_string());
            cx.notify();
            return;
        };

        // Parse the account address
        let signer = match address.parse::<subxt::utils::AccountId32>() {
            Ok(id) => id,
            Err(e) => {
                self.staking_action_generating = false;
                self.staking_action_message = Some(format!("Invalid address: {}", e));
                cx.notify();
                return;
            }
        };

        // Parse amount if needed
        let amount = if self.staking_operation.requires_amount() {
            match parse_token_amount(&self.staking_amount_input, self.token_decimals()) {
                Ok(amount) => amount,
                Err(e) => {
                    self.staking_action_generating = false;
                    self.staking_action_message = Some(e);
                    cx.notify();
                    return;
                }
            }
        } else {
            0
        };

        // For Nominate, redirect to Optimization view
        if self.staking_operation == StakingOperation::Nominate {
            self.staking_action_generating = false;
            self.staking_action_message =
                Some("Use 'Nominate Validators' from Optimization or Validators view".to_string());
            self.show_staking_modal = false;
            cx.notify();
            return;
        }

        // For ClaimRewards, route to pool claim if user has pool rewards
        if self.staking_operation == StakingOperation::ClaimRewards {
            self.claim_rewards(cx);
            return;
        }

        let handle = chain_handle.clone();
        let operation = self.staking_operation;
        let rewards_destination = self.rewards_destination.clone();
        let mut async_cx = cx.to_async();
        self.staking_action_generating = true;
        self.staking_action_message = Some("Generating QR...".to_string());
        cx.notify();

        cx.spawn(
            move |this: gpui::WeakEntity<Self>, _cx: &mut gpui::AsyncApp| async move {
                let result = match operation {
                    StakingOperation::Bond => handle.create_bond_payload(signer, amount).await,
                    StakingOperation::Unbond => handle.create_unbond_payload(signer, amount).await,
                    StakingOperation::BondExtra => {
                        handle.create_bond_extra_payload(signer, amount).await
                    }
                    StakingOperation::Rebond => handle.create_rebond_payload(signer, amount).await,
                    StakingOperation::WithdrawUnbonded => {
                        handle.create_withdraw_unbonded_payload(signer).await
                    }
                    StakingOperation::Chill => handle.create_chill_payload(signer).await,
                    StakingOperation::SetPayee => {
                        handle
                            .create_set_payee_payload(signer, rewards_destination)
                            .await
                    }
                    // Nominate and ClaimRewards are handled above, before the spawn
                    StakingOperation::Nominate | StakingOperation::ClaimRewards => {
                        unreachable!()
                    }
                };

                match result {
                    Ok(payload) => {
                        if let Err(e) =
                            this.update(&mut async_cx, |this, cx: &mut Context<Self>| {
                                this.staking_action_generating = false;
                                this.staking_action_message = None;
                                this.clear_qr_signature_state();
                                this.pending_tx_payload = Some(payload);
                                this.show_staking_modal = false;
                                this.show_qr_modal = true;
                                this.qr_modal_tab = QrModalTab::QrCode;
                                cx.notify();
                            })
                        {
                            tracing::error!("Failed to update UI with staking payload: {:?}", e);
                        }
                    }
                    Err(e) => {
                        if let Err(update_err) =
                            this.update(&mut async_cx, |this, cx: &mut Context<Self>| {
                                this.staking_action_generating = false;
                                this.staking_action_message = Some(e);
                                cx.notify();
                            })
                        {
                            tracing::error!(
                                "Failed to update UI with staking error: {:?}",
                                update_err
                            );
                        }
                    }
                }
            },
        )
        .detach();
    }

    /// Generate a claim-payout QR for pending nomination-pool rewards.
    pub fn claim_rewards(&mut self, cx: &mut Context<Self>) {
        self.show_staking_modal = false;
        self.show_pool_modal = false;
        self.connection_error = None;

        if !self.commands_available() {
            self.connection_error =
                Some("Wait for all chain data to finish loading before claiming rewards.".into());
            cx.notify();
            return;
        }

        if self.chain_handle.is_none() {
            self.connection_error = Some("Connect to a network before claiming rewards.".into());
            cx.notify();
            return;
        }

        if self.watched_account.is_none() {
            self.connection_error = Some("Watch an account before claiming rewards.".into());
            cx.notify();
            return;
        }

        let pending_rewards = self
            .staking_info
            .as_ref()
            .map_or(0, |info| info.rewards_pending);
        if pending_rewards == 0 {
            self.connection_error = Some(
                "No pending pool rewards to claim. Direct staking rewards are paid automatically."
                    .into(),
            );
            cx.notify();
            return;
        }

        self.pool_operation = PoolOperation::ClaimPayout;
        self.selected_pool_id = None;
        self.pool_amount_input.clear();
        self.generate_pool_qr(cx);
    }

    /// Generate QR payload for nominating validators.
    pub fn generate_nominate_qr(&mut self, targets: Vec<String>, cx: &mut Context<Self>) {
        if !self.commands_available() {
            self.connection_error =
                Some("Wait for all chain data to finish loading before nominating.".into());
            cx.notify();
            return;
        }

        let Some(ref chain_handle) = self.chain_handle else {
            self.connection_error = Some("Not connected".to_string());
            cx.notify();
            return;
        };

        let Some(ref address) = self.watched_account else {
            self.connection_error = Some("No account selected".to_string());
            cx.notify();
            return;
        };

        // Parse the signer account address
        let signer = match address.parse::<subxt::utils::AccountId32>() {
            Ok(id) => id,
            Err(e) => {
                self.connection_error = Some(format!("Invalid signer address: {}", e));
                cx.notify();
                return;
            }
        };

        // Parse all target validator addresses
        let mut parsed_targets = Vec::with_capacity(targets.len());
        for target in &targets {
            match target.parse::<subxt::utils::AccountId32>() {
                Ok(id) => parsed_targets.push(id),
                Err(e) => {
                    self.connection_error =
                        Some(format!("Invalid validator address {}: {}", target, e));
                    cx.notify();
                    return;
                }
            }
        }

        if parsed_targets.is_empty() {
            self.connection_error = Some("No validators selected".to_string());
            cx.notify();
            return;
        }

        let handle = chain_handle.clone();
        let mut async_cx = cx.to_async();

        cx.spawn(
            move |this: gpui::WeakEntity<Self>, _cx: &mut gpui::AsyncApp| async move {
                let result = handle.create_nominate_payload(signer, parsed_targets).await;

                match result {
                    Ok(payload) => {
                        if let Err(e) =
                            this.update(&mut async_cx, |this, cx: &mut Context<Self>| {
                                this.clear_qr_signature_state();
                                this.pending_tx_payload = Some(payload);
                                this.show_qr_modal = true;
                                this.qr_modal_tab = QrModalTab::QrCode;
                                cx.notify();
                            })
                        {
                            tracing::error!("Failed to update UI with nominate payload: {:?}", e);
                        }
                    }
                    Err(e) => {
                        if let Err(update_err) =
                            this.update(&mut async_cx, |this, cx: &mut Context<Self>| {
                                this.connection_error = Some(e);
                                cx.notify();
                            })
                        {
                            tracing::error!(
                                "Failed to update UI with nominate error: {:?}",
                                update_err
                            );
                        }
                    }
                }
            },
        )
        .detach();
    }

    /// Open the pool modal with a specific operation.
    pub fn open_pool_modal(
        &mut self,
        operation: PoolOperation,
        pool_id: Option<u32>,
        cx: &mut Context<Self>,
    ) {
        if !self.commands_available() {
            return;
        }
        self.pool_operation = operation;
        self.selected_pool_id = pool_id;
        self.pool_amount_input.clear();
        self.pool_action_message = None;
        self.pool_action_generating = false;
        self.show_pool_modal = true;
        cx.notify();
    }

    /// Generate QR payload for the current pool operation.
    pub fn generate_pool_qr(&mut self, cx: &mut Context<Self>) {
        if !self.commands_available() {
            self.pool_action_generating = false;
            self.pool_action_message = Some("Wait for all chain data to finish loading".into());
            cx.notify();
            return;
        }

        let Some(ref chain_handle) = self.chain_handle else {
            self.pool_action_generating = false;
            self.pool_action_message = Some("Not connected".to_string());
            cx.notify();
            return;
        };

        let Some(ref address) = self.watched_account else {
            self.pool_action_generating = false;
            self.pool_action_message = Some("No account selected".to_string());
            cx.notify();
            return;
        };

        // Parse the account address
        let signer = match address.parse::<subxt::utils::AccountId32>() {
            Ok(id) => id,
            Err(e) => {
                self.pool_action_generating = false;
                self.pool_action_message = Some(format!("Invalid address: {}", e));
                cx.notify();
                return;
            }
        };

        // Parse amount if needed
        let amount = if self.pool_operation.requires_amount() {
            match parse_token_amount(&self.pool_amount_input, self.token_decimals()) {
                Ok(amount) => amount,
                Err(e) => {
                    self.pool_action_generating = false;
                    self.pool_action_message = Some(e);
                    cx.notify();
                    return;
                }
            }
        } else {
            0
        };

        let pool_id = self.selected_pool_id;
        let handle = chain_handle.clone();
        let operation = self.pool_operation;
        let mut async_cx = cx.to_async();
        self.pool_action_generating = true;
        self.pool_action_message = Some("Generating QR...".to_string());
        cx.notify();

        cx.spawn(
            move |this: gpui::WeakEntity<Self>, _cx: &mut gpui::AsyncApp| async move {
                let result = match operation {
                    PoolOperation::Join => {
                        if let Some(id) = pool_id {
                            handle.create_pool_join_payload(signer, id, amount).await
                        } else {
                            Err("No pool selected".to_string())
                        }
                    }
                    PoolOperation::BondExtra => {
                        handle.create_pool_bond_extra_payload(signer, amount).await
                    }
                    PoolOperation::ClaimPayout => handle.create_pool_claim_payload(signer).await,
                    PoolOperation::Unbond => {
                        handle.create_pool_unbond_payload(signer, amount).await
                    }
                    PoolOperation::Withdraw => handle.create_pool_withdraw_payload(signer).await,
                };

                match result {
                    Ok(payload) => {
                        if let Err(e) =
                            this.update(&mut async_cx, |this, cx: &mut Context<Self>| {
                                this.pool_action_generating = false;
                                this.pool_action_message = None;
                                this.clear_qr_signature_state();
                                this.pending_tx_payload = Some(payload);
                                this.show_pool_modal = false;
                                this.show_qr_modal = true;
                                this.qr_modal_tab = QrModalTab::QrCode;
                                cx.notify();
                            })
                        {
                            tracing::error!("Failed to update UI with pool payload: {:?}", e);
                        }
                    }
                    Err(e) => {
                        if let Err(update_err) =
                            this.update(&mut async_cx, |this, cx: &mut Context<Self>| {
                                this.pool_action_generating = false;
                                this.pool_action_message = Some(e);
                                cx.notify();
                            })
                        {
                            tracing::error!(
                                "Failed to update UI with pool error: {:?}",
                                update_err
                            );
                        }
                    }
                }
            },
        )
        .detach();
    }

    /// Load staking history for the current account.
    pub fn load_history(&mut self, cx: &mut Context<Self>) {
        let Some(ref address) = self.watched_account else {
            tracing::warn!("No account to load history for");
            return;
        };
        let Some(ref chain_handle) = self.chain_handle else {
            tracing::warn!("Not connected to chain");
            return;
        };

        self.history_loading = true;
        cx.notify();

        let address = address.clone();
        let chain_handle = chain_handle.clone();
        let num_eras = 30u32; // Load last 30 eras
        let mut async_cx = cx.to_async();

        cx.spawn(
            move |this: gpui::WeakEntity<Self>, _cx: &mut gpui::AsyncApp| async move {
                let result = chain_handle.fetch_history(address, num_eras).await;
                if let Err(e) = this.update(&mut async_cx, |this, cx: &mut Context<Self>| {
                    this.history_loading = false;
                    match result {
                        Ok(history) => {
                            tracing::info!("History loaded: {} points", history.len());
                            this.staking_history = history;
                        }
                        Err(e) => {
                            tracing::error!("Failed to load history: {}", e);
                            this.connection_error = Some(format!("Failed to load history: {}", e));
                        }
                    }
                    cx.notify();
                }) {
                    tracing::error!("Failed to update UI with history: {:?}", e);
                }
            },
        )
        .detach();
    }

    /// Clear state derived from a previously scanned signature.
    pub fn clear_qr_signature_state(&mut self) {
        self.scanned_signature = None;
        self.signed_extrinsic = None;
        self.tx_status_message = None;
        self.tx_status = QrTxStatus::NotReady;
        self.tx_submitting = false;
        self.camera_preview = None;
    }

    /// Close the QR modal and clear transaction-scanning state.
    pub fn close_qr_modal(&mut self, cx: &mut Context<Self>) {
        self.show_qr_modal = false;
        self.pending_tx_payload = None;
        self.clear_qr_signature_state();
        self.stop_camera_with_reason("QR modal closed", cx);
    }

    /// Switch QR modal tabs and keep camera lifecycle aligned with the scan tab.
    pub fn set_qr_modal_tab(&mut self, tab: QrModalTab, cx: &mut Context<Self>) {
        self.qr_modal_tab = tab;
        match tab {
            QrModalTab::ScanSignature => {
                if self.pending_tx_payload.is_some()
                    && self.signed_extrinsic.is_none()
                    && self.qr_reader.is_none()
                {
                    self.start_camera(cx);
                } else {
                    cx.notify();
                }
            }
            _ => {
                if self.qr_reader.is_some() {
                    self.stop_camera_with_reason("left Scan Signature tab", cx);
                } else {
                    cx.notify();
                }
            }
        }
    }

    /// Start the camera for QR scanning.
    pub fn start_camera(&mut self, cx: &mut Context<Self>) {
        if self.qr_reader.is_some() {
            cx.notify();
            return;
        }

        // Ensure camera permission on macOS
        #[cfg(target_os = "macos")]
        {
            if let Err(e) = crate::tcc::ensure_camera_permission() {
                tracing::error!("Camera permission denied: {}", e);
                self.connection_error = Some(format!("Camera permission denied: {}", e));
                cx.notify();
                return;
            }
        }

        // Start the QR reader
        match crate::qr_reader::QrReader::new() {
            Ok(reader) => {
                self.qr_reader = Some(reader);
                self.camera_preview = None;
                self.tx_status_message = None;
                self.tx_status = QrTxStatus::NotReady;
                tracing::info!("Camera started for QR scanning");
                self.schedule_camera_poll(cx);
            }
            Err(e) => {
                tracing::error!("Failed to start camera: {}", e);
                self.connection_error = Some(format!("Failed to start camera: {}", e));
            }
        }
        cx.notify();
    }

    /// Stop the camera.
    pub fn stop_camera(&mut self, cx: &mut Context<Self>) {
        self.stop_camera_with_reason("camera stop requested", cx);
    }

    /// Stop the camera and include the UI reason in the logs.
    pub fn stop_camera_with_reason(&mut self, reason: &'static str, cx: &mut Context<Self>) {
        if let Some(mut reader) = self.qr_reader.take() {
            reader.stop();
            tracing::info!("Camera stopped ({})", reason);
        }
        self.camera_preview = None;
        cx.notify();
    }

    /// Wake GPUI periodically while the camera reader is active.
    fn schedule_camera_poll(&self, cx: &mut Context<Self>) {
        let mut async_cx = cx.to_async();
        let executor = async_cx.background_executor().clone();
        cx.spawn(
            move |this: gpui::WeakEntity<Self>, _cx: &mut gpui::AsyncApp| async move {
                loop {
                    executor.timer(Duration::from_millis(80)).await;

                    let keep_polling =
                        match this.update(&mut async_cx, |this, cx: &mut Context<Self>| {
                            if this.qr_reader.is_some() {
                                this.poll_camera(cx);
                                true
                            } else {
                                false
                            }
                        }) {
                            Ok(keep_polling) => keep_polling,
                            Err(e) => {
                                tracing::warn!("Failed to poll QR camera from UI task: {:?}", e);
                                false
                            }
                        };

                    if !keep_polling {
                        break;
                    }
                }
            },
        )
        .detach();
    }

    fn build_signed_extrinsic_from_signature(
        &mut self,
        signature_data: &[u8],
    ) -> Result<(), String> {
        let payload = self.pending_tx_payload.as_ref().ok_or_else(|| {
            "Signature received but no pending transaction is available".to_string()
        })?;

        let decoded_sig = stkopt_chain::decode_vault_signature(signature_data)?;
        tracing::info!("Decoded {:?} signature from Vault QR", decoded_sig.sig_type);

        let signed = stkopt_chain::build_signed_extrinsic(
            &payload.unsigned_payload,
            &payload.signer,
            &decoded_sig,
        )
        .map_err(|e| format!("Failed to build signed transaction: {}", e))?;

        tracing::info!(
            "Signed extrinsic built: 0x{} ({} bytes)",
            hex::encode(signed.hash),
            signed.encoded.len()
        );

        let encoded_len = signed.encoded.len();
        self.signed_extrinsic = Some(signed.encoded);
        self.tx_status = QrTxStatus::Ready;
        self.tx_status_message = Some(format!(
            "Signature decoded. Signed transaction ready ({} bytes).",
            encoded_len
        ));
        Ok(())
    }

    /// Submit the signed extrinsic built from the scanned Vault QR code.
    pub fn submit_scanned_transaction(&mut self, cx: &mut Context<Self>) {
        if self.tx_status == QrTxStatus::Submitting {
            return;
        }

        if self.tx_status == QrTxStatus::Submitted {
            self.tx_status_message = Some("Transaction already submitted".to_string());
            cx.notify();
            return;
        }

        let Some(ref chain_handle) = self.chain_handle else {
            self.tx_status = QrTxStatus::Failed;
            self.tx_status_message = Some("Not connected".to_string());
            cx.notify();
            return;
        };

        let Some(extrinsic) = self.signed_extrinsic.clone() else {
            self.tx_status = QrTxStatus::NotReady;
            self.tx_status_message = Some("Scan the signed QR code first".to_string());
            cx.notify();
            return;
        };

        self.tx_submitting = true;
        self.tx_status = QrTxStatus::Submitting;
        self.tx_status_message = Some("Submitting transaction...".to_string());
        cx.notify();

        let handle = chain_handle.clone();
        let mut async_cx = cx.to_async();
        cx.spawn(
            move |this: gpui::WeakEntity<Self>, _cx: &mut gpui::AsyncApp| async move {
                let result = handle.submit_signed_extrinsic(extrinsic).await;
                if let Err(e) = this.update(&mut async_cx, |this, cx: &mut Context<Self>| {
                    this.tx_submitting = false;
                    let (status, message) = match result {
                        Ok(crate::chain::TxSubmissionResult::InBlock { block_hash }) => {
                            this.signed_extrinsic = None;
                            (
                                QrTxStatus::Submitted,
                                format!(
                                    "Transaction included in block 0x{}",
                                    hex::encode(block_hash)
                                ),
                            )
                        }
                        Ok(crate::chain::TxSubmissionResult::Finalized { block_hash }) => {
                            this.signed_extrinsic = None;
                            (
                                QrTxStatus::Submitted,
                                format!(
                                    "Transaction finalized in block 0x{}",
                                    hex::encode(block_hash)
                                ),
                            )
                        }
                        Ok(crate::chain::TxSubmissionResult::Dropped(reason)) => (
                            QrTxStatus::Failed,
                            format!("Transaction dropped: {}", reason),
                        ),
                        Err(e) => (
                            QrTxStatus::Failed,
                            format!("Transaction submission failed: {}", e),
                        ),
                    };
                    this.tx_status = status;
                    this.tx_status_message = Some(message);
                    cx.notify();
                }) {
                    tracing::error!("Failed to update UI after transaction submit: {:?}", e);
                }
            },
        )
        .detach();
    }

    /// Poll the camera for scan results (called from render loop).
    pub fn poll_camera(&mut self, cx: &mut Context<Self>) {
        // Collect results first, then process
        let results: Vec<_> = if let Some(ref reader) = self.qr_reader {
            std::iter::from_fn(|| reader.try_recv()).collect()
        } else {
            return;
        };

        let mut received_update = false;

        for result in results {
            match result {
                crate::qr_reader::QrScanResult::Success(data, preview) => {
                    tracing::info!("QR code scanned: {} bytes", data.len());
                    self.camera_preview = Some(preview);
                    self.scanned_signature = Some(data.clone());
                    // Stop scanning after successful decode
                    if let Some(mut reader) = self.qr_reader.take() {
                        reader.stop();
                    }
                    match self.build_signed_extrinsic_from_signature(&data) {
                        Ok(()) => {}
                        Err(e) => {
                            tracing::error!("Failed to process signature QR: {}", e);
                            self.signed_extrinsic = None;
                            self.tx_status = QrTxStatus::Failed;
                            self.tx_status_message =
                                Some(format!("Failed to read signed QR: {}", e));
                        }
                    }

                    // Move to submit tab
                    self.qr_modal_tab = QrModalTab::Submit;
                    cx.notify();
                    return;
                }
                crate::qr_reader::QrScanResult::Scanning(preview) => {
                    self.camera_preview = Some(preview);
                    received_update = true;
                }
                crate::qr_reader::QrScanResult::Detected(preview) => {
                    self.camera_preview = Some(preview);
                    received_update = true;
                }
                crate::qr_reader::QrScanResult::Error(e) => {
                    tracing::error!("Camera error: {}", e);
                    self.connection_error = Some(e);
                    if let Some(mut reader) = self.qr_reader.take() {
                        reader.stop();
                    }
                    received_update = true;
                }
            }
        }

        if received_update {
            cx.notify();
        }
    }

    fn render_sidebar(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let entity = self.entity.clone();
        let current_section = self.current_section;

        let mut nav = div()
            .id("sidebar-root")
            .flex()
            .flex_col()
            .w(px(200.0))
            .min_w(px(200.0))
            .h_full()
            .bg(theme.surface)
            .border_r_1()
            .border_color(theme.border)
            .py_3()
            .on_mouse_down(MouseButton::Left, |_event, _window, _cx| {
                tracing::info!("[SIDEBAR] Root clicked!");
            });

        // App title and network selector
        nav = nav.child(
            div()
                .px_3()
                .pb_3()
                .mb_1()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(Text::new("⚡").size(TextSize::Xl))
                        .child(Heading::h3("Staking Optimizer").into_any_element()),
                )
                .child(
                    div()
                        .mt_1()
                        .flex()
                        .gap_1()
                        .child(network_pill(
                            "DOT",
                            Network::Polkadot,
                            self.network,
                            &theme,
                            entity.clone(),
                        ))
                        .child(network_pill(
                            "KSM",
                            Network::Kusama,
                            self.network,
                            &theme,
                            entity.clone(),
                        ))
                        .child(network_pill(
                            "WND",
                            Network::Westend,
                            self.network,
                            &theme,
                            entity.clone(),
                        ))
                        .child(network_pill(
                            "PAS",
                            Network::Paseo,
                            self.network,
                            &theme,
                            entity.clone(),
                        )),
                ),
        );

        // Navigation items
        for section in Section::all() {
            let section = *section;
            let is_active = section == current_section;
            let entity_clone = entity.clone();

            let mut item = div()
                .id(SharedString::from(format!("nav-{:?}", section)))
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_1()
                .mx_1()
                .rounded_md()
                .cursor_pointer()
                .text_sm();

            if is_active {
                item = item
                    .bg(theme.accent)
                    .text_color(crate::theme::text_color_on(theme.accent, &theme))
                    .font_weight(FontWeight::SEMIBOLD);
            } else {
                let hover_bg = theme.surface_hover;
                item = item
                    .text_color(theme.text_secondary)
                    .hover(move |s| s.bg(hover_bg));
            }

            item = item
                .child(div().child(section.icon()))
                .child(div().child(section.label()))
                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                    entity_clone.update(cx, |this, cx| {
                        this.current_section = section;
                        cx.notify();
                    });
                });

            nav = nav.child(item);
        }

        // Connection section at bottom
        nav = nav.child(
            div()
                .mt_auto()
                .px_3()
                .pt_3()
                .border_t_1()
                .border_color(theme.border)
                .flex()
                .flex_col()
                .gap_2()
                .child(self.render_connection_controls(cx))
                .child(self.render_connection_status(cx)),
        );

        nav
    }

    fn render_connection_controls(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let entity = self.entity.clone();

        div()
            .flex()
            .gap_1()
            .child(mode_pill(
                "Light",
                ConnectionMode::LightClient,
                self.connection_mode,
                &theme,
                entity.clone(),
            ))
            .child(mode_pill(
                "RPC",
                ConnectionMode::Rpc,
                self.connection_mode,
                &theme,
                entity.clone(),
            ))
    }

    fn render_connection_status(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        let (status_text, status_color) = match self.connection_status {
            ConnectionStatus::Disconnected => ("Disconnected", theme.error),
            ConnectionStatus::Connecting => ("Connecting...", theme.warning),
            ConnectionStatus::Connected => {
                if self.data_download_complete() {
                    ("Connected", theme.success)
                } else {
                    ("Loading data...", theme.warning)
                }
            }
        };

        let bars = self.connection_progress_steps();

        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(div().w(px(6.0)).h(px(6.0)).rounded_full().bg(status_color))
                    .child(Text::new(status_text).size(TextSize::Xs)),
            )
            .children(bars.into_iter().map(|(label, done)| {
                let fill_color = if done { theme.success } else { theme.border };
                let fill = div().h_full().rounded_full().bg(fill_color);
                let fill = if done { fill.w_full() } else { fill.w(px(0.0)) };

                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        Text::new(label)
                            .size(TextSize::Xs)
                            .color(theme.text_secondary),
                    )
                    .child(
                        div()
                            .w_full()
                            .h(px(4.0))
                            .rounded_full()
                            .bg(theme.border)
                            .child(fill),
                    )
            }))
    }

    fn render_content(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        let content: AnyElement = match self.current_section {
            Section::Dashboard => DashboardSection::render(self, cx).into_any_element(),
            Section::Account => AccountSection::render(self, cx).into_any_element(),
            Section::Validators => ValidatorsSection::render(self, cx).into_any_element(),
            Section::Optimization => OptimizationSection::render(self, cx).into_any_element(),
            Section::Pools => PoolsSection::render(self, cx).into_any_element(),
            Section::History => HistorySection::render(self, cx).into_any_element(),
        };

        div()
            .id("content-outer")
            .flex_1()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(theme.background)
            .on_mouse_down(MouseButton::Left, |_event, _window, _cx| {
                tracing::info!("[CONTENT] Outer container clicked!");
            })
            .child(
                div()
                    .id("content-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .p_6()
                    .bg(theme.transparent) // Transparent but present for hit-testing
                    .on_mouse_down(MouseButton::Left, |_event, _window, _cx| {
                        tracing::info!("[CONTENT] Scroll container clicked!");
                    })
                    .child(content),
            )
    }

    fn render_main_layout(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        let mut layout = div()
            .id("main-layout")
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(theme.background)
            .child(
                div()
                    .id("main-upper")
                    .flex()
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .child(self.render_sidebar(cx))
                    .child(self.render_content(cx)),
            )
            .child(self.render_log_divider(cx));

        if self.show_logs {
            layout = layout.child(
                div()
                    .id("bottom-log-pane")
                    .w_full()
                    .h(px(self.log_pane_height))
                    .min_h(px(LOG_PANE_MIN_HEIGHT))
                    .overflow_hidden()
                    .child(LogsView::render(
                        &self.log_buffer,
                        self.log_level_filter,
                        cx,
                        self.entity.clone(),
                    )),
            );
        }

        layout
    }

    fn render_log_divider(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let entity = self.entity.clone();

        PaneDivider::horizontal("log-pane-divider", CollapseDirection::Down)
            .label("Logs")
            .collapsed(!self.show_logs)
            .thickness(px(8.0))
            .collapsed_size(px(24.0))
            .theme(PaneDividerTheme::from(&theme))
            .on_toggle({
                let entity = entity.clone();
                move |collapsed, _window, cx| {
                    entity.update(cx, |this, cx| {
                        this.set_logs_visible(!collapsed, cx);
                    });
                }
            })
            .on_drag_start(move |position_y, _window, cx| {
                entity.update(cx, |this, cx| {
                    this.begin_log_pane_drag(position_y, cx);
                });
            })
    }
}

impl Render for StkoptApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Track viewport width for responsive chart sizing
        self.viewport_width = f32::from(window.viewport_size().width);
        self.viewport_height = f32::from(window.viewport_size().height);
        self.log_pane_height = clamp_log_pane_height(self.log_pane_height, self.viewport_height);

        // Process any pending chain updates
        self.process_pending_updates(cx);

        // Poll camera for QR scan results
        self.poll_camera(cx);

        let theme = cx.theme();
        let entity = self.entity.clone();

        let mut root = div()
            .id("stkopt-root")
            .track_focus(&self.focus_handle)
            .w_full()
            .h_full()
            .bg(theme.background)
            .text_color(theme.text_primary)
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_event, _window, _cx| {
                tracing::info!("[ROOT] Root element clicked!");
            });

        root = root
            .on_mouse_move({
                let entity = entity.clone();
                move |event, _window, cx| {
                    if event.pressed_button == Some(MouseButton::Left) {
                        let y: f32 = event.position.y.into();
                        entity.update(cx, |this, cx| {
                            this.update_log_pane_drag(y, cx);
                        });
                    }
                }
            })
            .on_mouse_up(MouseButton::Left, {
                let entity = entity.clone();
                move |_event, _window, cx| {
                    entity.update(cx, |this, cx| {
                        this.end_log_pane_drag(cx);
                    });
                }
            });

        // Add keyboard handler for shortcuts (Cmd on macOS, Ctrl on other platforms)
        root = root.on_key_down({
            let entity = entity.clone();
            move |event, _window, cx| {
                // Check for platform modifier (Cmd on macOS, Ctrl elsewhere)
                #[cfg(target_os = "macos")]
                let has_cmd_modifier = event.keystroke.modifiers.platform;
                #[cfg(not(target_os = "macos"))]
                let has_cmd_modifier = event.keystroke.modifiers.control;

                // Cmd/Ctrl+, to toggle settings
                if event.keystroke.key == "," && has_cmd_modifier {
                    entity.update(cx, |this, cx| {
                        this.show_settings = !this.show_settings;
                        cx.notify();
                    });
                }
                // Cmd/Ctrl+L to toggle logs
                if event.keystroke.key == "l" && has_cmd_modifier {
                    entity.update(cx, |this, cx| {
                        this.set_logs_visible(!this.show_logs, cx);
                    });
                }
                // Escape to close modals/settings/help/logs (in priority order)
                if event.keystroke.key == "escape" {
                    entity.update(cx, |this, cx| {
                        if this.show_qr_modal {
                            this.close_qr_modal(cx);
                        } else if this.show_staking_modal {
                            this.show_staking_modal = false;
                        } else if this.show_pool_modal {
                            this.show_pool_modal = false;
                        } else if this.show_help {
                            this.show_help = false;
                        } else if this.show_settings {
                            this.show_settings = false;
                        } else if this.show_logs {
                            this.set_logs_visible(false, cx);
                        }
                        cx.notify();
                    });
                }
                // ? to toggle help
                if event.keystroke.key == "?"
                    || (event.keystroke.key == "/" && event.keystroke.modifiers.shift)
                {
                    entity.update(cx, |this, cx| {
                        this.show_help = !this.show_help;
                        cx.notify();
                    });
                }
            }
        });

        if self.show_settings {
            root = root.child(
                div()
                    .id("settings-scroll")
                    .w_full()
                    .h_full()
                    .flex()
                    .flex_col()
                    .p_6()
                    .overflow_y_scroll()
                    .child(SettingsSection::render(self, cx)),
            );
        } else {
            root = root.child(self.render_main_layout(cx));
        }

        // Render overlays on top
        if self.show_help {
            root = root.child(HelpOverlay::render(self, cx));
        }
        if self.show_staking_modal {
            root = root.child(crate::views::StakingModal::render(self, cx));
        }
        if self.show_pool_modal {
            root = root.child(crate::views::PoolModal::render(self, cx));
        }
        if self.show_qr_modal {
            root = root.child(crate::views::QrModal::render(self, cx));
        }

        root
    }
}

/// Network selector pill button.
fn network_pill(
    label: &'static str,
    network: Network,
    current: Network,
    theme: &gpui_ui_kit::theme::Theme,
    entity: Entity<StkoptApp>,
) -> impl IntoElement {
    let is_active = network == current;
    let bg = if is_active {
        theme.accent
    } else {
        theme.surface_hover
    };
    let text_color = if is_active {
        crate::theme::text_color_on(theme.accent, theme)
    } else {
        theme.text_secondary
    };

    div()
        .id(SharedString::from(format!("network-{:?}", network)))
        .px_1()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .bg(bg)
        .child(
            Text::new(label)
                .size(TextSize::Xs)
                .color(text_color)
                .weight(if is_active { TextWeight::Semibold } else { TextWeight::Normal }),
        )
        .on_click(move |_event, _window, cx| {
            entity.update(cx, |this, cx| {
                if this.network != network {
                    this.network = network;
                    this.save_config();
                    // Disconnect from current network (keep chain_handle for future commands)
                    if this.connection_status == ConnectionStatus::Connected {
                        if let Some(ref handle) = this.chain_handle {
                            let handle = handle.clone();
                            cx.spawn(move |_: gpui::WeakEntity<StkoptApp>, _: &mut gpui::AsyncApp| async move {
                                let _ = handle.disconnect().await;
                            }).detach();
                        }
                        this.connection_status = ConnectionStatus::Disconnected;
                    }
                    // Clear cached data for old network
                    this.validators.clear();
                    this.pools.clear();
                    this.staking_info = None;
                    this.staking_history.clear();
                    this.operations_loading = false;
                    this.validators_loading = false;
                    this.pools_loading = false;
                    this.account_loading = false;
                    this.history_loading = false;
                    cx.notify();
                }
            });
        })
}

/// Connection mode selector pill button.
fn mode_pill(
    label: &'static str,
    mode: ConnectionMode,
    current: ConnectionMode,
    theme: &gpui_ui_kit::theme::Theme,
    entity: Entity<StkoptApp>,
) -> impl IntoElement {
    let is_active = mode == current;
    let bg = if is_active {
        theme.accent
    } else {
        theme.surface_hover
    };
    let text_color = if is_active {
        crate::theme::text_color_on(theme.accent, theme)
    } else {
        theme.text_secondary
    };

    div()
        .id(SharedString::from(format!("mode-{:?}", mode)))
        .px_1()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .bg(bg)
        .child(
            Text::new(label)
                .size(TextSize::Xs)
                .color(text_color)
                .weight(if is_active {
                    TextWeight::Semibold
                } else {
                    TextWeight::Normal
                }),
        )
        .on_click(move |_event, _window, cx| {
            entity.update(cx, |this, cx| {
                if this.connection_mode != mode {
                    this.set_connection_mode(mode, cx);
                }
            });
        })
}
