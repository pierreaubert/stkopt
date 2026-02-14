//! Staking Optimizer TUI - A terminal interface for Polkadot staking optimization.

mod action;
mod app;
mod config;
mod db;
mod event;
mod log_buffer;
mod qr_reader;
mod tcc;
mod theme;
mod tui;
mod ui;

use action::{AccountStatus, Action, PendingTransaction, TxSubmissionStatus};
use app::App;
use clap::Parser;
use color_eyre::Result;
use event::{Event, EventHandler};
use log_buffer::{LogBuffer, LogBufferLayer};
use ratatui::crossterm::event::KeyCode;
use stkopt_chain::{ChainClient, RewardDestination};
use stkopt_core::{ConnectionStatus, Network};
use tokio::sync::mpsc;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tui::Tui;

/// Staking operation to be performed by the chain task.
#[derive(Debug)]
enum StakingOp {
    // Direct staking
    Bond {
        signer: subxt::utils::AccountId32,
        value: u128,
    },
    Unbond {
        signer: subxt::utils::AccountId32,
        value: u128,
    },
    BondExtra {
        signer: subxt::utils::AccountId32,
        value: u128,
    },
    SetPayee {
        signer: subxt::utils::AccountId32,
        destination: RewardDestination,
    },
    WithdrawUnbonded {
        signer: subxt::utils::AccountId32,
    },
    Chill {
        signer: subxt::utils::AccountId32,
    },

    // Pool operations
    PoolJoin {
        signer: subxt::utils::AccountId32,
        pool_id: u32,
        amount: u128,
    },
    PoolBondExtra {
        signer: subxt::utils::AccountId32,
        amount: u128,
    },
    PoolClaim {
        signer: subxt::utils::AccountId32,
    },
    PoolUnbond {
        signer: subxt::utils::AccountId32,
        amount: u128,
    },
    PoolWithdraw {
        signer: subxt::utils::AccountId32,
    },
}

/// Unified request type for all chain operations.
/// Consolidates 5 separate channels into one.
#[derive(Debug)]
enum ChainRequest {
    /// Fetch account data.
    FetchAccount(subxt::utils::AccountId32),
    /// Generate QR code for nomination.
    GenerateNominationQR {
        signer: subxt::utils::AccountId32,
        targets: Vec<subxt::utils::AccountId32>,
    },
    /// Load staking history.
    FetchHistory {
        account: subxt::utils::AccountId32,
        num_eras: u32,
        cancel_rx: tokio::sync::watch::Receiver<bool>,
    },
    /// Execute a staking operation (generates QR).
    ExecuteStakingOp(StakingOp),
    /// Submit a signed transaction.
    SubmitTransaction(Vec<u8>),
}

/// Staking Optimizer TUI - Terminal interface for Polkadot staking optimization.
#[derive(Parser, Debug)]
#[command(name = "stkopt")]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Network to connect to
    #[arg(short, long, default_value = "polkadot")]
    network: NetworkArg,

    /// Custom Asset Hub RPC endpoint URL (for staking data queries)
    #[arg(long = "asset-hub-url")]
    asset_hub_url: Option<String>,

    /// Custom relay chain RPC endpoint URL (for staking transactions)
    #[arg(long = "relay-url")]
    relay_url: Option<String>,

    /// Custom People chain RPC endpoint URL (for identity data)
    #[arg(long = "people-url")]
    people_url: Option<String>,

    /// Update mode: fetch missing history data and store to database, then exit.
    /// Use with --address to specify which account to update.
    /// Suitable for running from cron jobs.
    #[arg(long)]
    update: bool,

    /// Account address to update history for (required with --update).
    #[arg(short, long, requires = "update")]
    address: Option<String>,

    /// Number of eras to fetch in update mode (default: 30)
    #[arg(long, default_value = "30")]
    eras: u32,

    /// Force RPC mode instead of light client.
    /// Light client is default (trustless) but RPC may be needed for
    /// historical data queries or when light client has issues.
    #[arg(long)]
    rpc: bool,
}

// Re-export connection types from stkopt_chain
use stkopt_chain::{ConnectionConfig, ConnectionMode, RpcEndpoints};

/// Network argument that can be parsed from string.
#[derive(Debug, Clone)]
struct NetworkArg(Network);

impl std::str::FromStr for NetworkArg {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "polkadot" | "dot" => Ok(NetworkArg(Network::Polkadot)),
            "kusama" | "ksm" => Ok(NetworkArg(Network::Kusama)),
            "westend" | "wnd" => Ok(NetworkArg(Network::Westend)),
            "paseo" | "pas" => Ok(NetworkArg(Network::Paseo)),
            _ => Err(format!(
                "Unknown network '{}'. Valid options: polkadot, kusama, westend, paseo",
                s
            )),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI arguments
    let args = Args::parse();

    // Initialize error handling
    color_eyre::install()?;

    // Create shared log buffer
    let log_buffer = LogBuffer::new();

    // Initialize logging - use stderr for update mode, buffer for TUI
    let env_filter = tracing_subscriber::EnvFilter::from_default_env()
        .add_directive("stkopt=info".parse()?)
        .add_directive("stkopt_chain=info".parse()?)
        .add_directive("stkopt_core=info".parse()?);

    if args.update {
        // In update mode, log to stderr so user can see progress
        tracing_subscriber::registry()
            .with(env_filter)
            .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
            .init();
    } else {
        // In TUI mode, log to buffer for display in UI
        tracing_subscriber::registry()
            .with(env_filter)
            .with(LogBufferLayer::new(log_buffer.clone()))
            .init();
    }

    let network = args.network.0;

    // Build connection configuration
    let connection_config = ConnectionConfig {
        mode: if args.rpc {
            ConnectionMode::Rpc
        } else {
            ConnectionMode::LightClient
        },
        rpc_endpoints: RpcEndpoints {
            asset_hub: args.asset_hub_url.clone(),
            relay: args.relay_url.clone(),
            people: args.people_url.clone(),
        },
    };

    // Handle update mode (batch mode for cron jobs)
    if args.update {
        return run_update_mode(network, connection_config.clone(), args.address, args.eras).await;
    }

    // Create action channel for responses from chain task to UI
    const ACTION_CHANNEL_CAPACITY: usize = 100;
    let (action_tx, mut action_rx) = mpsc::channel::<Action>(ACTION_CHANNEL_CAPACITY);

    // Create unified request channel for all chain operations
    const REQUEST_CHANNEL_CAPACITY: usize = 50;
    let (request_tx, request_rx) = mpsc::channel::<ChainRequest>(REQUEST_CHANNEL_CAPACITY);

    // Cancellation sender for history loading
    let (history_cancel_tx, history_cancel_rx) = tokio::sync::watch::channel(false);

    // Detect terminal theme (must be done before entering raw mode)
    let theme = theme::Theme::detect();

    // Load configuration
    let mut app_config = config::load_config().unwrap_or_default();
    tracing::info!("Loaded {} saved account(s)", app_config.accounts.len());

    // Create application state
    let mut app = App::new(network, log_buffer, theme);

    // Load cached data from database before chain connects
    let db_path = get_db_path();
    if let Ok(db) = db::HistoryDb::open(&db_path) {
        // Load cached validators
        if let Ok(cached_validators) = db.get_cached_validators(network)
            && !cached_validators.is_empty()
        {
            tracing::info!("Loaded {} cached validators", cached_validators.len());
            app.validators = cached_validators;
            app.loading.using_cache = true;
        }
    }

    // Load last saved account if available
    let restored_account = if let Some(last_addr) = app_config.last_account.as_deref()
        && let Ok(account) = last_addr.parse::<subxt::utils::AccountId32>()
    {
        tracing::info!("Restoring last used account: {}", last_addr);
        app.watched_account = Some(account.clone());

        // Load cached staking history for this account
        if let Ok(db) = db::HistoryDb::open(&db_path)
            && let Ok(cached_history) = db.get_history(network, last_addr, Some(30))
            && !cached_history.is_empty()
        {
            // Filter out cached entries with unrealistic APY (likely bad data)
            let filtered: Vec<_> = cached_history
                .into_iter()
                .filter(|h| h.apy <= 0.50)
                .collect();
            tracing::info!("Loaded {} cached history points", filtered.len());
            app.history.points = filtered;
            app.history.loaded_for = Some(last_addr.to_string());
        }

        Some(account)
    } else {
        // No saved account - show the account input prompt
        app.show_account_prompt = true;
        None
    };

    // Check and request camera permission on macOS
    #[cfg(target_os = "macos")]
    {
        tcc::print_camera_permission_status();
        if let Err(e) = tcc::ensure_camera_permission() {
            tracing::warn!(
                "Camera permission not available: {}. QR scanning may not work.",
                e
            );
        }
    }

    // Initialize terminal
    let mut tui = Tui::new()?;
    tui.enter()?;

    // Create event handler
    let mut events = EventHandler::new(50);

    // QR reader for scanning signatures from Vault
    let mut qr_reader: Option<qr_reader::QrReader> = None;

    // Spawn chain connection task
    let chain_action_tx = action_tx.clone();
    tokio::spawn(async move {
        chain_task(network, connection_config, chain_action_tx, request_rx).await;
    });

    // Send restored account request (will be processed once chain connects)
    if let Some(account) = restored_account {
        let _ = request_tx.send(ChainRequest::FetchAccount(account)).await;
    }

    // Main loop
    loop {
        // Render UI
        tui.draw(|frame| ui::render(frame, &mut app))?;

        // Handle events and actions
        tokio::select! {
            event = events.next() => {
                match event? {
                    Event::Tick => {
                        app.tick();

                        // Poll QR reader if scanning
                        if let Some(ref reader) = qr_reader
                            && let Some(result) = reader.try_recv()
                        {
                            match result {
                                qr_reader::QrScanResult::Success(data, preview) => {
                                    tracing::info!("QR code scanned: {} bytes", data.len());
                                    let _ = action_tx.send(Action::UpdateScanStatus(action::QrScanStatus::Success)).await;
                                    let _ = action_tx.send(Action::UpdateCameraPreview(
                                        preview.pixels,
                                        preview.width,
                                        preview.height,
                                        preview.qr_bounds,
                                    )).await;
                                    let _ = action_tx.send(Action::SignatureScanned(data)).await;
                                    // Stop scanning after successful scan
                                    if let Some(ref mut r) = qr_reader {
                                        r.stop();
                                    }
                                    qr_reader = None;
                                }
                                qr_reader::QrScanResult::Scanning(preview) => {
                                    // No QR detected, update status and preview for visual feedback
                                    let _ = action_tx.send(Action::UpdateScanStatus(action::QrScanStatus::Scanning)).await;
                                    let _ = action_tx.send(Action::UpdateCameraPreview(
                                        preview.pixels,
                                        preview.width,
                                        preview.height,
                                        preview.qr_bounds,
                                    )).await;
                                }
                                qr_reader::QrScanResult::Detected(preview) => {
                                    // QR detected but not decoded yet
                                    let _ = action_tx.send(Action::UpdateScanStatus(action::QrScanStatus::Detected)).await;
                                    let _ = action_tx.send(Action::UpdateCameraPreview(
                                        preview.pixels,
                                        preview.width,
                                        preview.height,
                                        preview.qr_bounds,
                                    )).await;
                                }
                                qr_reader::QrScanResult::Error(e) => {
                                    let _ = action_tx.send(Action::QrScanFailed(e)).await;
                                    qr_reader = None;
                                }
                            }
                        }
                    }
                    Event::Key(key_event) => {
                        if key_event.code == KeyCode::Char('q') && app.input_mode == app::InputMode::Normal {
                            break;
                        }
                        if let Some(action) = app.handle_key(key_event) {
                            let _ = action_tx.send(action).await;
                        }
                    }
                    Event::Resize(_, _) => {
                        // Terminal resize is handled automatically by ratatui
                    }
                }
            }
            Some(action) = action_rx.recv() => {
                // Handle special actions
                match &action {
                    Action::SetWatchedAccount(account, original_addr) => {
                        let _ = request_tx.send(ChainRequest::FetchAccount(account.clone())).await;
                        // Save account to config with original address string (preserves user's SS58 format)
                        app_config.last_account = Some(original_addr.clone());
                        app_config.add_account(
                            original_addr.clone(),
                            None,
                            Some(network.to_string()),
                        );
                        if let Err(e) = config::save_config(&app_config) {
                            tracing::warn!("Failed to save config: {}", e);
                        }
                        // Auto-load staking history in background
                        let _ = history_cancel_tx.send(false);
                        let _ = request_tx.send(ChainRequest::FetchHistory {
                            account: account.clone(),
                            num_eras: app.history.total_eras,
                            cancel_rx: history_cancel_rx.clone(),
                        }).await;
                        // Mark history as loading (will be handled by LoadStakingHistory action in app)
                        let _ = action_tx.send(Action::LoadStakingHistory).await;
                    }
                    Action::RunOptimization => {
                        // Run optimization with default strategy (TopApy)
                        let candidates: Vec<_> = app.validators.iter().map(|v| {
                            stkopt_core::ValidatorCandidate {
                                address: v.address.clone(),
                                commission: v.commission,
                                blocked: v.blocked,
                                apy: v.apy.unwrap_or(0.0),
                                total_stake: v.total_stake,
                                nominator_count: v.nominator_count,
                            }
                        }).collect();

                        let criteria = stkopt_core::OptimizationCriteria::default();
                        let result = stkopt_core::select_validators(&candidates, &criteria);
                        let _ = action_tx.send(Action::SetOptimizationResult(result)).await;
                    }
                    Action::RunOptimizationWithStrategy(strategy_idx) => {
                        // Run optimization with selected strategy
                        let candidates: Vec<_> = app.validators.iter().map(|v| {
                            stkopt_core::ValidatorCandidate {
                                address: v.address.clone(),
                                commission: v.commission,
                                blocked: v.blocked,
                                apy: v.apy.unwrap_or(0.0),
                                total_stake: v.total_stake,
                                nominator_count: v.nominator_count,
                            }
                        }).collect();

                        let strategy = match strategy_idx {
                            0 => stkopt_core::SelectionStrategy::TopApy,
                            1 => stkopt_core::SelectionStrategy::RandomFromTop,
                            2 => stkopt_core::SelectionStrategy::DiversifyByStake,
                            _ => stkopt_core::SelectionStrategy::TopApy,
                        };

                        let criteria = stkopt_core::OptimizationCriteria {
                            strategy,
                            ..stkopt_core::OptimizationCriteria::default()
                        };
                        let result = stkopt_core::select_validators(&candidates, &criteria);
                        let _ = action_tx.send(Action::SetOptimizationResult(result)).await;
                    }
                    Action::GenerateBondQR { value } => {
                        if let Some(account) = &app.watched_account {
                            let _ = request_tx
                                .send(ChainRequest::ExecuteStakingOp(StakingOp::Bond {
                                    signer: account.clone(),
                                    value: *value,
                                }))
                                .await;
                        }
                    }
                    Action::GenerateUnbondQR { value } => {
                        if let Some(account) = &app.watched_account {
                            let _ = request_tx
                                .send(ChainRequest::ExecuteStakingOp(StakingOp::Unbond {
                                    signer: account.clone(),
                                    value: *value,
                                }))
                                .await;
                        }
                    }
                    Action::GenerateBondExtraQR { value } => {
                        if let Some(account) = &app.watched_account {
                            let _ = request_tx
                                .send(ChainRequest::ExecuteStakingOp(StakingOp::BondExtra {
                                    signer: account.clone(),
                                    value: *value,
                                }))
                                .await;
                        }
                    }
                    Action::GenerateSetPayeeQR { destination } => {
                        if let Some(account) = &app.watched_account {
                            let _ = request_tx
                                .send(ChainRequest::ExecuteStakingOp(StakingOp::SetPayee {
                                    signer: account.clone(),
                                    destination: destination.clone(),
                                }))
                                .await;
                        }
                    }
                    Action::GenerateWithdrawUnbondedQR => {
                        if let Some(account) = &app.watched_account {
                            let _ = request_tx
                                .send(ChainRequest::ExecuteStakingOp(StakingOp::WithdrawUnbonded {
                                    signer: account.clone(),
                                }))
                                .await;
                        }
                    }
                    Action::GenerateChillQR => {
                        if let Some(account) = &app.watched_account {
                            let _ = request_tx
                                .send(ChainRequest::ExecuteStakingOp(StakingOp::Chill {
                                    signer: account.clone(),
                                }))
                                .await;
                        }
                    }
                    Action::GeneratePoolJoinQR { pool_id, amount } => {
                        if let Some(account) = &app.watched_account {
                            let _ = request_tx
                                .send(ChainRequest::ExecuteStakingOp(StakingOp::PoolJoin {
                                    signer: account.clone(),
                                    pool_id: *pool_id,
                                    amount: *amount,
                                }))
                                .await;
                        }
                    }
                    Action::GeneratePoolBondExtraQR { amount } => {
                        if let Some(account) = &app.watched_account {
                            let _ = request_tx
                                .send(ChainRequest::ExecuteStakingOp(StakingOp::PoolBondExtra {
                                    signer: account.clone(),
                                    amount: *amount,
                                }))
                                .await;
                        }
                    }
                    Action::GeneratePoolUnbondQR { amount } => {
                        if let Some(account) = &app.watched_account {
                            let _ = request_tx
                                .send(ChainRequest::ExecuteStakingOp(StakingOp::PoolUnbond {
                                    signer: account.clone(),
                                    amount: *amount,
                                }))
                                .await;
                        }
                    }
                    Action::GeneratePoolClaimQR => {
                        if let Some(account) = &app.watched_account {
                            let _ = request_tx
                                .send(ChainRequest::ExecuteStakingOp(StakingOp::PoolClaim {
                                    signer: account.clone(),
                                }))
                                .await;
                        }
                    }
                    Action::GeneratePoolWithdrawQR => {
                        if let Some(account) = &app.watched_account {
                            let _ = request_tx
                                .send(ChainRequest::ExecuteStakingOp(StakingOp::PoolWithdraw {
                                    signer: account.clone(),
                                }))
                                .await;
                        }
                    }
                    Action::GenerateNominationQR => {
                        // Get selected validator addresses
                        if let Some(account) = &app.watched_account {
                            let targets: Vec<subxt::utils::AccountId32> = app
                                .selected_validators
                                .iter()
                                .filter_map(|&idx| {
                                    app.validators.get(idx).and_then(|v| {
                                        use std::str::FromStr;
                                        subxt::utils::AccountId32::from_str(&v.address).ok()
                                    })
                                })
                                .collect();

                            if !targets.is_empty() {
                                let _ = request_tx.send(ChainRequest::GenerateNominationQR {
                                    signer: account.clone(),
                                    targets,
                                }).await;
                            }
                        }
                    }
                    Action::SignatureScanned(signature_data) => {
                        // Log what we received for debugging
                        tracing::info!(
                            "Received QR data: {} bytes, first 20: {:02x?}",
                            signature_data.len(),
                            &signature_data[..signature_data.len().min(20)]
                        );
                        // Decode the signature from Vault's QR code
                        match stkopt_chain::decode_vault_signature(signature_data) {
                            Ok(decoded_sig) => {
                                if let Some(ref pending) = app.qr.pending_unsigned {
                                    tracing::info!(
                                        "Decoded {:?} signature from Vault",
                                        decoded_sig.sig_type
                                    );

                                    // Build the signed extrinsic
                                    let signed = stkopt_chain::build_signed_extrinsic(
                                        &pending.payload,
                                        &pending.signer,
                                        &decoded_sig,
                                    );

                                    tracing::info!(
                                        "Extrinsic built: 0x{} ({} bytes)",
                                        hex::encode(signed.hash),
                                        signed.encoded.len()
                                    );

                                    // Store the pending transaction
                                    app.qr.pending_signed = Some(PendingTransaction {
                                        description: signed.description.clone(),
                                        signed_extrinsic: signed.encoded,
                                        tx_hash: signed.hash,
                                        status: TxSubmissionStatus::ReadyToSubmit,
                                    });

                                    // Stop camera and clear scanning state
                                    if let Some(ref mut reader) = qr_reader {
                                        reader.stop();
                                    }
                                    qr_reader = None;
                                    app.camera.scanning = false;
                                    app.camera.status = None;
                                    app.camera.preview = None;
                                    app.camera.qr_bounds = None;

                                    // Switch to Submit tab (tab 3)
                                    app.qr.modal_tab = 3;
                                } else {
                                    tracing::error!("Signature received but no pending unsigned tx");
                                }
                            }
                            Err(e) => {
                                tracing::error!("Failed to decode signature: {}", e);
                            }
                        }
                    }
                    Action::SubmitTransaction => {
                        // Submit the signed transaction via chain task
                        if let Some(ref pending_tx) = app.qr.pending_signed {
                            let extrinsic = pending_tx.signed_extrinsic.clone();
                            let _ = action_tx.send(Action::SetTxStatus(TxSubmissionStatus::Submitting)).await;

                            tracing::info!(
                                "Submitting transaction: 0x{} ({} bytes)",
                                hex::encode(pending_tx.tx_hash),
                                extrinsic.len()
                            );

                            // Send to chain task for submission
                            let _ = request_tx.send(ChainRequest::SubmitTransaction(extrinsic)).await;
                        }
                    }
                    Action::StartSignatureScan => {
                        // Start camera capture for QR scanning
                        match qr_reader::QrReader::new() {
                            Ok(reader) => {
                                qr_reader = Some(reader);
                                app.camera.scanning = true;
                                tracing::info!("Started camera for signature QR scanning");
                            }
                            Err(e) => {
                                tracing::error!("Failed to start camera: {}", e);
                                let _ = action_tx.send(Action::QrScanFailed(e)).await;
                            }
                        }
                    }
                    Action::StopSignatureScan => {
                        // Stop the QR reader
                        if let Some(ref mut reader) = qr_reader {
                            reader.stop();
                        }
                        qr_reader = None;
                        // Already handled in app.handle_action
                    }
                    Action::ClearPendingTx => {
                        // Stop the QR reader if active
                        if let Some(ref mut reader) = qr_reader {
                            reader.stop();
                        }
                        qr_reader = None;
                        // Already handled in app.handle_action
                    }
                    Action::QrScanFailed(_) => {
                        // Cleanup QR reader
                        qr_reader = None;
                    }
                    Action::LoadStakingHistory => {
                        if let Some(account) = &app.watched_account {
                            // Reset cancellation flag (watch channel - sync send)
                            let _ = history_cancel_tx.send(false);
                            let _ = request_tx.send(ChainRequest::FetchHistory {
                                account: account.clone(),
                                num_eras: app.history.total_eras,
                                cancel_rx: history_cancel_rx.clone(),
                            }).await;
                        }
                    }
                    Action::CancelLoadingHistory => {
                        // Signal cancellation
                        let _ = history_cancel_tx.send(true);
                    }
                    Action::SelectAddressBookEntry(idx) => {
                        let idx = *idx;
                        tracing::info!("[ADDR] SelectAddressBookEntry idx={}, watched={}", idx, app.watched_account.is_some());

                        // Known addresses matching the hardcoded list in ui.rs
                        let known_addresses: &[(&str, &str)] = &[
                            ("Polkadot Treasury", "13UVJyLnbVp9RBZYFwCNuGnK87JYJ2nb7jMwaVe4vQ2UNCzN"),
                            ("Polkadot Fellowship", "16SpacegeUTft9v3ts27CEC3tJaxgvE4uZeCctThFH3Vb24p"),
                            ("Snowbridge", "13cKp89Nt7t1hZVWnqhKW9LY7Udhxk2BmLwKi3snVgUAjZGE"),
                        ];

                        // Determine the actual index (accounting for "My Account")
                        let known_idx = if app.watched_account.is_some() {
                            if idx == 0 {
                                tracing::info!("[ADDR] My Account selected, skipping");
                                continue;
                            }
                            idx - 1
                        } else {
                            idx
                        };

                        tracing::info!("[ADDR] known_idx={}, known_addresses.len()={}", known_idx, known_addresses.len());
                        if let Some((name, addr)) = known_addresses.get(known_idx) {
                            use std::str::FromStr;
                            tracing::info!("[ADDR] Selected: {} ({})", name, addr);
                            if let Ok(account) = subxt::utils::AccountId32::from_str(addr) {
                                let _ = action_tx.send(Action::SetWatchedAccount(account, addr.to_string())).await;
                            }
                        } else {
                            tracing::warn!("[ADDR] known_idx {} out of bounds!", known_idx);
                        }
                    }
                    Action::RemoveAccount(address) => {
                        // Remove account from config
                        app_config.remove_account(address);
                        if let Err(e) = config::save_config(&app_config) {
                            tracing::warn!("Failed to save config after removing account: {}", e);
                        }

                        // Purge history from database
                        let db_path = get_db_path();
                        if let Ok(db) = db::HistoryDb::open(&db_path) {
                            match db.delete_address_history(address) {
                                Ok(deleted) => {
                                    tracing::info!(
                                        "Removed account {} and purged {} history entries",
                                        address, deleted
                                    );
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "Failed to purge history for {}: {}",
                                        address, e
                                    );
                                }
                            }
                        }
                    }
                    _ => {}
                }
                app.handle_action(action);
            }
        }

        if app.should_quit {
            break;
        }
    }

    // Restore terminal
    tui.exit()?;

    Ok(())
}

/// Build TransactionInfo from a payload for display purposes.
fn build_tx_info(
    payload: &stkopt_chain::UnsignedPayload,
    signer: &subxt::utils::AccountId32,
    targets: Vec<String>,
) -> crate::action::TransactionInfo {
    crate::action::TransactionInfo {
        signer: signer.to_string(),
        call: payload.description.clone(),
        targets,
        call_data_size: payload.call_data.len(),
        spec_version: payload.spec_version,
        tx_version: payload.tx_version,
        nonce: payload.nonce,
        include_metadata_hash: payload.include_metadata_hash,
    }
}

/// Send QR data and pending transaction info to the UI.
async fn send_staking_qr(
    action_tx: &mpsc::Sender<Action>,
    payload: stkopt_chain::UnsignedPayload,
    signer: subxt::utils::AccountId32,
    targets: Vec<String>,
) {
    let qr_data = stkopt_chain::encode_for_qr(&payload, &signer);
    let tx_info = build_tx_info(&payload, &signer, targets);
    let pending = crate::action::PendingUnsignedTx { payload, signer };
    let _ = action_tx
        .send(Action::SetPendingUnsignedTx(Some(pending)))
        .await;
    let _ = action_tx
        .send(Action::SetQRData(Some(qr_data), Some(tx_info)))
        .await;
}

/// Clear QR data and pending transaction info from the UI.
async fn clear_staking_qr(action_tx: &mpsc::Sender<Action>) {
    let _ = action_tx.send(Action::SetPendingUnsignedTx(None)).await;
    let _ = action_tx.send(Action::SetQRData(None, None)).await;
}

/// Background task for chain operations.
async fn chain_task(
    network: Network,
    config: ConnectionConfig,
    action_tx: mpsc::Sender<Action>,
    mut request_rx: mpsc::Receiver<ChainRequest>,
) {
    use crate::action::{DisplayPool, DisplayValidator};
    use std::collections::HashMap;
    use stkopt_core::get_era_apy;

    // Create status channel for connection updates (bounded for backpressure)
    const STATUS_CHANNEL_CAPACITY: usize = 10;
    let (status_tx, mut status_rx) = mpsc::channel::<ConnectionStatus>(STATUS_CHANNEL_CAPACITY);

    // Forward status updates to action channel
    let action_tx_for_status = action_tx.clone();
    tokio::spawn(async move {
        while let Some(status) = status_rx.recv().await {
            let _ = action_tx_for_status
                .send(Action::UpdateConnectionStatus(status))
                .await;
        }
    });

    // Connect to chain (light client or RPC based on config)
    let mut client = match ChainClient::connect(network, &config, status_tx).await {
        Ok(client) => {
            tracing::info!(
                "Connected to {} Asset Hub via {} (genesis: {:?})",
                network,
                client.connection_mode(),
                client.genesis_hash()
            );
            client
        }
        Err(e) => {
            tracing::error!("Failed to connect to Asset Hub: {}", e);
            let _ = action_tx
                .send(Action::UpdateConnectionStatus(ConnectionStatus::Error(
                    e.to_string(),
                )))
                .await;
            return;
        }
    };

    // Helper to attempt reconnection
    async fn try_reconnect(client: &ChainClient, max_attempts: u32) -> Option<ChainClient> {
        for attempt in 1..=max_attempts {
            tracing::info!("Reconnection attempt {}/{}...", attempt, max_attempts);
            match client.reconnect().await {
                Ok(new_client) => {
                    tracing::info!("Reconnected successfully!");
                    return Some(new_client);
                }
                Err(e) => {
                    tracing::warn!("Reconnection failed: {} - waiting before retry...", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }
        }
        None
    }

    // Send chain info for UI display and validation
    let chain_info = client.get_chain_info();
    let _ = action_tx.send(Action::SetChainInfo(chain_info)).await;

    // Connect to People chain for identity queries
    // Uses light client if main connection is via light client, otherwise RPC
    let people_client = match client.connect_people_chain().await {
        Ok(subxt_client) => {
            tracing::info!("Connected to {} People chain", network);
            Some(stkopt_chain::PeopleChainClient::new(subxt_client))
        }
        Err(e) => {
            tracing::warn!(
                "Failed to connect to People chain (identities unavailable): {}",
                e
            );
            None
        }
    };

    // Longer delay to let light client connection stabilize
    // Light clients need time to sync state after initial connection
    if client.is_light_client() {
        tracing::info!("Waiting for light client to stabilize...");
        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
    } else {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }

    // Fetch era info (retry a few times as light client may need time to sync state)
    let era_info = {
        let mut era_result = None;
        for attempt in 1..=10 {
            match client.get_active_era().await {
                Ok(Some(info)) => {
                    let _ = action_tx.send(Action::SetActiveEra(info.clone())).await;
                    era_result = Some(info);
                    break;
                }
                Ok(None) => {
                    tracing::info!("Waiting for era data (attempt {}/10)...", attempt);
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                }
                Err(e) => {
                    tracing::error!("Failed to get active era: {}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                }
            }
        }
        match era_result {
            Some(info) => info,
            None => {
                tracing::error!("Could not fetch active era after 10 attempts");
                return;
            }
        }
    };

    // Fetch era duration
    let era_duration_ms = match client.get_era_duration_ms().await {
        Ok(duration) => {
            let _ = action_tx.send(Action::SetEraDuration(duration)).await;
            duration
        }
        Err(e) => {
            tracing::error!("Failed to get era duration: {}", e);
            86_400_000 // Default to 24 hours
        }
    };

    // Track bytes transferred for bandwidth calculation
    // Estimates: era info ~1KB, validator ~200B, exposure ~100B, identity ~100B, pool ~150B
    let mut bytes_transferred: u64 = 1000; // Era info ~1KB

    let _ = action_tx
        .send(Action::SetLoadingProgress(
            0.1,
            Some(bytes_transferred),
            None,
        ))
        .await;

    // Open database for caching
    let db_path = get_db_path();
    let mut db = db::HistoryDb::open(&db_path).ok();

    // Load cached identities immediately (fast, from local database)
    let mut identity_map: HashMap<String, String> = if let Some(ref db) = db {
        match db.get_validator_identities(network) {
            Ok(cached) => {
                if !cached.is_empty() {
                    tracing::info!("Loaded {} cached validator identities", cached.len());
                }
                cached
            }
            Err(e) => {
                tracing::debug!("Failed to load cached identities: {}", e);
                HashMap::new()
            }
        }
    } else {
        HashMap::new()
    };

    // Fetch validators (use light-client-friendly approach when in light client mode)
    let validators = {
        let mut result = None;
        let mut reconnect_attempts = 0;
        const MAX_RECONNECT_ATTEMPTS: u32 = 3;

        'outer: loop {
            // For light client, use the multi-source approach that handles partial data better
            let fetch_result = if client.is_light_client() {
                tracing::info!("Fetching validators via light client (multi-source approach)...");
                client.get_validators_light_client().await
            } else {
                // RPC mode can use direct iteration
                client.get_validators().await
            };

            match fetch_result {
                Ok(v) => {
                    if client.is_light_client() {
                        tracing::info!(
                            "Light client: Found {} validators (partial data - light clients have iteration limits)",
                            v.len()
                        );
                    } else {
                        tracing::info!("Found {} registered validators", v.len());
                    }
                    result = Some(v);
                    break 'outer;
                }
                Err(e) => {
                    tracing::warn!("Failed to get validators: {}", e);
                }
            }

            // Try to reconnect
            reconnect_attempts += 1;
            if reconnect_attempts > MAX_RECONNECT_ATTEMPTS {
                tracing::error!(
                    "Could not fetch validators after {} reconnection attempts",
                    MAX_RECONNECT_ATTEMPTS
                );
                break;
            }

            tracing::warn!(
                "Connection appears unstable - attempting reconnection ({}/{})",
                reconnect_attempts,
                MAX_RECONNECT_ATTEMPTS
            );
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

            if let Some(new_client) = try_reconnect(&client, 3).await {
                client = new_client;
                // Update chain info after reconnect
                let chain_info = client.get_chain_info();
                let _ = action_tx.send(Action::SetChainInfo(chain_info)).await;
            } else {
                tracing::error!("Reconnection failed - cannot continue");
                break;
            }
        }

        match result {
            Some(v) => v,
            None => {
                tracing::error!("Could not fetch validators - cannot continue");
                return;
            }
        }
    };

    bytes_transferred += validators.len() as u64 * 200; // ~200 bytes per validator
    let _ = action_tx
        .send(Action::SetLoadingProgress(
            0.3,
            Some(bytes_transferred),
            None,
        ))
        .await;

    // Fetch staker exposures for the previous era (active era - 1)
    // Retry with backoff for light client stability
    let query_era = era_info.index.saturating_sub(1);
    let exposures = {
        let mut result = None;
        let max_attempts = if client.is_light_client() { 10 } else { 3 };
        for attempt in 1..=max_attempts {
            match client.get_era_stakers_overview(query_era).await {
                Ok(e) => {
                    tracing::info!("Found {} active validators for era {}", e.len(), query_era);
                    result = Some(e);
                    break;
                }
                Err(e) => {
                    if attempt < max_attempts {
                        let delay = (attempt as u64).min(10);
                        tracing::warn!(
                            "Failed to get era stakers (attempt {}/{}): {} - retrying in {}s...",
                            attempt,
                            max_attempts,
                            e,
                            delay
                        );
                        tokio::time::sleep(tokio::time::Duration::from_secs(delay)).await;
                    } else {
                        tracing::warn!(
                            "Failed to get era stakers after {} attempts: {}",
                            max_attempts,
                            e
                        );
                    }
                }
            }
        }
        result.unwrap_or_default()
    };

    bytes_transferred += exposures.len() as u64 * 100; // ~100 bytes per exposure
    let _ = action_tx
        .send(Action::SetLoadingProgress(
            0.6,
            Some(bytes_transferred),
            None,
        ))
        .await;

    // Fetch era reward
    let era_reward = match client.get_era_validator_reward(query_era).await {
        Ok(Some(r)) => {
            tracing::info!("Era {} reward: {}", query_era, r);
            r
        }
        Ok(None) => {
            tracing::info!("No reward data for era {}", query_era);
            0
        }
        Err(e) => {
            tracing::error!("Failed to get era reward: {}", e);
            0
        }
    };

    // Fetch era reward points
    let (total_points, validator_points) = match client.get_era_reward_points(query_era).await {
        Ok((total, points)) => {
            tracing::info!(
                "Fetched points for {} validators (total: {})",
                points.len(),
                total
            );
            (total, points)
        }
        Err(e) => {
            tracing::warn!("Failed to fetch reward points: {}", e);
            (0, Vec::new())
        }
    };

    // Map points for quick lookup
    let points_len = validator_points.len();
    let points_map: HashMap<[u8; 32], u32> = validator_points
        .into_iter()
        .map(|vp| (*vp.address.as_ref(), vp.points))
        .collect();

    bytes_transferred += 500 + points_len as u64 * 50; // Era reward ~500B + ~50 bytes per point entry
    let _ = action_tx
        .send(Action::SetLoadingProgress(
            0.7,
            Some(bytes_transferred),
            None,
        ))
        .await;

    // Fetch fresh validator identities from People chain and update cache
    if let Some(ref people) = people_client {
        let addresses: Vec<subxt::utils::AccountId32> =
            validators.iter().map(|v| v.address.clone()).collect();

        tracing::info!(
            "Fetching identities for {} validators from People chain...",
            addresses.len()
        );

        match people.get_identities(&addresses).await {
            Ok(identities) => {
                let fresh_identities: HashMap<String, String> = identities
                    .into_iter()
                    .filter_map(|id| id.display_name.map(|name| (id.address.to_string(), name)))
                    .collect();

                let with_names = fresh_identities.len();
                tracing::info!(
                    "Found {} validators with display names from People chain",
                    with_names
                );

                // Update cache with fresh data
                if let Some(ref mut db) = db {
                    match db.set_validator_identities_batch(network, &fresh_identities) {
                        Ok(count) => {
                            tracing::info!("Updated {} cached validator identities", count);
                        }
                        Err(e) => {
                            tracing::debug!("Failed to update identity cache: {}", e);
                        }
                    }
                }

                // Merge fresh identities into our map (fresh data takes precedence)
                identity_map.extend(fresh_identities);
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to fetch identities from People chain: {} (using cached data)",
                    e
                );
                // Keep using cached identities
            }
        }
    } else if identity_map.is_empty() {
        tracing::info!("Skipping identity fetch (People chain not connected, no cache)");
    } else {
        tracing::info!(
            "Using {} cached identities (People chain not connected)",
            identity_map.len()
        );
    }

    bytes_transferred += identity_map.len() as u64 * 100; // ~100 bytes per identity
    let _ = action_tx
        .send(Action::SetLoadingProgress(
            0.8,
            Some(bytes_transferred),
            None,
        ))
        .await;

    // Build exposure map for quick lookup
    let exposure_map: HashMap<[u8; 32], _> = exposures
        .iter()
        .map(|e| (*e.address.as_ref(), e.clone()))
        .collect();

    // Build display validators
    let mut display_validators: Vec<DisplayValidator> = validators
        .iter()
        .filter_map(|v| {
            let addr_bytes: [u8; 32] = *v.address.as_ref();
            let exposure = exposure_map.get(&addr_bytes);

            // Only show validators that were active in the queried era
            let (total_stake, own_stake, nominator_count) = match exposure {
                Some(e) => (e.total, e.own, e.nominator_count),
                None => return None, // Skip validators not active in this era
            };

            let points = points_map.get(&addr_bytes).copied().unwrap_or(0);

            // Calculate APY based on era points (rewards distributed proportionally to points)
            let validator_share = if total_points > 0 && points > 0 {
                (era_reward as f64 * points as f64 / total_points as f64) as u128
            } else {
                0
            };
            let nominator_reward =
                ((validator_share as f64) * (1.0 - v.preferences.commission)) as u128;
            let apy = if total_stake > 0 {
                get_era_apy(nominator_reward, total_stake, era_duration_ms)
            } else {
                0.0
            };

            let address_str = v.address.to_string();
            let name = identity_map.get(&address_str).cloned();

            Some(DisplayValidator {
                address: address_str,
                name,
                commission: v.preferences.commission,
                blocked: v.preferences.blocked,
                total_stake,
                own_stake,
                nominator_count,
                points,
                apy: Some(apy),
            })
        })
        .collect();

    // Sort by APY descending
    display_validators.sort_by(|a, b| {
        b.apy
            .unwrap_or(0.0)
            .partial_cmp(&a.apy.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Build validator APY map for pool APY calculation (before sending)
    let validator_apy_map: HashMap<String, f64> = display_validators
        .iter()
        .filter_map(|v| v.apy.map(|apy| (v.address.clone(), apy)))
        .collect();

    tracing::info!(
        "Built validator APY map with {} entries for pool APY calculation",
        validator_apy_map.len()
    );

    let _ = action_tx
        .send(Action::SetLoadingProgress(
            0.9,
            Some(bytes_transferred),
            None,
        ))
        .await;

    // Cache validators to database for faster startup next time
    if let Some(ref mut db) = db {
        match db.set_cached_validators(network, era_info.index, &display_validators) {
            Ok(count) => {
                tracing::info!("Cached {} validators for era {}", count, era_info.index);
            }
            Err(e) => {
                tracing::warn!("Failed to cache validators: {}", e);
            }
        }
    }

    let _ = action_tx
        .send(Action::SetDisplayValidators(display_validators))
        .await;

    tracing::info!("Validator data loaded successfully");

    // Stabilize connection before pool queries
    // Light clients can become unstable after heavy validator query phase
    if client.is_light_client() {
        tracing::info!("Stabilizing light client before pool queries...");
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Verify connection is still healthy, reconnect if needed
        if !client.is_connected().await {
            tracing::warn!("Light client disconnected after validator loading, reconnecting...");
            if let Some(new_client) = try_reconnect(&client, 3).await {
                client = new_client;
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            }
        }
    }

    // Fetch nomination pools with retry logic
    let pools = {
        let max_attempts = if client.is_light_client() { 3 } else { 2 };
        let mut result = Vec::new();
        for attempt in 1..=max_attempts {
            match client.get_nomination_pools().await {
                Ok(p) => {
                    tracing::info!("Found {} nomination pools", p.len());
                    result = p;
                    break;
                }
                Err(e) => {
                    if attempt < max_attempts {
                        tracing::warn!(
                            "Pool query failed (attempt {}/{}): {} - retrying...",
                            attempt,
                            max_attempts,
                            e
                        );
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                        if client.is_light_client()
                            && !client.is_connected().await
                            && let Some(new_client) = try_reconnect(&client, 2).await
                        {
                            client = new_client;
                            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                        }
                    } else if client.is_light_client() {
                        tracing::warn!(
                            "Nomination pools unavailable (light client limitation): {}",
                            e
                        );
                    } else {
                        tracing::warn!("Failed to get nomination pools: {}", e);
                    }
                }
            }
        }
        result
    };

    // Fetch pool metadata for names with retry logic
    let metadata = {
        let max_attempts = if client.is_light_client() { 3 } else { 2 };
        let mut result = Vec::new();
        for attempt in 1..=max_attempts {
            match client.get_pool_metadata().await {
                Ok(m) => {
                    result = m;
                    break;
                }
                Err(e) => {
                    if attempt < max_attempts {
                        tracing::warn!(
                            "Pool metadata query failed (attempt {}/{}): {} - retrying...",
                            attempt,
                            max_attempts,
                            e
                        );
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                        if client.is_light_client()
                            && !client.is_connected().await
                            && let Some(new_client) = try_reconnect(&client, 2).await
                        {
                            client = new_client;
                            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                        }
                    } else if client.is_light_client() {
                        tracing::warn!(
                            "Pool metadata unavailable (light client limitation): {}",
                            e
                        );
                    } else {
                        tracing::warn!("Failed to get pool metadata: {}", e);
                    }
                }
            }
        }
        result
    };

    // Build metadata map for name lookup
    let metadata_map: HashMap<u32, String> = metadata.into_iter().map(|m| (m.id, m.name)).collect();
    tracing::info!(
        "Built pool metadata map with {} entries (pool IDs: {:?})",
        metadata_map.len(),
        metadata_map.keys().take(10).collect::<Vec<_>>()
    );

    // Build display pools with APY calculation
    // Note: We fetch nominations in batches to avoid overwhelming the RPC
    let mut display_pools: Vec<DisplayPool> = Vec::with_capacity(pools.len());

    // First pass: build pools with names only (no RPC calls)
    for p in &pools {
        let name = metadata_map.get(&p.id).cloned().unwrap_or_default();
        display_pools.push(DisplayPool {
            id: p.id,
            name,
            state: p.state.into(),
            member_count: p.member_count,
            total_bonded: p.points,
            commission: None,
            apy: None, // Will be filled in second pass
        });
    }

    // Send pools immediately so UI shows them (without APY)
    let _ = action_tx
        .send(Action::SetDisplayPools(display_pools.clone()))
        .await;
    tracing::info!(
        "Sent {} pools to UI (fetching APY in background)",
        display_pools.len()
    );

    // Second pass: fetch APY for top pools only (limit RPC calls)
    let max_pools_to_query = 30.min(pools.len()); // Reduced to avoid connection issues
    for (idx, p) in pools.iter().take(max_pools_to_query).enumerate() {
        // Small delay to avoid overwhelming RPC endpoint
        if idx > 0 && idx % 5 == 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        // Fetch nominations with retry logic for light client failures
        let mut nominations_result = None;
        for attempt in 0..3 {
            match client.get_pool_nominations(p.id).await {
                Ok(noms) => {
                    nominations_result = Some(Ok(noms));
                    break;
                }
                Err(e) => {
                    if attempt < 2 {
                        tracing::debug!(
                            "Pool {} nominations query failed (attempt {}), retrying: {}",
                            p.id,
                            attempt + 1,
                            e
                        );
                        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                    } else {
                        tracing::warn!(
                            "Failed to get nominations for pool {} after 3 attempts: {}",
                            p.id,
                            e
                        );
                        nominations_result = Some(Err(e));
                    }
                }
            }
        }

        // Calculate APY based on nominated validators
        let apy = match nominations_result {
            Some(Ok(Some(nominations))) if !nominations.targets.is_empty() => {
                // Calculate average APY from nominated validators
                let mut total_apy = 0.0;
                let mut count = 0;
                for target in &nominations.targets {
                    let target_str = target.to_string();
                    if let Some(&validator_apy) = validator_apy_map.get(&target_str) {
                        total_apy += validator_apy;
                        count += 1;
                    } else {
                        tracing::debug!(
                            "Pool {} nominated validator {} not in validator list",
                            p.id,
                            target_str
                        );
                    }
                }
                if count > 0 {
                    tracing::debug!(
                        "Pool {} has {} nominated validators with APY, avg: {:.2}%",
                        p.id,
                        count,
                        (total_apy / count as f64) * 100.0
                    );
                    Some(total_apy / count as f64)
                } else {
                    tracing::debug!(
                        "Pool {} has {} nominations but none found in validator map",
                        p.id,
                        nominations.targets.len()
                    );
                    None
                }
            }
            Some(Ok(Some(_))) => {
                tracing::debug!("Pool {} has empty nominations", p.id);
                None
            }
            Some(Ok(None)) => {
                tracing::debug!("Pool {} has no nominations", p.id);
                None
            }
            Some(Err(_)) | None => {
                // Error already logged, continue to next pool
                None
            }
        };

        // Update APY for this pool
        display_pools[idx].apy = apy;

        // Send progress update every 10 pools
        if (idx + 1) % 10 == 0 {
            let _ = action_tx
                .send(Action::SetDisplayPools(display_pools.clone()))
                .await;
            tracing::debug!("Updated APY for {} pools", idx + 1);
        }
    }

    // Sort pools: by APY descending (pools with APY first, then by member count)
    display_pools.sort_by(|a, b| match (a.apy, b.apy) {
        (Some(a_apy), Some(b_apy)) => b_apy
            .partial_cmp(&a_apy)
            .unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => b.member_count.cmp(&a.member_count),
    });

    bytes_transferred += display_pools.len() as u64 * 150; // ~150 bytes per pool
    let _ = action_tx
        .send(Action::SetLoadingProgress(
            1.0,
            Some(bytes_transferred),
            None,
        ))
        .await;
    let _ = action_tx.send(Action::SetDisplayPools(display_pools)).await;

    tracing::info!("Nomination pools loaded successfully");

    // Listen for requests from the UI
    while let Some(request) = request_rx.recv().await {
        match request {
            ChainRequest::FetchAccount(account) => {
                tracing::info!("Fetching account status for {}", account);

                // Fetch all account data
                let balance = match client.get_account_balance(&account).await {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::error!("Failed to get account balance: {}", e);
                        // Use default zero balance on error - still update UI
                        stkopt_chain::AccountBalance {
                            free: 0,
                            reserved: 0,
                            frozen: 0,
                        }
                    }
                };

                let staking_ledger = match client.get_staking_ledger(&account).await {
                    Ok(l) => l,
                    Err(e) => {
                        tracing::error!("Failed to get staking ledger: {}", e);
                        None
                    }
                };

                let nominations = match client.get_nominations(&account).await {
                    Ok(n) => n,
                    Err(e) => {
                        tracing::error!("Failed to get nominations: {}", e);
                        None
                    }
                };

                let pool_membership = match client.get_pool_membership(&account).await {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::error!("Failed to get pool membership: {}", e);
                        None
                    }
                };

                let status = AccountStatus {
                    address: account,
                    balance,
                    staking_ledger,
                    nominations,
                    pool_membership,
                };

                let _ = action_tx
                    .send(Action::SetAccountStatus(Box::new(status)))
                    .await;
                tracing::info!("Account status updated");
            }
            ChainRequest::GenerateNominationQR { signer, targets } => {
                tracing::info!("Generating nomination QR for {} validators", targets.len());
                let target_strings: Vec<String> = targets.iter().map(|t| t.to_string()).collect();

                match client
                    .create_nominate_payload(&signer, &targets, true)
                    .await
                {
                    Ok(payload) => {
                        tracing::info!("QR data generated ({} bytes)", payload.call_data.len());
                        send_staking_qr(&action_tx, payload, signer, target_strings).await;
                    }
                    Err(e) => {
                        tracing::error!("Failed to generate nomination payload: {}", e);
                        clear_staking_qr(&action_tx).await;
                    }
                }
            }
            ChainRequest::FetchHistory {
                account,
                num_eras,
                cancel_rx,
            } => {
                tracing::info!(
                    "Loading staking history for {} ({} eras)",
                    account,
                    num_eras
                );

                let address = account.to_string();

                // Try to open database for caching
                let db_path = get_db_path();
                let mut db = match db::HistoryDb::open(&db_path) {
                    Ok(db) => Some(db),
                    Err(e) => {
                        tracing::warn!("Failed to open history database: {}", e);
                        None
                    }
                };

                // First, load any cached data immediately for fast display
                if let Some(ref db) = db
                    && let Ok(cached) = db.get_history(network, &address, Some(num_eras))
                    && !cached.is_empty()
                {
                    // Filter out cached entries with unrealistic APY (likely bad data)
                    let filtered: Vec<_> = cached.into_iter().filter(|h| h.apy <= 0.50).collect();
                    tracing::info!("Loaded {} cached history points (filtered)", filtered.len());
                    for point in filtered {
                        let _ = action_tx.send(Action::AddStakingHistoryPoint(point)).await;
                    }
                }

                // Get current era
                let current_era_info = match client.get_active_era().await {
                    Ok(Some(era)) => era,
                    Ok(None) => {
                        tracing::error!("No active era found");
                        let _ = action_tx.send(Action::HistoryLoadingComplete).await;
                        continue;
                    }
                    Err(e) => {
                        tracing::error!("Failed to get active era: {}", e);
                        let _ = action_tx.send(Action::HistoryLoadingComplete).await;
                        continue;
                    }
                };
                let current_era = current_era_info.index;
                let current_era_start_ms = current_era_info.start_timestamp_ms;

                // Get era duration for APY calculation
                let era_duration_ms = match client.get_era_duration_ms().await {
                    Ok(d) => d,
                    Err(e) => {
                        tracing::warn!("Failed to get era duration, using default: {}", e);
                        24 * 60 * 60 * 1000 // 24 hours
                    }
                };

                // Get user's bonded amount (approximate - use current value)
                let user_bonded = match client.get_staking_ledger(&account).await {
                    Ok(Some(ledger)) => ledger.active,
                    _ => 0,
                };

                // Determine which eras need to be fetched
                let start_era = current_era.saturating_sub(num_eras);
                let eras_to_fetch: Vec<u32> = if let Some(ref db) = db {
                    db.get_missing_eras(network, &address, start_era, current_era.saturating_sub(1))
                        .unwrap_or_else(|_| (start_era..current_era).collect())
                } else {
                    (start_era..current_era).collect()
                };

                if eras_to_fetch.is_empty() {
                    tracing::info!("All eras already cached");
                    let _ = action_tx.send(Action::HistoryLoadingComplete).await;
                    continue;
                }

                tracing::info!("Fetching {} missing eras from chain", eras_to_fetch.len());
                let mut new_points = Vec::new();

                // Fetch missing eras
                for era in eras_to_fetch {
                    // Check for cancellation
                    if *cancel_rx.borrow() {
                        tracing::info!("History loading cancelled");
                        let _ = action_tx.send(Action::HistoryLoadingComplete).await;
                        break;
                    }

                    // Get total era reward
                    let era_reward = match client.get_era_validator_reward(era).await {
                        Ok(Some(reward)) => reward,
                        Ok(None) => {
                            tracing::debug!("No reward data for era {}", era);
                            continue;
                        }
                        Err(e) => {
                            tracing::warn!("Failed to get era {} reward: {}", era, e);
                            continue;
                        }
                    };

                    // Get total staked for this era (direct point query, no iteration)
                    let total_staked = match client.get_era_total_stake_direct(era).await {
                        Ok(staked) if staked > 0 => staked,
                        Ok(_) => {
                            tracing::debug!("No stake data for era {}", era);
                            continue;
                        }
                        Err(e) => {
                            tracing::warn!("Failed to get era {} total staked: {}", era, e);
                            continue;
                        }
                    };

                    // Calculate network-wide APY based on total reward and total staked
                    let apy = get_era_apy(era_reward, total_staked, era_duration_ms);

                    // Sanity check: skip eras with unrealistic APY (likely incomplete or corrupted data)
                    if apy > 0.50 {
                        tracing::warn!(
                            "Era {} has unrealistic APY {:.2}% (reward={}, staked={}), skipping",
                            era,
                            apy * 100.0,
                            era_reward,
                            total_staked
                        );
                        continue;
                    }

                    // Estimate user's reward proportional to their stake
                    // NOTE: This is an estimate based on current stake, which may differ from actual historical stake
                    let user_reward = if user_bonded > 0 && total_staked > 0 {
                        let estimated =
                            (era_reward as f64 * user_bonded as f64 / total_staked as f64) as u128;

                        // Sanity check: cap estimated reward to avoid unrealistic values
                        let max_reasonable_reward = user_bonded / 200; // 0.5% of stake
                        if estimated > max_reasonable_reward && max_reasonable_reward > 0 {
                            tracing::warn!(
                                "Era {} reward estimate {} exceeds bound {}, capping",
                                era,
                                estimated,
                                max_reasonable_reward
                            );
                            max_reasonable_reward
                        } else {
                            estimated
                        }
                    } else {
                        0
                    };

                    // Calculate date for this era
                    let era_date =
                        calculate_era_date(era, current_era, current_era_start_ms, era_duration_ms);

                    let point = crate::action::StakingHistoryPoint {
                        era,
                        date: Some(era_date.clone()),
                        reward: user_reward,
                        bonded: user_bonded,
                        apy,
                    };

                    new_points.push(point.clone());
                    let _ = action_tx.send(Action::AddStakingHistoryPoint(point)).await;
                    tracing::debug!(
                        "Added history point for era {} (APY: {:.2}%)",
                        era,
                        apy * 100.0
                    );
                }

                // Store new points to database
                if let Some(ref mut db) = db
                    && !new_points.is_empty()
                {
                    if let Err(e) = db.insert_history_batch(network, &address, &new_points) {
                        tracing::warn!("Failed to cache history: {}", e);
                    } else {
                        tracing::info!("Cached {} new history points", new_points.len());
                    }
                }

                // Check if we completed without cancellation
                if !*cancel_rx.borrow() {
                    let _ = action_tx.send(Action::HistoryLoadingComplete).await;
                    tracing::info!("Staking history loaded");
                }
            }
            ChainRequest::ExecuteStakingOp(op) => {
                tracing::info!("Processing staking op: {:?}", op);
                let use_mortal_era = true;

                let (signer, result) = match &op {
                    StakingOp::Bond { signer, value } => (
                        signer,
                        client
                            .create_bond_payload(signer, *value, use_mortal_era)
                            .await,
                    ),
                    StakingOp::Unbond { signer, value } => (
                        signer,
                        client
                            .create_unbond_payload(signer, *value, use_mortal_era)
                            .await,
                    ),
                    StakingOp::BondExtra { signer, value } => (
                        signer,
                        client
                            .create_bond_extra_payload(signer, *value, use_mortal_era)
                            .await,
                    ),
                    StakingOp::SetPayee {
                        signer,
                        destination,
                    } => (
                        signer,
                        client
                            .create_set_payee_payload(signer, destination.clone(), use_mortal_era)
                            .await,
                    ),
                    StakingOp::WithdrawUnbonded { signer } => (
                        signer,
                        client
                            .create_withdraw_unbonded_payload(signer, 0, use_mortal_era)
                            .await,
                    ),
                    StakingOp::Chill { signer } => (
                        signer,
                        client.create_chill_payload(signer, use_mortal_era).await,
                    ),
                    StakingOp::PoolJoin {
                        signer,
                        pool_id,
                        amount,
                    } => (
                        signer,
                        client
                            .create_pool_join_payload(signer, *pool_id, *amount, use_mortal_era)
                            .await,
                    ),
                    StakingOp::PoolBondExtra { signer, amount } => (
                        signer,
                        client
                            .create_pool_bond_extra_payload(signer, *amount, use_mortal_era)
                            .await,
                    ),
                    StakingOp::PoolClaim { signer } => (
                        signer,
                        client
                            .create_pool_claim_payload(signer, use_mortal_era)
                            .await,
                    ),
                    StakingOp::PoolUnbond { signer, amount } => (
                        signer,
                        client
                            .create_pool_unbond_payload(signer, signer, *amount, use_mortal_era)
                            .await,
                    ),
                    StakingOp::PoolWithdraw { signer } => (
                        signer,
                        client
                            .create_pool_withdraw_payload(signer, signer, 0, use_mortal_era)
                            .await,
                    ),
                };

                match result {
                    Ok(payload) => {
                        tracing::info!("QR data generated ({} bytes)", payload.call_data.len());
                        send_staking_qr(&action_tx, payload, signer.clone(), vec![]).await;
                    }
                    Err(e) => {
                        tracing::error!("Failed to generate payload: {}", e);
                        clear_staking_qr(&action_tx).await;
                    }
                }
            }
            ChainRequest::SubmitTransaction(extrinsic) => {
                tracing::info!("Submitting signed extrinsic ({} bytes)", extrinsic.len());

                match client.submit_signed_extrinsic(&extrinsic).await {
                    Ok(progress) => {
                        tracing::info!("Transaction submitted, waiting for inclusion...");

                        // Wait for the transaction to be finalized
                        match progress.wait_for_finalized().await {
                            Ok(result) => {
                                tracing::info!(
                                    "Transaction finalized in block 0x{}",
                                    hex::encode(result.block_hash)
                                );
                                let _ = action_tx
                                    .send(Action::SetTxStatus(TxSubmissionStatus::Finalized {
                                        block_hash: result.block_hash,
                                    }))
                                    .await;
                            }
                            Err(e) => {
                                tracing::error!("Transaction failed: {}", e);
                                let _ = action_tx
                                    .send(Action::SetTxStatus(TxSubmissionStatus::Failed(
                                        e.to_string(),
                                    )))
                                    .await;
                            }
                        }
                    }
                    Err(e) => {
                        let error_str = e.to_string();
                        tracing::error!("Failed to submit transaction: {}", error_str);

                        // Provide more helpful error messages for common issues
                        let user_message = if error_str.contains("1010")
                            || error_str.contains("Invalid Transaction")
                        {
                            "Transaction rejected - may be expired or already submitted. Please generate a new QR code.".to_string()
                        } else if error_str.contains("1014") || error_str.contains("Priority") {
                            "Transaction priority too low. Please try again.".to_string()
                        } else if error_str.contains("1012") || error_str.contains("Pool") {
                            "Transaction pool is full. Please try again later.".to_string()
                        } else {
                            error_str
                        };

                        let _ = action_tx
                            .send(Action::SetTxStatus(TxSubmissionStatus::Failed(
                                user_message,
                            )))
                            .await;
                    }
                }
            }
        }
    }
}

/// Get the path to the history database file.
fn get_db_path() -> std::path::PathBuf {
    if let Some(proj_dirs) = directories::ProjectDirs::from("io", "stkopt", "stkopt") {
        let data_dir = proj_dirs.data_dir();
        std::fs::create_dir_all(data_dir).ok();
        data_dir.join("history.db")
    } else {
        std::path::PathBuf::from("stkopt_history.db")
    }
}

/// Run in update mode: fetch missing history and store to database, then exit.
/// This is suitable for running from cron jobs.
async fn run_update_mode(
    network: Network,
    config: ConnectionConfig,
    address: Option<String>,
    num_eras: u32,
) -> Result<()> {
    use stkopt_core::apy::get_era_apy;

    let address = match address {
        Some(addr) => addr,
        None => {
            eprintln!("Error: --address is required with --update");
            std::process::exit(1);
        }
    };

    let account: subxt::utils::AccountId32 = address
        .parse()
        .map_err(|_| color_eyre::eyre::eyre!("Invalid address format: {}", address))?;

    println!("Updating staking history for {} on {}", address, network);

    // Open database
    let db_path = get_db_path();
    let mut db = db::HistoryDb::open(&db_path).map_err(|e| {
        color_eyre::eyre::eyre!("Failed to open database at {}: {}", db_path.display(), e)
    })?;

    println!("Database: {}", db_path.display());

    // Create connection status channel (not used in update mode, but required by API)
    let (status_tx, _status_rx) = mpsc::channel::<ConnectionStatus>(1);

    // Connect to chain (update mode always uses RPC for historical queries)
    println!("Connecting to {} Asset Hub...", network);
    let client = ChainClient::connect(network, &config, status_tx).await?;
    println!("Connected via {}", client.connection_mode());

    // Get current era info
    let current_era_info = client
        .get_active_era()
        .await?
        .ok_or_else(|| color_eyre::eyre::eyre!("No active era found"))?;

    let current_era = current_era_info.index;
    let current_era_start_ms = current_era_info.start_timestamp_ms;
    let era_duration_ms = client
        .get_era_duration_ms()
        .await
        .unwrap_or(24 * 60 * 60 * 1000);

    println!(
        "Current era: {} ({}ms per era)",
        current_era, era_duration_ms
    );

    // Get user's bonded amount
    let user_bonded = match client.get_staking_ledger(&account).await {
        Ok(Some(ledger)) => ledger.active,
        _ => 0,
    };

    // Calculate era range
    let start_era = current_era.saturating_sub(num_eras);

    // Find which eras are missing from the cache
    let missing_eras = db
        .get_missing_eras(network, &address, start_era, current_era.saturating_sub(1))
        .map_err(|e| color_eyre::eyre::eyre!("Database error: {}", e))?;

    if missing_eras.is_empty() {
        println!("All {} eras are already cached", num_eras);
        return Ok(());
    }

    println!(
        "Fetching {} missing eras ({} of {} already cached)",
        missing_eras.len(),
        num_eras - missing_eras.len() as u32,
        num_eras
    );

    let mut points = Vec::new();
    let mut fetched = 0;

    for era in missing_eras {
        // Get total era reward
        let era_reward = match client.get_era_validator_reward(era).await {
            Ok(Some(reward)) => reward,
            Ok(None) => {
                eprintln!("  Era {}: no reward data", era);
                continue;
            }
            Err(e) => {
                eprintln!("  Era {}: error getting reward: {}", era, e);
                continue;
            }
        };

        // Get total staked (direct point query, no iteration)
        let total_staked = match client.get_era_total_stake_direct(era).await {
            Ok(staked) if staked > 0 => staked,
            Ok(_) => {
                eprintln!("  Era {}: no stake data", era);
                continue;
            }
            Err(e) => {
                eprintln!("  Era {}: error getting stake: {}", era, e);
                continue;
            }
        };

        // Calculate APY
        let apy = get_era_apy(era_reward, total_staked, era_duration_ms);

        // Estimate user's reward
        let user_reward = if user_bonded > 0 && total_staked > 0 {
            (era_reward as f64 * user_bonded as f64 / total_staked as f64) as u128
        } else {
            0
        };

        // Calculate date
        let era_date = calculate_era_date(era, current_era, current_era_start_ms, era_duration_ms);

        let point = action::StakingHistoryPoint {
            era,
            date: Some(era_date),
            reward: user_reward,
            bonded: user_bonded,
            apy,
        };

        points.push(point);
        fetched += 1;

        if fetched % 10 == 0 {
            print!("  Fetched {} eras...\r", fetched);
        }
    }

    println!();

    // Store to database
    if !points.is_empty() {
        db.insert_history_batch(network, &address, &points)
            .map_err(|e| color_eyre::eyre::eyre!("Failed to store history: {}", e))?;
        println!("Stored {} era records to database", points.len());
    }

    let total = db
        .count_history(network, &address)
        .map_err(|e| color_eyre::eyre::eyre!("Database error: {}", e))?;

    println!("Total records for {}: {}", address, total);
    Ok(())
}

/// Calculate the date string for an era in YYYYMMDD format.
/// Uses current time as fallback if start_timestamp_ms is 0.
fn calculate_era_date(
    era: u32,
    current_era: u32,
    current_era_start_ms: u64,
    era_duration_ms: u64,
) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let eras_ago = current_era.saturating_sub(era);

    // If current_era_start_ms is 0 (unavailable), use current time as reference
    let era_start_ms = if current_era_start_ms > 0 {
        current_era_start_ms.saturating_sub(eras_ago as u64 * era_duration_ms)
    } else {
        // Fallback: use current time minus eras_ago * era_duration
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                tracing::warn!("Time regression detected: {}, using 0", e);
            })
            .unwrap_or_default()
            .as_millis() as u64;
        now_ms.saturating_sub(eras_ago as u64 * era_duration_ms)
    };

    // Convert to date
    let secs = era_start_ms / 1000;
    let days_since_epoch = secs / 86400;

    // More accurate date calculation using days
    let mut year = 1970i32;
    let mut remaining_days = days_since_epoch as i32;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }

    let days_in_months: [i32; 12] = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1;
    for days in days_in_months {
        if remaining_days < days {
            break;
        }
        remaining_days -= days;
        month += 1;
    }

    let day = remaining_days + 1;

    format!("{:04}{:02}{:02}", year, month, day)
}

/// Check if a year is a leap year.
fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}
