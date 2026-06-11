//! Chain background task for blockchain operations.

use crate::action::{
    AccountStatus, Action, DisplayPool, DisplayValidator, PendingUnsignedTx, StakingHistoryPoint,
    TransactionInfo,
};
use crate::db;
use color_eyre::Result;
use std::collections::HashMap;
use stkopt_chain::{ChainClient, ConnectionConfig, RewardDestination};
use stkopt_core::{ConnectionStatus, Network};
use subxt::utils::AccountId32;
use tokio::sync::mpsc;

const PEOPLE_READY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const MS_PER_DAY: u64 = 24 * 60 * 60 * 1000;

fn eras_for_lookback_days(lookback_days: u32, era_duration_ms: u64) -> u32 {
    if lookback_days == 0 || era_duration_ms == 0 {
        return 1;
    }
    let lookback_ms = lookback_days as u64 * MS_PER_DAY;
    lookback_ms.div_ceil(era_duration_ms).max(1) as u32
}

fn pool_nomination_apy(
    nominations: &stkopt_chain::PoolNominations,
    validator_apy_map: &HashMap<String, f64>,
) -> Option<f64> {
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

/// Staking operation to be performed by the chain task.
#[derive(Debug)]
pub enum StakingOp {
    // Direct staking
    Bond {
        signer: AccountId32,
        value: u128,
    },
    Unbond {
        signer: AccountId32,
        value: u128,
    },
    BondExtra {
        signer: AccountId32,
        value: u128,
    },
    SetPayee {
        signer: AccountId32,
        destination: RewardDestination,
    },
    WithdrawUnbonded {
        signer: AccountId32,
    },
    Chill {
        signer: AccountId32,
    },

    // Pool operations
    PoolJoin {
        signer: AccountId32,
        pool_id: u32,
        amount: u128,
    },
    PoolBondExtra {
        signer: AccountId32,
        amount: u128,
    },
    PoolClaim {
        signer: AccountId32,
    },
    PoolUnbond {
        signer: AccountId32,
        amount: u128,
    },
    PoolWithdraw {
        signer: AccountId32,
    },
}

/// Unified request type for all chain operations.
#[derive(Debug)]
pub enum ChainRequest {
    /// Fetch account data.
    FetchAccount(AccountId32),
    /// Generate QR code for nomination.
    GenerateNominationQR {
        signer: AccountId32,
        targets: Vec<AccountId32>,
    },
    /// Load staking history.
    FetchHistory {
        account: AccountId32,
        lookback_days: u32,
        cancel_rx: tokio::sync::watch::Receiver<bool>,
    },
    /// Execute a staking operation (generates QR).
    ExecuteStakingOp(StakingOp),
    /// Submit a signed transaction.
    SubmitTransaction(Vec<u8>),
    /// Reconnect to a different network.
    Reconnect(Network),
}

/// Build TransactionInfo from a payload for display purposes.
fn build_tx_info(
    payload: &stkopt_chain::UnsignedPayload,
    signer: &AccountId32,
    targets: Vec<String>,
) -> TransactionInfo {
    TransactionInfo {
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
    signer: AccountId32,
    targets: Vec<String>,
) {
    let qr_data = match stkopt_chain::encode_for_qr(&payload, &signer) {
        Ok(data) => data,
        Err(e) => {
            tracing::error!("Failed to encode signing QR: {}", e);
            clear_staking_qr(action_tx).await;
            let _ = action_tx
                .send(Action::QrScanFailed(format!(
                    "Failed to encode signing QR: {}",
                    e
                )))
                .await;
            return;
        }
    };
    let tx_info = build_tx_info(&payload, &signer, targets);
    let pending = PendingUnsignedTx { payload, signer };
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

/// Helper to attempt reconnection.
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

async fn connect_ready_people_client(
    client: &ChainClient,
    network: Network,
) -> Option<stkopt_chain::PeopleChainClient> {
    match client.connect_people_chain_client().await {
        Ok(people) => match people.wait_until_ready(PEOPLE_READY_TIMEOUT).await {
            Ok(block) => {
                tracing::info!("Connected to {} People chain at block {}", network, block);
                Some(people)
            }
            Err(e) => {
                tracing::warn!(
                    "People chain connected but did not become ready (identities unavailable): {}",
                    e
                );
                None
            }
        },
        Err(e) => {
            tracing::warn!(
                "Failed to connect to People chain (identities unavailable): {}",
                e
            );
            None
        }
    }
}

/// Background task for chain operations.
pub async fn chain_task(
    mut network: Network,
    config: ConnectionConfig,
    action_tx: mpsc::Sender<Action>,
    mut request_rx: mpsc::Receiver<ChainRequest>,
) {
    use stkopt_core::get_era_apy;

    // Create status channel for connection updates (bounded for backpressure)
    const STATUS_CHANNEL_CAPACITY: usize = 10;
    let (status_tx, mut status_rx) = mpsc::channel::<ConnectionStatus>(STATUS_CHANNEL_CAPACITY);

    // Forward status updates to action channel
    let action_tx_for_status = action_tx.clone();
    tokio::spawn(async move {
        while let Some(status) = status_rx.recv().await {
            if matches!(status, ConnectionStatus::Connected) {
                tracing::debug!(
                    "Ignoring low-level chain Connected status until app startup data is ready"
                );
                continue;
            }
            let _ = action_tx_for_status
                .send(Action::UpdateConnectionStatus(status))
                .await;
        }
    });

    // Connect to chain (light client or RPC based on config)
    let mut client = match ChainClient::connect(network, &config, status_tx.clone()).await {
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

    // Send chain info for UI display and validation
    match client.get_chain_info().await {
        Ok(chain_info) => {
            let _ = action_tx.send(Action::SetChainInfo(chain_info)).await;
        }
        Err(e) => {
            tracing::warn!("Failed to get chain info: {}", e);
        }
    }

    // Connect to People chain for identity queries
    let mut people_client = connect_ready_people_client(&client, network).await;

    // Longer delay to let light client connection stabilize
    if client.is_light_client() {
        tracing::info!("Waiting for light client to stabilize...");
        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
    } else {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }

    // Fetch era info
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

    // Load cached identities immediately
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

    // Fetch validators
    let validators = {
        let mut result = None;
        let mut reconnect_attempts = 0;
        const MAX_RECONNECT_ATTEMPTS: u32 = 3;

        'outer: loop {
            let fetch_result = if client.is_light_client() {
                tracing::info!("Fetching validators via light client (multi-source approach)...");
                client.get_validators_light_client().await
            } else {
                client.get_validators().await
            };

            match fetch_result {
                Ok(v) => {
                    if client.is_light_client() {
                        tracing::info!("Light client: Found {} validators (partial data)", v.len());
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
                match client.get_chain_info().await {
                    Ok(chain_info) => {
                        let _ = action_tx.send(Action::SetChainInfo(chain_info)).await;
                    }
                    Err(e) => {
                        tracing::warn!("Failed to get chain info after reconnect: {}", e);
                    }
                }
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

    bytes_transferred += validators.len() as u64 * 200;
    let _ = action_tx
        .send(Action::SetLoadingProgress(
            0.3,
            Some(bytes_transferred),
            None,
        ))
        .await;

    // Fetch staker exposures
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

    bytes_transferred += exposures.len() as u64 * 100;
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

    let points_len = validator_points.len();
    let points_map: HashMap<[u8; 32], u32> = validator_points
        .into_iter()
        .map(|vp| (*vp.address.as_ref(), vp.points))
        .collect();

    bytes_transferred += 500 + points_len as u64 * 50;
    let _ = action_tx
        .send(Action::SetLoadingProgress(
            0.7,
            Some(bytes_transferred),
            None,
        ))
        .await;

    // Fetch fresh validator identities from People chain and update cache
    if let Some(ref people) = people_client {
        let addresses: Vec<AccountId32> = validators.iter().map(|v| v.address.clone()).collect();

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

                identity_map.extend(fresh_identities);
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to fetch identities from People chain: {} (using cached data)",
                    e
                );
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

    bytes_transferred += identity_map.len() as u64 * 100;
    let _ = action_tx
        .send(Action::SetLoadingProgress(
            0.8,
            Some(bytes_transferred),
            None,
        ))
        .await;

    // Build exposure map
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

            let (total_stake, own_stake, nominator_count) = match exposure {
                Some(e) => (e.total, e.own, e.nominator_count),
                None => return None,
            };

            let points = points_map.get(&addr_bytes).copied().unwrap_or(0);

            let apy = if era_reward > 0 && total_points > 0 && points > 0 && total_stake > 0 {
                let validator_share =
                    (era_reward as f64 * points as f64 / total_points as f64) as u128;
                let nominator_reward =
                    ((validator_share as f64) * (1.0 - v.preferences.commission)) as u128;
                Some(get_era_apy(nominator_reward, total_stake, era_duration_ms))
            } else {
                None
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
                apy,
            })
        })
        .collect();

    display_validators.sort_by(|a, b| {
        b.apy
            .unwrap_or(0.0)
            .partial_cmp(&a.apy.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

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

    // Cache validators
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
    if client.is_light_client() {
        tracing::info!("Stabilizing light client before pool queries...");
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        if !client.is_connected().await {
            tracing::warn!("Light client disconnected after validator loading, reconnecting...");
            if let Some(new_client) = try_reconnect(&client, 3).await {
                client = new_client;
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            }
        }
    }

    // Fetch nomination pools
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

    // Fetch pool metadata
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

    let metadata_map: HashMap<u32, String> = metadata.into_iter().map(|m| (m.id, m.name)).collect();
    tracing::info!(
        "Built pool metadata map with {} entries (pool IDs: {:?})",
        metadata_map.len(),
        metadata_map.keys().take(10).collect::<Vec<_>>()
    );

    // Build display pools
    let mut display_pools: Vec<DisplayPool> = Vec::with_capacity(pools.len());

    for p in &pools {
        let name = metadata_map.get(&p.id).cloned().unwrap_or_default();
        display_pools.push(DisplayPool {
            id: p.id,
            name,
            state: p.state.into(),
            member_count: p.member_count,
            total_bonded: p.points,
            commission: None,
            apy: None,
        });
    }

    let _ = action_tx
        .send(Action::SetDisplayPools(display_pools.clone()))
        .await;
    tracing::info!(
        "Sent {} pools to UI (fetching APY in background)",
        display_pools.len()
    );

    // Second pass: fetch APY for top pools
    let max_pools_to_query = 30.min(pools.len());
    let pool_ids: Vec<u32> = pools
        .iter()
        .take(max_pools_to_query)
        .map(|pool| pool.id)
        .collect();
    let nominations_map: HashMap<u32, stkopt_chain::PoolNominations> =
        match client.get_pool_nominations_batch(&pool_ids).await {
            Ok(nominations) => nominations
                .into_iter()
                .map(|nominations| (nominations.pool_id, nominations))
                .collect(),
            Err(e) => {
                tracing::warn!("Failed to get batched pool nominations: {}", e);
                HashMap::new()
            }
        };

    for (idx, p) in pools.iter().take(max_pools_to_query).enumerate() {
        let apy = if let Some(nominations) = nominations_map.get(&p.id) {
            let apy = pool_nomination_apy(nominations, &validator_apy_map);
            if let Some(apy) = apy {
                tracing::debug!(
                    "Pool {} has nominated validators with APY, avg: {:.2}%",
                    p.id,
                    apy * 100.0
                );
            } else if nominations.targets.is_empty() {
                tracing::debug!("Pool {} has empty nominations", p.id);
            } else {
                tracing::debug!(
                    "Pool {} has {} nominations but none found in validator map",
                    p.id,
                    nominations.targets.len()
                );
            }
            apy
        } else {
            tracing::debug!("Pool {} has no nominations", p.id);
            None
        };

        display_pools[idx].apy = apy;

        if (idx + 1) % 10 == 0 {
            let _ = action_tx
                .send(Action::SetDisplayPools(display_pools.clone()))
                .await;
            tracing::debug!("Updated APY for {} pools", idx + 1);
        }
    }

    display_pools.sort_by(|a, b| match (a.apy, b.apy) {
        (Some(a_apy), Some(b_apy)) => b_apy
            .partial_cmp(&a_apy)
            .unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => b.member_count.cmp(&a.member_count),
    });

    bytes_transferred += display_pools.len() as u64 * 150;
    let _ = action_tx
        .send(Action::SetLoadingProgress(
            1.0,
            Some(bytes_transferred),
            None,
        ))
        .await;
    let _ = action_tx.send(Action::SetDisplayPools(display_pools)).await;
    let _ = action_tx
        .send(Action::UpdateConnectionStatus(ConnectionStatus::Connected))
        .await;

    tracing::info!("Nomination pools loaded successfully");

    // Listen for requests from the UI
    while let Some(request) = request_rx.recv().await {
        match request {
            ChainRequest::FetchAccount(account) => {
                tracing::info!("Fetching account status for {}", account);

                let balance = match client.get_account_balance(&account).await {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::error!("Failed to get account balance: {}", e);
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
                        let _ = action_tx
                            .send(Action::QrScanFailed(format!(
                                "Failed to generate nomination QR: {}",
                                e
                            )))
                            .await;
                    }
                }
            }
            ChainRequest::FetchHistory {
                account,
                lookback_days,
                cancel_rx,
            } => {
                tracing::info!(
                    "Loading staking history for {} (last {} days)",
                    account,
                    lookback_days
                );

                let address = account.to_string();
                let db_path = get_db_path();
                let mut db = match db::HistoryDb::open(&db_path) {
                    Ok(db) => Some(db),
                    Err(e) => {
                        tracing::warn!("Failed to open history database: {}", e);
                        None
                    }
                };

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

                let era_duration_ms = match client.get_era_duration_ms().await {
                    Ok(d) => d,
                    Err(e) => {
                        tracing::warn!("Failed to get era duration, using default: {}", e);
                        24 * 60 * 60 * 1000
                    }
                };
                let num_eras = eras_for_lookback_days(lookback_days, era_duration_ms);
                let _ = action_tx.send(Action::SetHistoryTotalEras(num_eras)).await;

                if let Some(ref db) = db
                    && let Ok(cached) = db.get_history(network, &address, Some(num_eras))
                    && !cached.is_empty()
                {
                    let filtered: Vec<_> = cached.into_iter().filter(|h| h.apy <= 0.50).collect();
                    tracing::info!("Loaded {} cached history points (filtered)", filtered.len());
                    for point in filtered {
                        let _ = action_tx.send(Action::AddStakingHistoryPoint(point)).await;
                    }
                }

                let user_bonded = match client.get_staking_ledger(&account).await {
                    Ok(Some(ledger)) => ledger.active,
                    _ => 0,
                };

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

                for era in eras_to_fetch {
                    if *cancel_rx.borrow() {
                        tracing::info!("History loading cancelled");
                        let _ = action_tx.send(Action::HistoryLoadingComplete).await;
                        break;
                    }

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

                    let apy = get_era_apy(era_reward, total_staked, era_duration_ms);

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

                    let user_reward = if user_bonded > 0 && total_staked > 0 {
                        let estimated =
                            (era_reward as f64 * user_bonded as f64 / total_staked as f64) as u128;
                        let max_reasonable_reward = user_bonded / 200;
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

                    let era_date =
                        calculate_era_date(era, current_era, current_era_start_ms, era_duration_ms);

                    let point = StakingHistoryPoint {
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

                if let Some(ref mut db) = db
                    && !new_points.is_empty()
                {
                    if let Err(e) = db.insert_history_batch(network, &address, &new_points) {
                        tracing::warn!("Failed to cache history: {}", e);
                    } else {
                        tracing::info!("Cached {} new history points", new_points.len());
                    }
                }

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

                        match progress.wait_for_finalized().await {
                            Ok(result) => {
                                tracing::info!(
                                    "Transaction finalized in block 0x{}",
                                    hex::encode(result.block_hash)
                                );
                                let _ = action_tx
                                    .send(Action::SetTxStatus(
                                        crate::action::TxSubmissionStatus::Finalized {
                                            block_hash: result.block_hash,
                                        },
                                    ))
                                    .await;
                            }
                            Err(e) => {
                                tracing::error!("Transaction failed: {}", e);
                                let _ = action_tx
                                    .send(Action::SetTxStatus(
                                        crate::action::TxSubmissionStatus::Failed(e.to_string()),
                                    ))
                                    .await;
                            }
                        }
                    }
                    Err(e) => {
                        let error_str = e.to_string();
                        tracing::error!("Failed to submit transaction: {}", error_str);

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
                            .send(Action::SetTxStatus(
                                crate::action::TxSubmissionStatus::Failed(user_message),
                            ))
                            .await;
                    }
                }
            }
            ChainRequest::Reconnect(new_network) => {
                tracing::info!("Switching network from {} to {}", network, new_network);
                network = new_network;
                match ChainClient::connect(network, &config, status_tx.clone()).await {
                    Ok(new_client) => {
                        tracing::info!(
                            "Connected to {} Asset Hub via {} (genesis: {:?})",
                            network,
                            new_client.connection_mode(),
                            new_client.genesis_hash()
                        );
                        client = new_client;
                        match client.get_chain_info().await {
                            Ok(chain_info) => {
                                let _ = action_tx.send(Action::SetChainInfo(chain_info)).await;
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Failed to get chain info after network switch: {}",
                                    e
                                );
                            }
                        }
                        #[allow(unused_assignments)]
                        {
                            people_client = connect_ready_people_client(&client, network).await;
                        }
                        let _ = action_tx
                            .send(Action::UpdateConnectionStatus(ConnectionStatus::Connected))
                            .await;
                    }
                    Err(e) => {
                        tracing::error!("Failed to connect to {}: {}", network, e);
                        let _ = action_tx
                            .send(Action::UpdateConnectionStatus(ConnectionStatus::Error(
                                e.to_string(),
                            )))
                            .await;
                    }
                }
            }
        }
    }
}

/// Get the path to the history database file.
pub fn get_db_path() -> std::path::PathBuf {
    if let Some(proj_dirs) = directories::ProjectDirs::from("io", "stkopt", "stkopt") {
        let data_dir = proj_dirs.data_dir();
        std::fs::create_dir_all(data_dir).ok();
        data_dir.join("history.db")
    } else {
        std::path::PathBuf::from("stkopt_history.db")
    }
}

/// Run in update mode: fetch missing history and store to database, then exit.
pub async fn run_update_mode(
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

    let account: AccountId32 = address
        .parse()
        .map_err(|_| color_eyre::eyre::eyre!("Invalid address format: {}", address))?;

    println!("Updating staking history for {} on {}", address, network);

    let db_path = get_db_path();
    let mut db = db::HistoryDb::open(&db_path).map_err(|e| {
        color_eyre::eyre::eyre!("Failed to open database at {}: {}", db_path.display(), e)
    })?;

    println!("Database: {}", db_path.display());

    let (status_tx, _status_rx) = mpsc::channel::<ConnectionStatus>(1);

    println!("Connecting to {} Asset Hub...", network);
    let client = ChainClient::connect(network, &config, status_tx).await?;
    println!("Connected via {}", client.connection_mode());

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

    let user_bonded = match client.get_staking_ledger(&account).await {
        Ok(Some(ledger)) => ledger.active,
        _ => 0,
    };

    let start_era = current_era.saturating_sub(num_eras);

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

        let apy = get_era_apy(era_reward, total_staked, era_duration_ms);

        let user_reward = if user_bonded > 0 && total_staked > 0 {
            (era_reward as f64 * user_bonded as f64 / total_staked as f64) as u128
        } else {
            0
        };

        let era_date = calculate_era_date(era, current_era, current_era_start_ms, era_duration_ms);

        let point = StakingHistoryPoint {
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
pub fn calculate_era_date(
    era: u32,
    current_era: u32,
    current_era_start_ms: u64,
    era_duration_ms: u64,
) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let eras_ago = current_era.saturating_sub(era);

    let era_start_ms = if current_era_start_ms > 0 {
        current_era_start_ms.saturating_sub(eras_ago as u64 * era_duration_ms)
    } else {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                tracing::warn!("Time regression detected: {}, using 0", e);
            })
            .unwrap_or_default()
            .as_millis() as u64;
        now_ms.saturating_sub(eras_ago as u64 * era_duration_ms)
    };

    chrono::DateTime::from_timestamp_millis(era_start_ms as i64)
        .map(|dt| dt.format("%Y%m%d").to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(byte: u8) -> AccountId32 {
        AccountId32::from([byte; 32])
    }

    #[test]
    fn pool_nomination_apy_averages_known_targets() {
        let known_a = account(1);
        let known_b = account(2);
        let missing = account(3);
        let nominations = stkopt_chain::PoolNominations {
            pool_id: 7,
            stash: account(9),
            targets: vec![known_a, missing, known_b],
        };
        let validator_apy_map =
            HashMap::from([(known_a.to_string(), 0.12), (known_b.to_string(), 0.06)]);

        assert_eq!(
            pool_nomination_apy(&nominations, &validator_apy_map),
            Some(0.09)
        );
    }

    #[test]
    fn pool_nomination_apy_is_none_without_known_targets() {
        let nominations = stkopt_chain::PoolNominations {
            pool_id: 7,
            stash: account(9),
            targets: vec![account(3)],
        };

        assert_eq!(pool_nomination_apy(&nominations, &HashMap::new()), None);
    }

    #[test]
    fn test_eras_for_lookback_days_uses_era_duration() {
        assert_eq!(eras_for_lookback_days(30, MS_PER_DAY), 30);
        assert_eq!(eras_for_lookback_days(30, 6 * 60 * 60 * 1000), 120);
        assert_eq!(eras_for_lookback_days(1, 7 * 60 * 60 * 1000), 4);
    }

    #[test]
    fn test_eras_for_lookback_days_never_returns_zero() {
        assert_eq!(eras_for_lookback_days(0, MS_PER_DAY), 1);
        assert_eq!(eras_for_lookback_days(30, 0), 1);
    }

    // --- build_tx_info tests ---

    fn make_test_unsigned_payload() -> stkopt_chain::UnsignedPayload {
        stkopt_chain::UnsignedPayload {
            call_data: vec![0x01, 0x02, 0x03],
            description: "Staking.nominate".to_string(),
            metadata_hash: [0u8; 32],
            genesis_hash: [0u8; 32],
            block_hash: [0u8; 32],
            spec_version: 1_002_000,
            tx_version: 26,
            nonce: 42,
            era: stkopt_chain::Era::Immortal,
            include_metadata_hash: true,
            use_asset_payment: false,
            extension_ids: vec!["CheckNonce".to_string()],
        }
    }

    #[test]
    fn test_build_tx_info_maps_fields_correctly() {
        let payload = make_test_unsigned_payload();
        let signer = account(5);
        let targets = vec!["target1".to_string(), "target2".to_string()];

        let info = build_tx_info(&payload, &signer, targets.clone());

        assert_eq!(info.signer, signer.to_string());
        assert_eq!(info.call, "Staking.nominate");
        assert_eq!(info.targets, targets);
        assert_eq!(info.call_data_size, 3);
        assert_eq!(info.spec_version, 1_002_000);
        assert_eq!(info.tx_version, 26);
        assert_eq!(info.nonce, 42);
        assert!(info.include_metadata_hash);
    }

    #[test]
    fn test_build_tx_info_empty_targets() {
        let payload = make_test_unsigned_payload();
        let signer = account(7);
        let info = build_tx_info(&payload, &signer, vec![]);

        assert!(info.targets.is_empty());
        assert_eq!(info.signer, signer.to_string());
    }

    // --- get_db_path tests ---

    #[test]
    fn test_get_db_path_returns_non_empty() {
        let path = get_db_path();
        assert!(!path.as_os_str().is_empty());
    }

    #[test]
    fn test_get_db_path_ends_with_history_db() {
        let path = get_db_path();
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("history.db")
        );
    }

    // --- calculate_era_date tests ---

    #[test]
    fn test_calculate_era_date_current_era() {
        // era == current_era, so eras_ago == 0
        let date = calculate_era_date(100, 100, 1_700_000_000_000, 86_400_000);
        // Should be 1_700_000_000_000 ms since epoch
        let expected = chrono::DateTime::from_timestamp_millis(1_700_000_000_000)
            .unwrap()
            .format("%Y%m%d")
            .to_string();
        assert_eq!(date, expected);
    }

    #[test]
    fn test_calculate_era_date_past_era() {
        // 5 eras ago
        let date = calculate_era_date(95, 100, 1_700_000_000_000, 86_400_000);
        let expected_ms = 1_700_000_000_000u64 - 5 * 86_400_000;
        let expected = chrono::DateTime::from_timestamp_millis(expected_ms as i64)
            .unwrap()
            .format("%Y%m%d")
            .to_string();
        assert_eq!(date, expected);
    }

    #[test]
    fn test_calculate_era_date_zero_start_ms_fallback() {
        // When current_era_start_ms is 0, it falls back to current time minus eras_ago
        let date = calculate_era_date(90, 100, 0, 86_400_000);
        // Should return a non-empty 8-digit date string (YYYYMMDD)
        assert_eq!(date.len(), 8);
        assert!(date.parse::<u64>().is_ok());
    }

    #[test]
    fn test_calculate_era_date_era_greater_than_current() {
        // era > current_era: saturating_sub yields 0, so it uses current_era_start_ms
        let date = calculate_era_date(200, 100, 1_700_000_000_000, 86_400_000);
        let expected = chrono::DateTime::from_timestamp_millis(1_700_000_000_000)
            .unwrap()
            .format("%Y%m%d")
            .to_string();
        assert_eq!(date, expected);
    }

    #[test]
    fn test_calculate_era_date_zero_duration() {
        // Zero duration should still work (no change in ms per era)
        let date = calculate_era_date(99, 100, 1_700_000_000_000, 0);
        let expected = chrono::DateTime::from_timestamp_millis(1_700_000_000_000)
            .unwrap()
            .format("%Y%m%d")
            .to_string();
        assert_eq!(date, expected);
    }
}
