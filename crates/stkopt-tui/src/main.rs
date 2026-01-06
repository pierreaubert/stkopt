//! Staking Optimizer TUI - A terminal interface for Polkadot staking optimization.

mod action;
mod app;
mod event;
mod log_buffer;
mod tui;
mod ui;

use action::{AccountStatus, Action};
use app::App;
use clap::Parser;
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
    let custom_rpc = args.rpc_url;

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

    // Create application state
    let mut app = App::new(network, log_buffer);

    // Initialize terminal
    let mut tui = Tui::new()?;
    tui.enter()?;

    // Create event handler
    let mut events = EventHandler::new(250);

    // Spawn chain connection task
    let chain_action_tx = action_tx.clone();
    let account_action_tx = action_tx.clone();
    let qr_action_tx = action_tx.clone();
    tokio::spawn(async move {
        chain_task(network, custom_rpc, chain_action_tx, account_rx, account_action_tx, qr_rx, qr_action_tx).await;
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

    // Connect to the network
    let client = match ChainClient::connect_rpc(network, custom_rpc.as_deref(), status_tx).await {
        Ok(client) => {
            tracing::info!(
                "Connected to {} (genesis: {:?})",
                network,
                client.genesis_hash()
            );
            client
        }
        Err(e) => {
            tracing::error!("Failed to connect: {}", e);
            let _ = action_tx.send(Action::UpdateConnectionStatus(ConnectionStatus::Error(
                e.to_string(),
            )));
            return;
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

            Some(DisplayValidator {
                address: v.address.to_string(),
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
                        let qr_data = stkopt_chain::encode_for_qr(&payload);
                        let _ = qr_action_tx.send(Action::SetQRData(Some(qr_data)));
                        tracing::info!("QR data generated ({} bytes)", payload.call_data.len());
                    }
                    Err(e) => {
                        tracing::error!("Failed to generate nomination payload: {}", e);
                        let _ = qr_action_tx.send(Action::SetQRData(None));
                    }
                }
            }
            else => break,
        }
    }
}
