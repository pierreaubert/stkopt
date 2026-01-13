//! Main application state and view for the Staking Optimizer desktop app.

use std::sync::{Arc, Mutex};

use gpui::prelude::*;
use gpui::*;
use gpui_ui_kit::theme::ThemeExt;
use gpui_ui_kit::*;

use crate::views::{
    AccountSection, DashboardSection, HelpOverlay, HistorySection, LogsView, OptimizationSection,
    PoolsSection, SettingsSection, ValidatorsSection,
};

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
            Section::Dashboard,
            Section::Account,
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
    /// Staking history data points
    pub staking_history: Vec<HistoryPoint>,
    /// Whether history is currently loading
    pub history_loading: bool,
    /// Available nomination pools
    pub pools: Vec<PoolInfo>,
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
    /// Saved accounts (address book)
    pub address_book: Vec<SavedAccount>,
    /// Whether staking modal is visible
    pub show_staking_modal: bool,
    /// Current staking operation type
    pub staking_operation: StakingOperation,
    /// Amount input for staking operations
    pub staking_amount_input: String,
    /// Pending transaction payload (for QR display/signing)
    pub pending_tx_payload: Option<crate::chain::TransactionPayload>,
    /// Whether QR modal is visible
    pub show_qr_modal: bool,
    /// Current tab in QR modal
    pub qr_modal_tab: QrModalTab,
    /// Transaction submission status message
    pub tx_status_message: Option<String>,
    /// Whether pool operations modal is visible
    pub show_pool_modal: bool,
    /// Current pool operation type
    pub pool_operation: PoolOperation,
    /// Selected pool ID for operation
    pub selected_pool_id: Option<u32>,
    /// Amount input for pool operations
    pub pool_amount_input: String,
    /// Active QR reader (camera)
    pub qr_reader: Option<crate::qr_reader::QrReader>,
    /// Latest camera preview frame
    pub camera_preview: Option<crate::qr_reader::CameraPreview>,
    /// Scanned signature data (from Vault QR)
    pub scanned_signature: Option<Vec<u8>>,
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
            | StakingOperation::ClaimRewards => false,
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
}

impl Network {
    pub fn label(&self) -> &'static str {
        match self {
            Network::Polkadot => "Polkadot",
            Network::Kusama => "Kusama",
            Network::Westend => "Westend",
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            Network::Polkadot => "DOT",
            Network::Kusama => "KSM",
            Network::Westend => "WND",
        }
    }

    pub fn token_decimals(&self) -> u8 {
        match self {
            Network::Polkadot => 10,
            Network::Kusama => 12,
            Network::Westend => 12,
        }
    }

    /// Convert to stkopt_core::Network
    pub fn to_core(&self) -> stkopt_core::Network {
        match self {
            Network::Polkadot => stkopt_core::Network::Polkadot,
            Network::Kusama => stkopt_core::Network::Kusama,
            Network::Westend => stkopt_core::Network::Westend,
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
            crate::persistence::NetworkConfig::Paseo => Network::Westend, // Paseo not yet supported in GPUI
            crate::persistence::NetworkConfig::Custom => Network::Polkadot,
        };

        // Convert connection mode config
        let connection_mode = match config.connection_mode {
            crate::persistence::ConnectionModeConfig::Rpc => ConnectionMode::Rpc,
            crate::persistence::ConnectionModeConfig::LightClient => ConnectionMode::LightClient,
        };

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
                    let _ = this.update(&mut async_cx, |_, cx: &mut Context<Self>| cx.notify());
                }
            },
        )
        .detach();

        // Auto-connect if enabled
        if config.auto_connect {
            let handle = chain_handle.clone();
            let net = network; // Copy
            let use_light = connection_mode == ConnectionMode::LightClient;
            cx.spawn(move |_, _cx: &mut gpui::AsyncApp| async move {
                if let Err(e) = handle.connect(net.to_core(), use_light).await {
                    eprintln!("Failed to auto-connect: {}", e);
                }
            })
            .detach();
        }

        let instance = Self {
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
            validators: Vec::new(),
            selected_validators: Vec::new(),
            validator_sort: crate::actions::ValidatorSortColumn::default(),
            validator_sort_asc: false,
            validator_search: String::new(),
            staking_history: Vec::new(),
            history_loading: false,
            pools: Vec::new(),
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
            show_logs: false,
            address_book: Vec::new(),
            show_staking_modal: false,
            staking_operation: StakingOperation::default(),
            staking_amount_input: String::new(),
            pending_tx_payload: None,
            show_qr_modal: false,
            qr_modal_tab: QrModalTab::default(),
            tx_status_message: None,
            show_pool_modal: false,
            pool_operation: PoolOperation::default(),
            selected_pool_id: None,
            pool_amount_input: String::new(),
            qr_reader: None,
            camera_preview: None,
            scanned_signature: None,
        };

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
                    let _ = this.update(&mut async_cx, |_, cx: &mut Context<Self>| cx.notify());
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
                    stkopt_core::ConnectionStatus::Disconnected => ConnectionStatus::Disconnected,
                    stkopt_core::ConnectionStatus::Connecting => ConnectionStatus::Connecting,
                    stkopt_core::ConnectionStatus::Connected => ConnectionStatus::Connected,
                    stkopt_core::ConnectionStatus::Syncing { .. } => ConnectionStatus::Connecting,
                    stkopt_core::ConnectionStatus::Error(e) => {
                        self.connection_error = Some(e);
                        ConnectionStatus::Disconnected
                    }
                };
            }
            ChainUpdate::ValidatorsLoaded(validators) => {
                self.validators = validators;
                tracing::info!("Loaded {} validators from chain", self.validators.len());
            }
            ChainUpdate::PoolsLoaded(pools) => {
                self.pools = pools;
                tracing::info!("Loaded {} pools from chain", self.pools.len());
            }
            ChainUpdate::AccountLoaded(account_data) => {
                // Update staking info from account data
                self.staking_info = Some(StakingInfo {
                    total_balance: account_data.free_balance + account_data.reserved_balance,
                    transferable: account_data.free_balance,
                    bonded: account_data.staked_balance.unwrap_or(0),
                    unbonding: account_data.unbonding_balance,
                    rewards_pending: 0, // Rewards are auto-compounded or claimed, no "pending" in modern staking
                    is_nominating: account_data.is_nominating,
                    nomination_count: account_data.nominations.len(),
                });
                tracing::info!(
                    "Account data loaded: balance={}, unbonding={}",
                    account_data.free_balance,
                    account_data.unbonding_balance
                );
            }
            ChainUpdate::HistoryLoaded(history) => {
                self.staking_history = history;
            }
            ChainUpdate::QrPayloadGenerated(payload) => {
                // Store the generated QR payload for display in QR modal
                tracing::info!("QR payload generated: {}", payload.description);
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
    }

    /// Remove an account from the address book.
    pub fn remove_from_address_book(&mut self, address: &str) {
        self.address_book
            .retain(|a| a.address != address || a.network != self.network);
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

    /// Open the staking modal with a specific operation.
    pub fn open_staking_modal(&mut self, operation: StakingOperation, cx: &mut Context<Self>) {
        self.staking_operation = operation;
        self.staking_amount_input.clear();
        self.show_staking_modal = true;
        cx.notify();
    }

    /// Generate QR payload for the current staking operation.
    pub fn generate_staking_qr(&mut self, cx: &mut Context<Self>) {
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

        // Parse the account address
        let signer = match address.parse::<subxt::utils::AccountId32>() {
            Ok(id) => id,
            Err(e) => {
                self.connection_error = Some(format!("Invalid address: {}", e));
                cx.notify();
                return;
            }
        };

        // Parse amount if needed
        let amount = if self.staking_operation.requires_amount() {
            let divisor = self.decimal_divisor();
            match self.staking_amount_input.parse::<f64>() {
                Ok(val) => (val * divisor as f64) as u128,
                Err(_) => {
                    self.connection_error = Some("Invalid amount".to_string());
                    cx.notify();
                    return;
                }
            }
        } else {
            0
        };

        let handle = chain_handle.clone();
        let operation = self.staking_operation;
        let mut async_cx = cx.to_async();

        cx.spawn(
            move |this: gpui::WeakEntity<Self>, _cx: &mut gpui::AsyncApp| async move {
                let result = match operation {
                    StakingOperation::Bond => handle.create_bond_payload(signer, amount).await,
                    StakingOperation::Unbond => handle.create_unbond_payload(signer, amount).await,
                    StakingOperation::BondExtra => {
                        handle.create_bond_extra_payload(signer, amount).await
                    }
                    StakingOperation::WithdrawUnbonded => {
                        handle.create_withdraw_unbonded_payload(signer).await
                    }
                    StakingOperation::Chill => handle.create_chill_payload(signer).await,
                    StakingOperation::Rebond
                    | StakingOperation::Nominate
                    | StakingOperation::ClaimRewards => Err("Not implemented yet".to_string()),
                };

                match result {
                    Ok(payload) => {
                        let _ = this.update(&mut async_cx, |this, cx: &mut Context<Self>| {
                            this.pending_tx_payload = Some(payload);
                            this.show_staking_modal = false;
                            this.show_qr_modal = true;
                            this.qr_modal_tab = QrModalTab::QrCode;
                            cx.notify();
                        });
                    }
                    Err(e) => {
                        let _ = this.update(&mut async_cx, |this, cx: &mut Context<Self>| {
                            this.connection_error = Some(e);
                            cx.notify();
                        });
                    }
                }
            },
        )
        .detach();
    }

    /// Generate QR payload for nominating validators.
    pub fn generate_nominate_qr(&mut self, targets: Vec<String>, cx: &mut Context<Self>) {
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
                        let _ = this.update(&mut async_cx, |this, cx: &mut Context<Self>| {
                            this.pending_tx_payload = Some(payload);
                            this.show_qr_modal = true;
                            this.qr_modal_tab = QrModalTab::QrCode;
                            cx.notify();
                        });
                    }
                    Err(e) => {
                        let _ = this.update(&mut async_cx, |this, cx: &mut Context<Self>| {
                            this.connection_error = Some(e);
                            cx.notify();
                        });
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
        self.pool_operation = operation;
        self.selected_pool_id = pool_id;
        self.pool_amount_input.clear();
        self.show_pool_modal = true;
        cx.notify();
    }

    /// Generate QR payload for the current pool operation.
    pub fn generate_pool_qr(&mut self, cx: &mut Context<Self>) {
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

        // Parse the account address
        let signer = match address.parse::<subxt::utils::AccountId32>() {
            Ok(id) => id,
            Err(e) => {
                self.connection_error = Some(format!("Invalid address: {}", e));
                cx.notify();
                return;
            }
        };

        // Parse amount if needed
        let amount = if self.pool_operation.requires_amount() {
            let divisor = self.decimal_divisor();
            match self.pool_amount_input.parse::<f64>() {
                Ok(val) => (val * divisor as f64) as u128,
                Err(_) => {
                    self.connection_error = Some("Invalid amount".to_string());
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
                        let _ = this.update(&mut async_cx, |this, cx: &mut Context<Self>| {
                            this.pending_tx_payload = Some(payload);
                            this.show_pool_modal = false;
                            this.show_qr_modal = true;
                            this.qr_modal_tab = QrModalTab::QrCode;
                            cx.notify();
                        });
                    }
                    Err(e) => {
                        let _ = this.update(&mut async_cx, |this, cx: &mut Context<Self>| {
                            this.connection_error = Some(e);
                            cx.notify();
                        });
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
                let _ = this.update(&mut async_cx, |this, cx: &mut Context<Self>| {
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
                });
            },
        )
        .detach();
    }

    /// Start the camera for QR scanning.
    pub fn start_camera(&mut self, cx: &mut Context<Self>) {
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
                tracing::info!("Camera started for QR scanning");
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
        if let Some(mut reader) = self.qr_reader.take() {
            reader.stop();
            tracing::info!("Camera stopped");
        }
        self.camera_preview = None;
        cx.notify();
    }

    /// Poll the camera for scan results (called from render loop).
    pub fn poll_camera(&mut self, cx: &mut Context<Self>) {
        // Collect results first, then process
        let results: Vec<_> = if let Some(ref reader) = self.qr_reader {
            std::iter::from_fn(|| reader.try_recv()).collect()
        } else {
            return;
        };

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
                    // Move to submit tab
                    self.qr_modal_tab = QrModalTab::Submit;
                    self.tx_status_message =
                        Some(format!("Signature scanned ({} bytes)", data.len()));
                    cx.notify();
                    return;
                }
                crate::qr_reader::QrScanResult::Scanning(preview) => {
                    self.camera_preview = Some(preview);
                }
                crate::qr_reader::QrScanResult::Detected(preview) => {
                    self.camera_preview = Some(preview);
                }
                crate::qr_reader::QrScanResult::Error(e) => {
                    tracing::error!("Camera error: {}", e);
                    self.connection_error = Some(e);
                    if let Some(mut reader) = self.qr_reader.take() {
                        reader.stop();
                    }
                }
            }
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
            .w(px(220.0))
            .min_w(px(220.0))
            .h_full()
            .bg(theme.surface)
            .border_r_1()
            .border_color(theme.border)
            .py_4()
            .on_mouse_down(MouseButton::Left, |_event, _window, _cx| {
                tracing::info!("[SIDEBAR] Root clicked!");
            });

        // App title and network selector
        nav = nav.child(
            div()
                .px_4()
                .pb_4()
                .mb_2()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(Text::new("⚡").size(TextSize::Lg))
                        .child(Heading::h3("Staking Optimizer").into_any_element()),
                )
                .child(
                    div()
                        .mt_2()
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
                .gap_3()
                .px_4()
                .py_2()
                .mx_2()
                .rounded_md()
                .cursor_pointer()
                .text_sm();

            if is_active {
                item = item
                    .bg(theme.accent)
                    .text_color(rgba(0xffffffff))
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
                .px_4()
                .pt_4()
                .border_t_1()
                .border_color(theme.border)
                .flex()
                .flex_col()
                .gap_3()
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
            ConnectionStatus::Connected => ("Connected", theme.success),
        };

        div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().w(px(8.0)).h(px(8.0)).rounded_full().bg(status_color))
            .child(Text::new(status_text).size(TextSize::Sm))
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
                    .bg(gpui::rgba(0x00000000)) // Transparent but present for hit-testing
                    .on_mouse_down(MouseButton::Left, |_event, _window, _cx| {
                        tracing::info!("[CONTENT] Scroll container clicked!");
                    })
                    .child(content),
            )
    }
}

impl Render for StkoptApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            .on_mouse_down(MouseButton::Left, |_event, _window, _cx| {
                tracing::info!("[ROOT] Root element clicked!");
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
                        this.show_logs = !this.show_logs;
                        cx.notify();
                    });
                }
                // Escape to close modals/settings/help/logs (in priority order)
                if event.keystroke.key == "escape" {
                    entity.update(cx, |this, cx| {
                        if this.show_qr_modal {
                            this.show_qr_modal = false;
                            this.pending_tx_payload = None;
                            this.stop_camera(cx);
                        } else if this.show_staking_modal {
                            this.show_staking_modal = false;
                        } else if this.show_pool_modal {
                            this.show_pool_modal = false;
                        } else if this.show_help {
                            this.show_help = false;
                        } else if this.show_settings {
                            this.show_settings = false;
                        } else if this.show_logs {
                            this.show_logs = false;
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
        } else if self.show_logs {
            root = root.child(
                div()
                    .id("logs-overlay")
                    .w_full()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(LogsView::render(&self.log_buffer, cx)),
            );
        } else {
            root = root
                .child(self.render_sidebar(cx))
                .child(self.render_content(cx));
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
        rgba(0xffffffff)
    } else {
        theme.text_secondary
    };

    div()
        .id(SharedString::from(format!("network-{:?}", network)))
        .px_2()
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
                    this.staking_history.clear();
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
        rgba(0xffffffff)
    } else {
        theme.text_secondary
    };

    div()
        .id(SharedString::from(format!("mode-{:?}", mode)))
        .px_2()
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
                if this.connection_mode != mode {
                    this.connection_mode = mode;
                    this.save_config();
                    // Disconnect from current connection (keep chain_handle for future commands)
                    if this.connection_status == ConnectionStatus::Connected {
                        if let Some(ref handle) = this.chain_handle {
                            let handle = handle.clone();
                            cx.spawn(move |_: gpui::WeakEntity<StkoptApp>, _: &mut gpui::AsyncApp| async move {
                                let _ = handle.disconnect().await;
                            }).detach();
                        }
                        this.connection_status = ConnectionStatus::Disconnected;
                    }
                    cx.notify();
                }
            });
        })
}
