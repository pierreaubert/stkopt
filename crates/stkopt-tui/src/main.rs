//! Staking Optimizer TUI - A terminal interface for Polkadot staking optimization.

mod action;
mod app;
mod chain_task;
mod config;
mod db;
mod event;
mod log_buffer;
mod qr_reader;
mod tcc;
mod theme;
mod tui;
mod ui;

use action::{Action, PendingTransaction, TxSubmissionStatus};
use app::App;
use chain_task::{ChainRequest, StakingOp, chain_task, get_db_path, run_update_mode};
use clap::Parser;
use color_eyre::Result;
use event::{Event, EventHandler};
use log_buffer::{LogBuffer, LogBufferLayer};
use ratatui::crossterm::event::KeyCode;
use std::cmp::Ordering;
use stkopt_core::{Network, OptimizationResult, SelectionStrategy, ValidatorCandidate};
use tokio::sync::mpsc;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tui::Tui;

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

fn validator_candidates(app: &App) -> Vec<ValidatorCandidate> {
    app.validators
        .iter()
        .map(|v| ValidatorCandidate {
            address: v.address.clone(),
            commission: v.commission,
            blocked: v.blocked,
            apy: v.apy.unwrap_or(0.0),
            total_stake: v.total_stake,
            nominator_count: v.nominator_count,
        })
        .collect()
}

fn fallback_select_without_apy(
    candidates: &[ValidatorCandidate],
    criteria: &stkopt_core::OptimizationCriteria,
) -> OptimizationResult {
    let mut eligible: Vec<_> = candidates
        .iter()
        .filter(|v| v.commission <= criteria.max_commission && !v.blocked)
        .cloned()
        .collect();

    eligible.sort_by(|a, b| {
        a.commission
            .partial_cmp(&b.commission)
            .unwrap_or(Ordering::Equal)
            .then_with(|| b.total_stake.cmp(&a.total_stake))
            .then_with(|| b.nominator_count.cmp(&a.nominator_count))
    });

    OptimizationResult {
        selected: eligible.into_iter().take(criteria.target_count).collect(),
        estimated_apy_min: 0.0,
        estimated_apy_max: 0.0,
        estimated_apy_avg: 0.0,
    }
}

fn optimize_nomination(
    app: &App,
    strategy: SelectionStrategy,
) -> (OptimizationResult, Option<String>) {
    let candidates = validator_candidates(app);
    let criteria = stkopt_core::OptimizationCriteria {
        strategy,
        ..stkopt_core::OptimizationCriteria::default()
    };
    let result = stkopt_core::select_validators(&candidates, &criteria);

    if !result.selected.is_empty() || candidates.iter().any(|v| v.apy > 0.0) {
        return (result, None);
    }

    let fallback = fallback_select_without_apy(&candidates, &criteria);
    let status = if fallback.selected.is_empty() {
        Some(
            "No eligible validators found. Try loading validator data or relaxing filters."
                .to_string(),
        )
    } else {
        Some(format!(
            "APY is unavailable; selected {} low-commission active validators by stake.",
            fallback.selected.len()
        ))
    };
    (fallback, status)
}

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
        .add_directive("stkopt_core=info".parse()?)
        .add_directive("json-rpc=warn".parse()?)
        .add_directive("network=info".parse()?)
        .add_directive("runtime=info".parse()?)
        .add_directive("sync-service=info".parse()?)
        .add_directive("bitswap-service=info".parse()?)
        .add_directive("tx-service=info".parse()?)
        .add_directive("stkopt_chain::lightclient=info".parse()?)
        .add_directive("stkopt_chain::queries::identity=info".parse()?);

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
                        let (result, status) =
                            optimize_nomination(&app, SelectionStrategy::TopApy);
                        let _ = action_tx.send(Action::SetOptimizationResult(result)).await;
                        if let Some(status) = status {
                            let _ = action_tx
                                .send(Action::SetNominationStatus(Some(status)))
                                .await;
                        }
                    }
                    Action::RunOptimizationWithStrategy(strategy_idx) => {
                        // Run optimization with selected strategy
                        let strategy = match strategy_idx {
                            0 => SelectionStrategy::TopApy,
                            1 => SelectionStrategy::RandomFromTop,
                            2 => SelectionStrategy::DiversifyByStake,
                            _ => SelectionStrategy::TopApy,
                        };
                        let (result, status) = optimize_nomination(&app, strategy);
                        let _ = action_tx.send(Action::SetOptimizationResult(result)).await;
                        if let Some(status) = status {
                            let _ = action_tx
                                .send(Action::SetNominationStatus(Some(status)))
                                .await;
                        }
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
                        if app.watched_account.is_none() {
                            let _ = action_tx
                                .send(Action::SetNominationStatus(Some(
                                    "Select an account before generating a nomination QR."
                                        .to_string(),
                                )))
                                .await;
                        } else if app.selected_validators.is_empty() {
                            let _ = action_tx
                                .send(Action::SetNominationStatus(Some(
                                    "Select validators first, or press o to optimize.".to_string(),
                                )))
                                .await;
                        } else if let Some(account) = &app.watched_account {
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
                                let _ = action_tx
                                    .send(Action::SetNominationStatus(Some(format!(
                                        "Generating nomination QR for {} validators...",
                                        targets.len()
                                    ))))
                                    .await;
                                let _ = request_tx.send(ChainRequest::GenerateNominationQR {
                                    signer: account.clone(),
                                    targets,
                                }).await;
                            } else {
                                let _ = action_tx
                                    .send(Action::SetNominationStatus(Some(
                                        "Selected validator addresses could not be encoded for nomination."
                                            .to_string(),
                                    )))
                                    .await;
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
                                    let signed = match stkopt_chain::build_signed_extrinsic(
                                        &pending.payload,
                                        &pending.signer,
                                        &decoded_sig,
                                    ) {
                                        Ok(signed) => signed,
                                        Err(e) => {
                                            tracing::error!("Failed to build signed extrinsic: {}", e);
                                            app.qr.pending_signed = None;
                                            app.camera.status = Some(app::CameraScanStatus::Error);
                                            continue;
                                        }
                                    };

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
                    Action::SwitchNetwork(network) => {
                        let _ = request_tx.send(ChainRequest::Reconnect(*network)).await;
                    }
                    Action::SelectAddressBookEntry(idx) => {
                        let idx = *idx;
                        tracing::info!("[ADDR] SelectAddressBookEntry idx={}, watched={}", idx, app.watched_account.is_some());

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

                        tracing::info!("[ADDR] known_idx={}, known_addresses.len()={}", known_idx, app::KNOWN_ADDRESSES.len());
                        if let Some((name, addr)) = app::KNOWN_ADDRESSES.get(known_idx) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use action::DisplayValidator;
    use std::str::FromStr;
    use stkopt_core::{OptimizationCriteria, ValidatorCandidate};
    use theme::Theme;

    fn make_validator(
        address: &str,
        commission: f64,
        blocked: bool,
        total_stake: u128,
        apy: Option<f64>,
    ) -> DisplayValidator {
        DisplayValidator {
            address: address.to_string(),
            name: None,
            commission,
            blocked,
            total_stake,
            own_stake: total_stake / 10,
            nominator_count: 10,
            points: 0,
            apy,
        }
    }

    fn make_validator_with_nominators(
        address: &str,
        commission: f64,
        blocked: bool,
        total_stake: u128,
        nominator_count: u32,
        apy: Option<f64>,
    ) -> DisplayValidator {
        DisplayValidator {
            address: address.to_string(),
            name: None,
            commission,
            blocked,
            total_stake,
            own_stake: total_stake / 10,
            nominator_count,
            points: 0,
            apy,
        }
    }

    // ── validator_candidates ──────────────────────────────────────────────

    #[test]
    fn test_validator_candidates_empty_app() {
        let app = App::new(Network::Polkadot, LogBuffer::new(), Theme::Dark);
        let candidates = validator_candidates(&app);
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_validator_candidates_maps_fields_correctly() {
        let mut app = App::new(Network::Polkadot, LogBuffer::new(), Theme::Dark);
        app.validators = vec![make_validator_with_nominators(
            "v1",
            0.10,
            false,
            1_000_000,
            42,
            Some(0.15),
        )];
        let candidates = validator_candidates(&app);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].address, "v1");
        assert_eq!(candidates[0].commission, 0.10);
        assert!(!candidates[0].blocked);
        assert_eq!(candidates[0].total_stake, 1_000_000);
        assert_eq!(candidates[0].nominator_count, 42);
        assert_eq!(candidates[0].apy, 0.15);
    }

    #[test]
    fn test_validator_candidates_apy_defaults_to_zero() {
        let mut app = App::new(Network::Polkadot, LogBuffer::new(), Theme::Dark);
        app.validators = vec![make_validator("v1", 0.05, false, 100, None)];
        let candidates = validator_candidates(&app);
        assert_eq!(candidates[0].apy, 0.0);
    }

    #[test]
    fn test_validator_candidates_multiple_validators() {
        let mut app = App::new(Network::Polkadot, LogBuffer::new(), Theme::Dark);
        app.validators = vec![
            make_validator("a", 0.05, false, 100, Some(0.10)),
            make_validator("b", 0.10, true, 200, None),
        ];
        let candidates = validator_candidates(&app);
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].address, "a");
        assert_eq!(candidates[1].address, "b");
        assert!(candidates[1].blocked);
    }

    // ── fallback_select_without_apy ───────────────────────────────────────

    #[test]
    fn test_fallback_select_without_apy_empty_candidates() {
        let criteria = OptimizationCriteria::default();
        let result = fallback_select_without_apy(&[], &criteria);
        assert!(result.selected.is_empty());
        assert_eq!(result.estimated_apy_min, 0.0);
        assert_eq!(result.estimated_apy_max, 0.0);
        assert_eq!(result.estimated_apy_avg, 0.0);
    }

    #[test]
    fn test_fallback_select_without_apy_all_blocked() {
        let candidates = vec![ValidatorCandidate {
            address: "v1".to_string(),
            commission: 0.05,
            blocked: true,
            apy: 0.0,
            total_stake: 1_000,
            nominator_count: 10,
        }];
        let criteria = OptimizationCriteria::default();
        let result = fallback_select_without_apy(&candidates, &criteria);
        assert!(result.selected.is_empty());
    }

    #[test]
    fn test_fallback_select_without_apy_all_high_commission() {
        let candidates = vec![ValidatorCandidate {
            address: "v1".to_string(),
            commission: 0.50,
            blocked: false,
            apy: 0.0,
            total_stake: 1_000,
            nominator_count: 10,
        }];
        let criteria = OptimizationCriteria::default();
        let result = fallback_select_without_apy(&candidates, &criteria);
        assert!(result.selected.is_empty());
    }

    #[test]
    fn test_fallback_select_without_apy_sorts_by_commission() {
        let candidates = vec![
            ValidatorCandidate {
                address: "high-comm".to_string(),
                commission: 0.10,
                blocked: false,
                apy: 0.0,
                total_stake: 1_000,
                nominator_count: 10,
            },
            ValidatorCandidate {
                address: "low-comm".to_string(),
                commission: 0.01,
                blocked: false,
                apy: 0.0,
                total_stake: 1_000,
                nominator_count: 10,
            },
        ];
        let criteria = OptimizationCriteria::default();
        let result = fallback_select_without_apy(&candidates, &criteria);
        assert_eq!(result.selected.len(), 2);
        assert_eq!(result.selected[0].address, "low-comm");
        assert_eq!(result.selected[1].address, "high-comm");
    }

    #[test]
    fn test_fallback_select_without_apy_tiebreak_by_total_stake() {
        let candidates = vec![
            ValidatorCandidate {
                address: "low-stake".to_string(),
                commission: 0.05,
                blocked: false,
                apy: 0.0,
                total_stake: 500,
                nominator_count: 10,
            },
            ValidatorCandidate {
                address: "high-stake".to_string(),
                commission: 0.05,
                blocked: false,
                apy: 0.0,
                total_stake: 5_000,
                nominator_count: 10,
            },
        ];
        let criteria = OptimizationCriteria::default();
        let result = fallback_select_without_apy(&candidates, &criteria);
        assert_eq!(result.selected.len(), 2);
        assert_eq!(result.selected[0].address, "high-stake");
        assert_eq!(result.selected[1].address, "low-stake");
    }

    #[test]
    fn test_fallback_select_without_apy_tiebreak_by_nominator_count() {
        let candidates = vec![
            ValidatorCandidate {
                address: "few-noms".to_string(),
                commission: 0.05,
                blocked: false,
                apy: 0.0,
                total_stake: 1_000,
                nominator_count: 5,
            },
            ValidatorCandidate {
                address: "many-noms".to_string(),
                commission: 0.05,
                blocked: false,
                apy: 0.0,
                total_stake: 1_000,
                nominator_count: 50,
            },
        ];
        let criteria = OptimizationCriteria::default();
        let result = fallback_select_without_apy(&candidates, &criteria);
        assert_eq!(result.selected.len(), 2);
        assert_eq!(result.selected[0].address, "many-noms");
        assert_eq!(result.selected[1].address, "few-noms");
    }

    #[test]
    fn test_fallback_select_without_apy_respects_target_count() {
        let candidates: Vec<ValidatorCandidate> = (0..20)
            .map(|i| ValidatorCandidate {
                address: format!("v{}", i),
                commission: 0.01,
                blocked: false,
                apy: 0.0,
                total_stake: 1_000,
                nominator_count: 10,
            })
            .collect();
        let criteria = OptimizationCriteria::default();
        let result = fallback_select_without_apy(&candidates, &criteria);
        // default target_count is 16
        assert_eq!(result.selected.len(), 16);
    }

    #[test]
    fn test_fallback_select_without_apy_mixed_eligible() {
        let candidates = vec![
            ValidatorCandidate {
                address: "blocked".to_string(),
                commission: 0.01,
                blocked: true,
                apy: 0.0,
                total_stake: 10_000,
                nominator_count: 100,
            },
            ValidatorCandidate {
                address: "high-comm".to_string(),
                commission: 0.50,
                blocked: false,
                apy: 0.0,
                total_stake: 10_000,
                nominator_count: 100,
            },
            ValidatorCandidate {
                address: "eligible".to_string(),
                commission: 0.05,
                blocked: false,
                apy: 0.0,
                total_stake: 1_000,
                nominator_count: 10,
            },
        ];
        let criteria = OptimizationCriteria::default();
        let result = fallback_select_without_apy(&candidates, &criteria);
        assert_eq!(result.selected.len(), 1);
        assert_eq!(result.selected[0].address, "eligible");
    }

    // ── NetworkArg::from_str ──────────────────────────────────────────────

    #[test]
    fn test_network_arg_from_str_polkadot() {
        assert_eq!(
            NetworkArg::from_str("polkadot").unwrap().0,
            Network::Polkadot
        );
    }

    #[test]
    fn test_network_arg_from_str_dot() {
        assert_eq!(NetworkArg::from_str("dot").unwrap().0, Network::Polkadot);
    }

    #[test]
    fn test_network_arg_from_str_kusama() {
        assert_eq!(NetworkArg::from_str("kusama").unwrap().0, Network::Kusama);
    }

    #[test]
    fn test_network_arg_from_str_ksm() {
        assert_eq!(NetworkArg::from_str("ksm").unwrap().0, Network::Kusama);
    }

    #[test]
    fn test_network_arg_from_str_westend() {
        assert_eq!(NetworkArg::from_str("westend").unwrap().0, Network::Westend);
    }

    #[test]
    fn test_network_arg_from_str_wnd() {
        assert_eq!(NetworkArg::from_str("wnd").unwrap().0, Network::Westend);
    }

    #[test]
    fn test_network_arg_from_str_paseo() {
        assert_eq!(NetworkArg::from_str("paseo").unwrap().0, Network::Paseo);
    }

    #[test]
    fn test_network_arg_from_str_pas() {
        assert_eq!(NetworkArg::from_str("pas").unwrap().0, Network::Paseo);
    }

    #[test]
    fn test_network_arg_from_str_unknown() {
        let result = NetworkArg::from_str("bitcoin");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("bitcoin"));
    }

    #[test]
    fn test_network_arg_from_str_case_insensitive() {
        assert_eq!(
            NetworkArg::from_str("PolKaDot").unwrap().0,
            Network::Polkadot
        );
        assert_eq!(NetworkArg::from_str("DOT").unwrap().0, Network::Polkadot);
        assert_eq!(NetworkArg::from_str("KUSAMA").unwrap().0, Network::Kusama);
        assert_eq!(NetworkArg::from_str("WestEnd").unwrap().0, Network::Westend);
        assert_eq!(NetworkArg::from_str("PaSeO").unwrap().0, Network::Paseo);
    }

    // ── optimize_nomination ───────────────────────────────────────────────

    #[test]
    fn optimize_nomination_falls_back_when_apy_is_unavailable() {
        let mut app = App::new(Network::Polkadot, LogBuffer::new(), Theme::Dark);
        app.validators = vec![
            make_validator("high-stake", 0.05, false, 1_000, None),
            make_validator("blocked", 0.01, true, 10_000, None),
            make_validator("low-commission", 0.01, false, 500, None),
        ];

        let (result, status) = optimize_nomination(&app, SelectionStrategy::TopApy);

        assert_eq!(result.selected.len(), 2);
        assert_eq!(result.selected[0].address, "low-commission");
        assert_eq!(result.selected[1].address, "high-stake");
        assert!(
            status
                .as_deref()
                .is_some_and(|message| message.contains("APY is unavailable"))
        );
    }

    #[test]
    fn optimize_nomination_uses_apy_when_available() {
        let mut app = App::new(Network::Polkadot, LogBuffer::new(), Theme::Dark);
        app.validators = vec![
            make_validator("lower-apy", 0.05, false, 1_000, Some(0.08)),
            make_validator("higher-apy", 0.05, false, 500, Some(0.12)),
        ];

        let (result, status) = optimize_nomination(&app, SelectionStrategy::TopApy);

        assert_eq!(result.selected.len(), 2);
        assert_eq!(result.selected[0].address, "higher-apy");
        assert!(status.is_none());
    }

    #[test]
    fn test_optimize_nomination_no_eligible_validators() {
        let mut app = App::new(Network::Polkadot, LogBuffer::new(), Theme::Dark);
        app.validators = vec![
            make_validator("blocked1", 0.01, true, 10_000, None),
            make_validator("blocked2", 0.01, true, 5_000, None),
        ];

        let (result, status) = optimize_nomination(&app, SelectionStrategy::TopApy);

        assert!(result.selected.is_empty());
        assert!(
            status
                .as_deref()
                .is_some_and(|msg| msg.contains("No eligible validators found"))
        );
    }

    #[test]
    fn test_optimize_nomination_mixed_apy_and_no_apy() {
        let mut app = App::new(Network::Polkadot, LogBuffer::new(), Theme::Dark);
        app.validators = vec![
            make_validator("no-apy", 0.01, false, 10_000, None),
            make_validator("with-apy", 0.05, false, 500, Some(0.10)),
        ];

        let (result, status) = optimize_nomination(&app, SelectionStrategy::TopApy);

        // When at least one validator has APY, core optimizer is used and fallback is skipped
        assert!(!result.selected.is_empty());
        assert!(status.is_none());
    }

    #[test]
    fn test_optimize_nomination_all_high_commission_no_apy() {
        let mut app = App::new(Network::Polkadot, LogBuffer::new(), Theme::Dark);
        app.validators = vec![
            make_validator("high-comm1", 0.50, false, 10_000, None),
            make_validator("high-comm2", 0.60, false, 5_000, None),
        ];

        let (result, status) = optimize_nomination(&app, SelectionStrategy::TopApy);

        // default max_commission is 0.15, so none are eligible
        assert!(result.selected.is_empty());
        assert!(
            status
                .as_deref()
                .is_some_and(|msg| msg.contains("No eligible validators found"))
        );
    }
}
