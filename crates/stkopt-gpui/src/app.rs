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
    /// QR payload hex string for signing
    pub qr_payload: Option<String>,
    /// Chain handle for async operations
    pub chain_handle: Option<crate::chain::ChainHandle>,
    /// Connection error message
    pub connection_error: Option<String>,
    /// Pending chain updates (shared with async tasks)
    pub pending_updates: Arc<Mutex<Vec<crate::chain::ChainUpdate>>>,
    /// Database service for caching
    pub db: crate::db_service::DbService,
    /// Shared log buffer
    pub log_buffer: Arc<crate::log::LogBuffer>,
    /// Whether log window is visible
    pub show_logs: bool,
    /// Saved accounts (address book)
    pub address_book: Vec<SavedAccount>,
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

    /// Convert to stkopt_core::Network
    pub fn to_core(&self) -> stkopt_core::Network {
        match self {
            Network::Polkadot => stkopt_core::Network::Polkadot,
            Network::Kusama => stkopt_core::Network::Kusama,
            Network::Westend => stkopt_core::Network::Westend,
        }
    }
}

/// Staking information for an account
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct StakingInfo {
    pub total_balance: u128,
    pub transferable: u128,
    pub bonded: u128,
    pub unbonding: u128,
    pub rewards_pending: u128,
    pub is_nominating: bool,
    pub nomination_count: usize,
}

/// Validator information
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ValidatorInfo {
    pub address: String,
    pub name: Option<String>,
    pub commission: f64,
    pub total_stake: u128,
    pub own_stake: u128,
    pub nominator_count: u32,
    pub apy: Option<f64>,
    pub blocked: bool,
}

/// Historical staking data point
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct HistoryPoint {
    pub era: u32,
    pub staked: u128,
    pub rewards: u128,
    pub apy: f64,
}

/// Nomination pool information
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PoolInfo {
    pub id: u32,
    pub name: String,
    pub state: PoolState,
    pub member_count: u32,
    pub total_bonded: u128,
    pub commission: Option<f64>,
}

/// Pool state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum PoolState {
    Open,
    Blocked,
    Destroying,
}

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
                state: if i % 10 == 9 { PoolState::Blocked } else { PoolState::Open },
                member_count: 50 + (i * 17 % 500) as u32,
                total_bonded: 100_000_000_000_000u128 + (i as u128 * 50_000_000_000_000),
                commission: if i % 3 == 0 { Some(5.0 + (i % 10) as f64) } else { None },
            }
        })
        .collect()
}

impl StkoptApp {
    pub fn new(cx: &mut Context<Self>, log_buffer: Arc<crate::log::LogBuffer>) -> Self {
        // Load saved config from disk
        let config = crate::persistence::load_config().unwrap_or_default();
        
        // Convert network config to app network
        let network = match config.network {
            crate::persistence::NetworkConfig::Polkadot => Network::Polkadot,
            crate::persistence::NetworkConfig::Kusama => Network::Kusama,
            crate::persistence::NetworkConfig::Westend => Network::Westend,
            crate::persistence::NetworkConfig::Custom => Network::Polkadot,
        };
        
        // Convert connection mode config
        let connection_mode = match config.connection_mode {
            crate::persistence::ConnectionModeConfig::Rpc => ConnectionMode::Rpc,
            crate::persistence::ConnectionModeConfig::LightClient => ConnectionMode::LightClient,
        };

        // Initialize database and chain worker
        let handle = crate::gpui_tokio::Tokio::handle(cx);
        let db = crate::db_service::DbService::new(handle.clone()).expect("Failed to initialize database");
        let (chain_handle, mut update_rx) = crate::chain::spawn_chain_worker(Some(db.clone()), handle);

        // Spawn chain update listener
        // We initialize pending_updates early to share with the listener
        let pending_updates = Arc::new(Mutex::new(Vec::new()));
        let listener_updates = pending_updates.clone();
        
        let mut async_cx = cx.to_async();
        cx.spawn(move |this: WeakEntity<Self>, _cx: &mut gpui::AsyncApp| async move {
            while let Some(update) = update_rx.recv().await {
                if let Ok(mut queue) = listener_updates.lock() {
                    queue.push(update);
                }
                // Notify the view to process updates in the next frame
                // We use process_pending_updates in render() to pick these up
                let _ = this.update(&mut async_cx, |_, cx: &mut Context<Self>| cx.notify());
            }
        }).detach();

        // Auto-connect if enabled
        if config.auto_connect {
            let handle = chain_handle.clone();
            let net = network; // Copy
            let use_light = connection_mode == ConnectionMode::LightClient;
            cx.spawn(move |_, _cx: &mut gpui::AsyncApp| async move {
                if let Err(e) = handle.connect(net.to_core(), use_light).await {
                    eprintln!("Failed to auto-connect: {}", e);
                }
            }).detach();
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
            pools: Vec::new(),
            show_settings: false,
            settings_theme: config.theme,
            settings_network: config.network,
            settings_connection_mode: config.connection_mode,
            settings_auto_connect: config.auto_connect,
            settings_show_testnets: config.show_testnets,
            show_help: false,
            optimization_result: None,
            qr_payload: None,
            chain_handle: Some(chain_handle),
            connection_error: None,
            pending_updates,
            db,
            log_buffer,
            show_logs: false,
            address_book: Vec::new(),
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
        cx.spawn(move |this: WeakEntity<Self>, _cx: &mut gpui::AsyncApp| async move {
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
        }).detach();

    }

    /// Process any pending chain updates. Called during render.
    pub fn process_pending_updates(&mut self, cx: &mut Context<Self>) {
        let updates: Vec<crate::chain::ChainUpdate> = {
            let mut pending = self.pending_updates.lock().unwrap();
            std::mem::take(&mut *pending)
        };
        
        for update in updates {
            self.apply_chain_update(update, cx);
        }
    }
    
    /// Apply a single chain update to the app state.
    fn apply_chain_update(&mut self, update: crate::chain::ChainUpdate, cx: &mut Context<Self>) {
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
                    unbonding: 0, // TODO: Get from chain
                    rewards_pending: 0, // TODO: Get from chain
                    is_nominating: account_data.is_nominating,
                    nomination_count: account_data.nominations.len(),
                });
                tracing::info!("Account data loaded: balance={}", account_data.free_balance);
            }
            ChainUpdate::HistoryLoaded(history) => {
                self.staking_history = history;
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
        };

        if let Err(e) = crate::persistence::save_config(&config) {
            eprintln!("Failed to save config: {}", e);
        }
    }

    /// Add an account to the address book if not already present.
    pub fn add_to_address_book(&mut self, address: String) {
        // Check if already exists
        if self.address_book.iter().any(|a| a.address == address && a.network == self.network) {
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
        self.address_book.retain(|a| a.address != address || a.network != self.network);
    }

    fn render_sidebar(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let entity = self.entity.clone();
        let current_section = self.current_section;

        let mut nav = div()
            .flex()
            .flex_col()
            .w(px(220.0))
            .min_w(px(220.0))
            .h_full()
            .bg(theme.surface)
            .border_r_1()
            .border_color(theme.border)
            .py_4();

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
                        .child(
                            Heading::h3("Staking Optimizer")
                                .into_any_element(),
                        ),
                )
                .child(
                    div()
                        .mt_2()
                        .flex()
                        .gap_1()
                        .child(network_pill("DOT", Network::Polkadot, self.network, &theme, entity.clone()))
                        .child(network_pill("KSM", Network::Kusama, self.network, &theme, entity.clone()))
                        .child(network_pill("WND", Network::Westend, self.network, &theme, entity.clone())),
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
            .child(mode_pill("Light", ConnectionMode::LightClient, self.connection_mode, &theme, entity.clone()))
            .child(mode_pill("RPC", ConnectionMode::Rpc, self.connection_mode, &theme, entity.clone()))
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
            .child(
                div()
                    .w(px(8.0))
                    .h(px(8.0))
                    .rounded_full()
                    .bg(status_color),
            )
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
            .flex_1()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(theme.background)
            .child(
                div()
                    .id("content-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .p_6()
                    .child(content),
            )
    }
}

impl Render for StkoptApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Process any pending chain updates
        self.process_pending_updates(cx);
        
        let theme = cx.theme();
        let entity = self.entity.clone();

        let mut root = div()
            .id("stkopt-root")
            .track_focus(&self.focus_handle)
            .w_full()
            .h_full()
            .bg(theme.background)
            .text_color(theme.text_primary)
            .flex();

        // Add keyboard handler for settings shortcut
        #[cfg(target_os = "macos")]
        {
            root = root.on_key_down({
                let entity = entity.clone();
                move |event, _window, cx| {
                    if event.keystroke.key == "," && event.keystroke.modifiers.platform {
                        entity.update(cx, |this, cx| {
                            this.show_settings = !this.show_settings;
                            cx.notify();
                        });
                    }
                    // Cmd+L to toggle logs
                    if event.keystroke.key == "l" && event.keystroke.modifiers.platform {
                        entity.update(cx, |this, cx| {
                            this.show_logs = !this.show_logs;
                            cx.notify();
                        });
                    }
                    // Escape to close settings/help/logs
                    if event.keystroke.key == "escape" {
                        entity.update(cx, |this, cx| {
                            if this.show_help {
                                this.show_help = false;
                                cx.notify();
                            } else if this.show_settings {
                                this.show_settings = false;
                                cx.notify();
                            } else if this.show_logs {
                                this.show_logs = false;
                                cx.notify();
                            }
                        });
                    }
                    // ? to toggle help
                    if event.keystroke.key == "?" || (event.keystroke.key == "/" && event.keystroke.modifiers.shift) {
                        entity.update(cx, |this, cx| {
                            this.show_help = !this.show_help;
                            cx.notify();
                        });
                    }
                }
            });
        }
        #[cfg(not(target_os = "macos"))]
        {
            root = root.on_key_down({
                let entity = entity.clone();
                move |event, _window, cx| {
                    if event.keystroke.key == "," && event.keystroke.modifiers.control {
                        entity.update(cx, |this, cx| {
                            this.show_settings = !this.show_settings;
                            cx.notify();
                        });
                    }
                    // Ctrl+L to toggle logs
                    if event.keystroke.key == "l" && event.keystroke.modifiers.control {
                        entity.update(cx, |this, cx| {
                            this.show_logs = !this.show_logs;
                            cx.notify();
                        });
                    }
                    // Escape to close settings/help/logs
                    if event.keystroke.key == "escape" {
                        entity.update(cx, |this, cx| {
                            if this.show_help {
                                this.show_help = false;
                                cx.notify();
                            } else if this.show_settings {
                                this.show_settings = false;
                                cx.notify();
                            } else if this.show_logs {
                                this.show_logs = false;
                                cx.notify();
                            }
                        });
                    }
                    // ? to toggle help
                    if event.keystroke.key == "?" || (event.keystroke.key == "/" && event.keystroke.modifiers.shift) {
                        entity.update(cx, |this, cx| {
                            this.show_help = !this.show_help;
                            cx.notify();
                        });
                    }
                }
            });
        }

        if self.show_help {
            root = root
                .child(self.render_sidebar(cx))
                .child(self.render_content(cx))
                .child(HelpOverlay::render(self, cx));
        } else if self.show_settings {
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
    let bg = if is_active { theme.accent } else { theme.surface_hover };
    let text_color = if is_active { rgba(0xffffffff) } else { theme.text_secondary };

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
                    // Disconnect and reconnect with new network
                    if this.connection_status == ConnectionStatus::Connected {
                        this.connection_status = ConnectionStatus::Disconnected;
                        this.chain_handle = None;
                    }
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
    let bg = if is_active { theme.accent } else { theme.surface_hover };
    let text_color = if is_active { rgba(0xffffffff) } else { theme.text_secondary };

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
                    // Disconnect and reconnect with new mode
                    if this.connection_status == ConnectionStatus::Connected {
                        this.connection_status = ConnectionStatus::Disconnected;
                        this.chain_handle = None;
                    }
                    cx.notify();
                }
            });
        })
}
