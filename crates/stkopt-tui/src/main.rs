//! Staking Optimizer TUI - A terminal interface for Polkadot staking optimization.

mod action;
mod app;
mod config;
mod db;
mod event;
mod log_buffer;
mod theme;
mod tui;
mod ui;

use action::{AccountStatus, Action};
use app::App;
use clap::Parser;
use config::AppConfig;
use color_eyre::Result;
use event::{Event, EventHandler};
use log_buffer::{LogBuffer, LogBufferLayer};
use ratatui::crossterm::event::KeyCode;
use stkopt_chain::ChainClient;
use stkopt_core::{ConnectionStatus, Network};
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

    /// Custom RPC endpoint URL (overrides default endpoints)
    #[arg(short = 'u', long = "url")]
    rpc_url: Option<String>,

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

    // Initialize logging with our buffer layer
    let env_filter = tracing_subscriber::EnvFilter::from_default_env()
        .add_directive("stkopt=info".parse()?)
        .add_directive("stkopt_chain=info".parse()?)
        .add_directive("stkopt_core=info".parse()?);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(LogBufferLayer::new(log_buffer.clone()))
        .init();

    let network = args.network.0;
    let custom_rpc = args.rpc_url.clone();

    // Handle update mode (batch mode for cron jobs)
    if args.update {
        return run_update_mode(network, custom_rpc, args.address, args.eras).await;
    }

    // Create action channel
    let (action_tx, mut action_rx) = mpsc::unbounded_channel::<Action>();

    // Create account request channel
    let (account_tx, account_rx) = mpsc::unbounded_channel::<subxt::utils::AccountId32>();

    // Create QR generation request channel
    // (account, validator addresses)
    let (qr_tx, qr_rx) = mpsc::unbounded_channel::<(
        subxt::utils::AccountId32,
        Vec<subxt::utils::AccountId32>,
    )>();

    // Create history loading request channel
    // (account, num_eras, cancellation receiver)
    let (history_tx, history_rx) =
        mpsc::unbounded_channel::<(subxt::utils::AccountId32, u32, tokio::sync::watch::Receiver<bool>)>();

    // Cancellation sender for history loading
    let (history_cancel_tx, history_cancel_rx) = tokio::sync::watch::channel(false);

    // Detect terminal theme (must be done before entering raw mode)
    let theme = theme::Theme::detect();

    // Load configuration
    let mut app_config = AppConfig::load();
    tracing::info!(
        "Loaded {} saved account(s)",
        app_config.accounts.len()
    );

    // Create application state
    let mut app = App::new(network, log_buffer, theme);

    // Load last saved account if available
    if let Some(last_addr) = app_config.last_account()
        && let Ok(account) = last_addr.parse::<subxt::utils::AccountId32>()
    {
        tracing::info!("Restoring last used account: {}", last_addr);
        app.watched_account = Some(account);
    }

    // Initialize terminal
    let mut tui = Tui::new()?;
    tui.enter()?;

    // Create event handler
    let mut events = EventHandler::new(250);

    // Spawn chain connection task
    let chain_action_tx = action_tx.clone();
    let account_action_tx = action_tx.clone();
    let qr_action_tx = action_tx.clone();
    let history_action_tx = action_tx.clone();
    tokio::spawn(async move {
        chain_task(
            network,
            custom_rpc,
            chain_action_tx,
            account_rx,
            account_action_tx,
            qr_rx,
            qr_action_tx,
            history_rx,
            history_action_tx,
        )
        .await;
    });

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
                    }
                    Event::Key(key_event) => {
                        if key_event.code == KeyCode::Char('q') && app.input_mode == app::InputMode::Normal {
                            break;
                        }
                        if let Some(action) = app.handle_key(key_event) {
                            let _ = action_tx.send(action);
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
                    Action::SetWatchedAccount(account) => {
                        let _ = account_tx.send(account.clone());
                        // Save account to config (public key only)
                        let addr_str = account.to_string();
                        app_config.add_account(
                            addr_str,
                            None,
                            Some(network.to_string()),
                        );
                        if let Err(e) = app_config.save() {
                            tracing::warn!("Failed to save config: {}", e);
                        }
                    }
                    Action::RunOptimization => {
                        // Run optimization with current validators
                        let candidates: Vec<_> = app.validators.iter().map(|v| {
                            stkopt_core::ValidatorCandidate {
                                address: v.address.clone(),
                                commission: v.commission,
                                blocked: v.blocked,
                                apy: v.apy,
                                total_stake: v.total_stake,
                                nominator_count: v.nominator_count,
                            }
                        }).collect();

                        let criteria = stkopt_core::OptimizationCriteria::default();
                        let result = stkopt_core::select_validators(&candidates, &criteria);
                        let _ = action_tx.send(Action::SetOptimizationResult(result));
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
                                let _ = qr_tx.send((account.clone(), targets));
                            }
                        }
                    }
                    Action::LoadStakingHistory => {
                        if let Some(account) = &app.watched_account {
                            // Reset cancellation flag
                            let _ = history_cancel_tx.send(false);
                            let _ = history_tx.send((
                                account.clone(),
                                app.history_total_eras,
                                history_cancel_rx.clone(),
                            ));
                        }
                    }
                    Action::CancelLoadingHistory => {
                        // Signal cancellation
                        let _ = history_cancel_tx.send(true);
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

/// Background task for chain operations.
#[allow(clippy::too_many_arguments)]
async fn chain_task(
    network: Network,
    custom_rpc: Option<String>,
    action_tx: mpsc::UnboundedSender<Action>,
    mut account_rx: mpsc::UnboundedReceiver<subxt::utils::AccountId32>,
    account_action_tx: mpsc::UnboundedSender<Action>,
    mut qr_rx: mpsc::UnboundedReceiver<(
        subxt::utils::AccountId32,
        Vec<subxt::utils::AccountId32>,
    )>,
    qr_action_tx: mpsc::UnboundedSender<Action>,
    mut history_rx: mpsc::UnboundedReceiver<(
        subxt::utils::AccountId32,
        u32,
        tokio::sync::watch::Receiver<bool>,
    )>,
    history_action_tx: mpsc::UnboundedSender<Action>,
) {
    use crate::action::{DisplayPool, DisplayValidator};
    use stkopt_core::get_era_apy;
    use std::collections::HashMap;

    // Create status channel for connection updates
    let (status_tx, mut status_rx) = mpsc::unbounded_channel::<ConnectionStatus>();

    // Forward status updates to action channel
    let action_tx_clone = action_tx.clone();
    tokio::spawn(async move {
        while let Some(status) = status_rx.recv().await {
            let _ = action_tx_clone.send(Action::UpdateConnectionStatus(status));
        }
    });

    // Connect to Asset Hub
    let client = match ChainClient::connect_rpc(network, custom_rpc.as_deref(), status_tx).await {
        Ok(client) => {
            tracing::info!(
                "Connected to {} Asset Hub (genesis: {:?})",
                network,
                client.genesis_hash()
            );
            client
        }
        Err(e) => {
            tracing::error!("Failed to connect to Asset Hub: {}", e);
            let _ = action_tx.send(Action::UpdateConnectionStatus(ConnectionStatus::Error(
                e.to_string(),
            )));
            return;
        }
    };

    // Connect to People chain for identity queries
    let people_client = match stkopt_chain::connect_people_chain(network).await {
        Ok(subxt_client) => {
            tracing::info!("Connected to {} People chain", network);
            Some(stkopt_chain::PeopleChainClient::new(subxt_client))
        }
        Err(e) => {
            tracing::warn!("Failed to connect to People chain (identities unavailable): {}", e);
            None
        }
    };

    // Brief delay to let connection stabilize
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Fetch era info (retry a few times as light client may need time to sync state)
    let era_info = {
        let mut era_result = None;
        for attempt in 1..=10 {
            match client.get_active_era().await {
                Ok(Some(info)) => {
                    let _ = action_tx.send(Action::SetActiveEra(info.clone()));
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
            let _ = action_tx.send(Action::SetEraDuration(duration));
            duration
        }
        Err(e) => {
            tracing::error!("Failed to get era duration: {}", e);
            86_400_000 // Default to 24 hours
        }
    };

    let _ = action_tx.send(Action::SetLoadingProgress(0.1));

    // Fetch validators
    let validators = match client.get_validators().await {
        Ok(v) => {
            tracing::info!("Found {} registered validators", v.len());
            v
        }
        Err(e) => {
            tracing::error!("Failed to get validators: {}", e);
            return;
        }
    };

    let _ = action_tx.send(Action::SetLoadingProgress(0.3));

    // Fetch staker exposures for the previous era (active era - 1)
    let query_era = era_info.index.saturating_sub(1);
    let exposures = match client.get_era_stakers_overview(query_era).await {
        Ok(e) => {
            tracing::info!("Found {} validator exposures for era {}", e.len(), query_era);
            e
        }
        Err(e) => {
            tracing::error!("Failed to get era stakers: {}", e);
            Vec::new()
        }
    };

    let _ = action_tx.send(Action::SetLoadingProgress(0.6));

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

    let _ = action_tx.send(Action::SetLoadingProgress(0.7));

    // Fetch validator identities from People chain
    let identity_map: HashMap<String, String> = if let Some(ref people) = people_client {
        let addresses: Vec<subxt::utils::AccountId32> = validators
            .iter()
            .map(|v| v.address.clone())
            .collect();

        tracing::info!("Fetching identities for {} validators from People chain...", addresses.len());

        match people.get_identities(&addresses).await {
            Ok(identities) => {
                let with_names = identities
                    .iter()
                    .filter(|id| id.display_name.is_some())
                    .count();
                tracing::info!(
                    "Found {} identities ({} with display names)",
                    identities.len(),
                    with_names
                );
                identities
                    .into_iter()
                    .filter_map(|id| {
                        id.display_name.map(|name| (id.address.to_string(), name))
                    })
                    .collect()
            }
            Err(e) => {
                tracing::warn!("Failed to fetch identities: {}", e);
                HashMap::new()
            }
        }
    } else {
        tracing::info!("Skipping identity fetch (People chain not connected)");
        HashMap::new()
    };

    let _ = action_tx.send(Action::SetLoadingProgress(0.8));

    // Build exposure map for quick lookup
    let exposure_map: HashMap<[u8; 32], _> = exposures
        .iter()
        .map(|e| (*e.address.as_ref(), e.clone()))
        .collect();

    // Calculate for APY calculation
    let active_validator_count = exposures.len();

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

            // Calculate APY
            // Each validator gets an equal share of the era reward (simplified)
            let validator_share = if active_validator_count > 0 {
                era_reward / active_validator_count as u128
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
                points: 0, // TODO: fetch from reward points
                apy,
            })
        })
        .collect();

    // Sort by APY descending
    display_validators.sort_by(|a, b| b.apy.partial_cmp(&a.apy).unwrap_or(std::cmp::Ordering::Equal));

    // Build validator APY map for pool APY calculation (before sending)
    let validator_apy_map: HashMap<String, f64> = display_validators
        .iter()
        .map(|v| (v.address.clone(), v.apy))
        .collect();

    let _ = action_tx.send(Action::SetLoadingProgress(0.9));
    let _ = action_tx.send(Action::SetDisplayValidators(display_validators));

    tracing::info!("Validator data loaded successfully");

    // Fetch nomination pools
    let pools = match client.get_nomination_pools().await {
        Ok(p) => {
            tracing::info!("Found {} nomination pools", p.len());
            p
        }
        Err(e) => {
            tracing::error!("Failed to get nomination pools: {}", e);
            Vec::new()
        }
    };

    // Fetch pool metadata for names
    let metadata = match client.get_pool_metadata().await {
        Ok(m) => m,
        Err(e) => {
            tracing::error!("Failed to get pool metadata: {}", e);
            Vec::new()
        }
    };

    // Build metadata map for name lookup
    let metadata_map: HashMap<u32, String> = metadata.into_iter().map(|m| (m.id, m.name)).collect();

    // Build display pools with APY calculation
    let mut display_pools: Vec<DisplayPool> = Vec::with_capacity(pools.len());
    for p in pools {
        let name = metadata_map.get(&p.id).cloned().unwrap_or_default();

        // Calculate APY based on nominated validators
        let apy = match client.get_pool_nominations(p.id).await {
            Ok(Some(nominations)) if !nominations.targets.is_empty() => {
                // Calculate average APY from nominated validators
                let mut total_apy = 0.0;
                let mut count = 0;
                for target in &nominations.targets {
                    if let Some(&validator_apy) = validator_apy_map.get(&target.to_string()) {
                        total_apy += validator_apy;
                        count += 1;
                    }
                }
                if count > 0 {
                    Some(total_apy / count as f64)
                } else {
                    None
                }
            }
            _ => None,
        };

        display_pools.push(DisplayPool {
            id: p.id,
            name,
            state: p.state,
            member_count: p.member_count,
            points: p.points,
            apy,
        });
    }

    // Sort pools: by APY descending (pools with APY first, then by member count)
    display_pools.sort_by(|a, b| {
        match (a.apy, b.apy) {
            (Some(a_apy), Some(b_apy)) => b_apy.partial_cmp(&a_apy).unwrap_or(std::cmp::Ordering::Equal),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => b.member_count.cmp(&a.member_count),
        }
    });

    let _ = action_tx.send(Action::SetLoadingProgress(1.0));
    let _ = action_tx.send(Action::SetDisplayPools(display_pools));

    tracing::info!("Nomination pools loaded successfully");

    // Listen for account fetch and QR generation requests
    loop {
        tokio::select! {
            Some(account) = account_rx.recv() => {
                tracing::info!("Fetching account status for {}", account);

                // Fetch all account data
                let balance = match client.get_account_balance(&account).await {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::error!("Failed to get account balance: {}", e);
                        continue;
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

                let _ = account_action_tx.send(Action::SetAccountStatus(Box::new(status)));
                tracing::info!("Account status updated");
            }
            Some((signer, targets)) = qr_rx.recv() => {
                tracing::info!("Generating nomination QR for {} validators", targets.len());

                match client.create_nominate_payload(&signer, &targets).await {
                    Ok(payload) => {
                        let qr_data = stkopt_chain::encode_for_qr(&payload, &signer);
                        let qr_len = qr_data.len();
                        let _ = qr_action_tx.send(Action::SetQRData(Some(qr_data)));
                        tracing::info!("QR data generated ({} bytes for Polkadot Vault)", qr_len);
                    }
                    Err(e) => {
                        tracing::error!("Failed to generate nomination payload: {}", e);
                        let _ = qr_action_tx.send(Action::SetQRData(None));
                    }
                }
            }
            Some((account, num_eras, cancel_rx)) = history_rx.recv() => {
                tracing::info!("Loading staking history for {} ({} eras)", account, num_eras);

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
                    tracing::info!("Loaded {} cached history points", cached.len());
                    for point in cached {
                        let _ = history_action_tx.send(Action::AddStakingHistoryPoint(point));
                    }
                }

                // Get current era
                let current_era_info = match client.get_active_era().await {
                    Ok(Some(era)) => era,
                    Ok(None) => {
                        tracing::error!("No active era found");
                        let _ = history_action_tx.send(Action::HistoryLoadingComplete);
                        continue;
                    }
                    Err(e) => {
                        tracing::error!("Failed to get active era: {}", e);
                        let _ = history_action_tx.send(Action::HistoryLoadingComplete);
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
                    let _ = history_action_tx.send(Action::HistoryLoadingComplete);
                    continue;
                }

                tracing::info!("Fetching {} missing eras from chain", eras_to_fetch.len());
                let mut new_points = Vec::new();

                // Fetch missing eras
                for era in eras_to_fetch {
                    // Check for cancellation
                    if *cancel_rx.borrow() {
                        tracing::info!("History loading cancelled");
                        let _ = history_action_tx.send(Action::HistoryLoadingComplete);
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

                    // Get total staked for this era to calculate network-wide APY
                    let total_staked = match client.get_era_total_staked(era).await {
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

                    // Estimate user's reward proportional to their stake
                    let user_reward = if user_bonded > 0 && total_staked > 0 {
                        // User's share of the total reward
                        (era_reward as f64 * user_bonded as f64 / total_staked as f64) as u128
                    } else {
                        0
                    };

                    // Calculate date for this era
                    let era_date = calculate_era_date(era, current_era, current_era_start_ms, era_duration_ms);

                    let point = crate::action::StakingHistoryPoint {
                        era,
                        date: era_date.clone(),
                        reward: user_reward,
                        bonded: user_bonded,
                        apy,
                    };

                    new_points.push(point.clone());
                    let _ = history_action_tx.send(Action::AddStakingHistoryPoint(point));
                    tracing::debug!("Added history point for era {} (APY: {:.2}%)", era, apy * 100.0);
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
                    let _ = history_action_tx.send(Action::HistoryLoadingComplete);
                    tracing::info!("Staking history loaded");
                }
            }
            else => break,
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
    custom_rpc: Option<String>,
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

    let account: subxt::utils::AccountId32 = address.parse().map_err(|_| {
        color_eyre::eyre::eyre!("Invalid address format: {}", address)
    })?;

    println!("Updating staking history for {} on {}", address, network);

    // Open database
    let db_path = get_db_path();
    let mut db = db::HistoryDb::open(&db_path).map_err(|e| {
        color_eyre::eyre::eyre!("Failed to open database at {}: {}", db_path.display(), e)
    })?;

    println!("Database: {}", db_path.display());

    // Create connection status channel (not used in update mode, but required by API)
    let (status_tx, _status_rx) = mpsc::unbounded_channel::<ConnectionStatus>();

    // Connect to chain
    println!("Connecting to {} Asset Hub...", network);
    let client = ChainClient::connect_rpc(
        network,
        custom_rpc.as_deref(),
        status_tx,
    )
    .await?;
    println!("Connected");

    // Get current era info
    let current_era_info = client
        .get_active_era()
        .await?
        .ok_or_else(|| color_eyre::eyre::eyre!("No active era found"))?;

    let current_era = current_era_info.index;
    let current_era_start_ms = current_era_info.start_timestamp_ms;
    let era_duration_ms = client.get_era_duration_ms().await.unwrap_or(24 * 60 * 60 * 1000);

    println!("Current era: {} ({}ms per era)", current_era, era_duration_ms);

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

        // Get total staked
        let total_staked = match client.get_era_total_staked(era).await {
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
            date: era_date,
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
fn calculate_era_date(era: u32, current_era: u32, current_era_start_ms: u64, era_duration_ms: u64) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let eras_ago = current_era.saturating_sub(era);

    // If current_era_start_ms is 0 (unavailable), use current time as reference
    let era_start_ms = if current_era_start_ms > 0 {
        current_era_start_ms.saturating_sub(eras_ago as u64 * era_duration_ms)
    } else {
        // Fallback: use current time minus eras_ago * era_duration
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
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
