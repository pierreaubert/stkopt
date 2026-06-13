//! Chain background task for blockchain operations.

use crate::action::{
    AccountStatus, Action, DisplayValidator, PendingUnsignedTx, StakingHistoryPoint,
    TransactionInfo,
};
use crate::db;
use color_eyre::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use stkopt_chain::{
    AccountBalance, ChainClient, ConnectionConfig, DisplayValidatorEnrichment, NominatorInfo,
    PoolEnrichmentOutcome, PoolMembership, RewardDestination, StakingLedger, UnlockChunk,
    ValidatorEnrichmentOutcome, basic_display_pools, basic_display_validators,
    eras_for_lookback_days, fetch_and_enrich_pools, fetch_and_enrich_validators, pool_metadata_map,
    staking_history_point, validator_apy_map,
};
use stkopt_core::{
    AccountStatusService, CachePolicy, CachedAccountStatus, CachedChainMetadata, ConnectionStatus,
    HistoryService, Network, StartupDataService,
};
use subxt::utils::AccountId32;
use tokio::sync::mpsc;

use futures::stream::{self, StreamExt};
use std::future::Future;

const PEOPLE_READY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

pub(crate) fn cached_validators_have_chain_data(validators: &[DisplayValidator]) -> bool {
    validators.iter().any(|validator| validator.total_stake > 0)
        && validators.iter().any(|validator| validator.apy.is_some())
}

/// Resolve the slashing span count from a chain lookup, defaulting to `0` on
/// error. This mirrors the previous hard-coded behaviour while making the
/// value explicit and testable.
fn slashing_spans_from_result(result: Result<u32, stkopt_chain::ChainError>) -> u32 {
    result.unwrap_or(0)
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

fn account_status_from_cache(address: AccountId32, status: &CachedAccountStatus) -> AccountStatus {
    let nomination_targets: Vec<AccountId32> = status
        .nominations_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<Vec<String>>(json).ok())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|address| address.parse().ok())
        .collect();

    let unlocking: Vec<UnlockChunk> = status
        .unlocking_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<Vec<UnlockChunk>>(json).ok())
        .unwrap_or_default();

    let unbonding_eras: Vec<(u32, u128)> = status
        .pool_unbonding_eras_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<Vec<(u32, u128)>>(json).ok())
        .unwrap_or_default();

    AccountStatus {
        address: address.clone(),
        balance: AccountBalance {
            free: status.free_balance,
            reserved: status.reserved_balance,
            frozen: status.frozen_balance,
        },
        staking_ledger: (status.staked_amount > 0 || !unlocking.is_empty()).then_some(
            StakingLedger {
                stash: address.clone(),
                total: status.staked_amount,
                active: status.staked_amount,
                unlocking,
            },
        ),
        nominations: (!nomination_targets.is_empty()).then_some(NominatorInfo {
            targets: nomination_targets,
            submitted_in: 0,
        }),
        pool_membership: status.pool_id.map(|pool_id| PoolMembership {
            pool_id,
            points: status.pool_points.unwrap_or_default(),
            unbonding_eras,
            last_recorded_reward_counter: status.pool_last_recorded_reward_counter,
        }),
    }
}

fn cached_account_status_from_live(status: &AccountStatus) -> CachedAccountStatus {
    let unlocking_json = status.staking_ledger.as_ref().and_then(|ledger| {
        if ledger.unlocking.is_empty() {
            None
        } else {
            serde_json::to_string(&ledger.unlocking).ok()
        }
    });

    let (pool_unbonding_eras_json, pool_last_recorded_reward_counter) = status
        .pool_membership
        .as_ref()
        .map(|membership| {
            let json = if membership.unbonding_eras.is_empty() {
                None
            } else {
                serde_json::to_string(&membership.unbonding_eras).ok()
            };
            (json, membership.last_recorded_reward_counter)
        })
        .unwrap_or_default();

    CachedAccountStatus {
        free_balance: status.balance.free,
        reserved_balance: status.balance.reserved,
        frozen_balance: status.balance.frozen,
        staked_amount: status
            .staking_ledger
            .as_ref()
            .map(|ledger| ledger.active)
            .unwrap_or_default(),
        nominations_json: status.nominations.as_ref().and_then(|nominations| {
            let targets: Vec<String> = nominations
                .targets
                .iter()
                .map(|target| target.to_string())
                .collect();
            if targets.is_empty() {
                None
            } else {
                serde_json::to_string(&targets).ok()
            }
        }),
        pool_id: status
            .pool_membership
            .as_ref()
            .map(|membership| membership.pool_id),
        pool_points: status
            .pool_membership
            .as_ref()
            .map(|membership| membership.points),
        unlocking_json,
        pool_unbonding_eras_json,
        pool_last_recorded_reward_counter,
    }
}

/// Fetch a single era's staking history point.
///
/// `get_reward` and `get_stake` are async callbacks that return the raw chain
/// values for the era. They are generic so this helper can be unit-tested
/// without a real `ChainClient`.
async fn fetch_history_era<RewardFut, StakeFut>(
    era: u32,
    current_era: u32,
    current_era_start_ms: u64,
    era_duration_ms: u64,
    user_bonded: u128,
    get_reward: impl FnOnce(u32) -> RewardFut,
    get_stake: impl FnOnce(u32) -> StakeFut,
) -> Option<StakingHistoryPoint>
where
    RewardFut: Future<Output = Result<Option<u128>, stkopt_chain::ChainError>>,
    StakeFut: Future<Output = Result<u128, stkopt_chain::ChainError>>,
{
    let era_reward = match get_reward(era).await {
        Ok(Some(reward)) => reward,
        Ok(None) => {
            tracing::debug!("No reward data for era {}", era);
            return None;
        }
        Err(e) => {
            tracing::warn!("Failed to get era {} reward: {}", era, e);
            return None;
        }
    };

    let total_staked = match get_stake(era).await {
        Ok(staked) if staked > 0 => staked,
        Ok(_) => {
            tracing::debug!("No stake data for era {}", era);
            return None;
        }
        Err(e) => {
            tracing::warn!("Failed to get era {} total staked: {}", era, e);
            return None;
        }
    };

    let point = staking_history_point(
        era,
        current_era,
        current_era_start_ms,
        era_duration_ms,
        era_reward,
        user_bonded,
        total_staked,
    );

    if !HistoryService::is_valid_cached_apy(point.apy, CachePolicy::default()) {
        tracing::warn!(
            "Era {} has unrealistic APY {:.2}% (reward={}, staked={}), skipping",
            era,
            point.apy * 100.0,
            era_reward,
            total_staked
        );
        return None;
    }

    Some(point)
}

/// Background task for chain operations.
pub async fn chain_task(
    mut network: Network,
    config: ConnectionConfig,
    action_tx: mpsc::Sender<Action>,
    mut request_rx: mpsc::Receiver<ChainRequest>,
) {
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
    let chain_info = match client.get_chain_info().await {
        Ok(chain_info) => {
            let _ = action_tx
                .send(Action::SetChainInfo(chain_info.clone()))
                .await;
            Some(chain_info)
        }
        Err(e) => {
            tracing::warn!("Failed to get chain info: {}", e);
            None
        }
    };

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
    let db_path = prepare_db_path();
    let mut db = db::HistoryDb::open(&db_path).ok();
    if let (Some(ref db), Some(ref chain_info)) = (db.as_ref(), chain_info.as_ref()) {
        let (token_symbol, token_decimals, ss58_prefix) = (
            network.token_symbol().to_string(),
            network.token_decimals(),
            network.ss58_format(),
        );
        let metadata = CachedChainMetadata {
            genesis_hash: hex::encode(client.genesis_hash()),
            spec_version: chain_info.spec_version,
            tx_version: chain_info.tx_version,
            ss58_prefix,
            token_symbol,
            token_decimals,
            era_duration_ms,
            current_era: era_info.index,
        };
        if let Err(e) = db.set_chain_metadata(network, &metadata) {
            tracing::warn!("Failed to cache chain metadata: {}", e);
        }
    }

    let mut cached_validator_apy_map = HashMap::new();
    let mut validators_cached = false;
    let mut pools_cached = false;
    if let Some(ref db) = db {
        match StartupDataService::load(db, network, era_info.index, CachePolicy::default()) {
            Ok(startup) => {
                if startup.validators.is_displayable() && !startup.validators.data.is_empty() {
                    let has_chain_data =
                        cached_validators_have_chain_data(&startup.validators.data);
                    validators_cached = !startup.validators.needs_refresh() && has_chain_data;
                    cached_validator_apy_map = if has_chain_data {
                        validator_apy_map(&startup.validators.data)
                    } else {
                        HashMap::new()
                    };
                    tracing::info!(
                        "Using {:?} startup validator cache: {} validators for era {} (chain_data={})",
                        startup.validators.freshness,
                        startup.validators.data.len(),
                        era_info.index,
                        has_chain_data
                    );
                    if has_chain_data {
                        bytes_transferred += startup.validators.data.len() as u64 * 200;
                        let _ = action_tx
                            .send(Action::SetLoadingProgress(
                                0.45,
                                Some(bytes_transferred),
                                None,
                            ))
                            .await;
                        let _ = action_tx
                            .send(Action::SetDisplayValidators(startup.validators.data))
                            .await;
                    }
                }

                if startup.pools.is_displayable() && !startup.pools.data.is_empty() {
                    pools_cached = !startup.pools.needs_refresh();
                    tracing::info!(
                        "Using {:?} startup pool cache: {} pools for era {}",
                        startup.pools.freshness,
                        startup.pools.data.len(),
                        era_info.index
                    );
                    bytes_transferred += startup.pools.data.len() as u64 * 150;
                    let _ = action_tx
                        .send(Action::SetLoadingProgress(
                            0.85,
                            Some(bytes_transferred),
                            None,
                        ))
                        .await;
                    let _ = action_tx
                        .send(Action::SetDisplayPools(startup.pools.data))
                        .await;
                }
            }
            Err(e) => tracing::debug!("Failed to load startup cache: {}", e),
        }
    }

    if validators_cached && pools_cached {
        let _ = action_tx
            .send(Action::SetLoadingProgress(
                1.0,
                Some(bytes_transferred),
                None,
            ))
            .await;
        let _ = action_tx
            .send(Action::UpdateConnectionStatus(ConnectionStatus::Connected))
            .await;
    }

    if !validators_cached {
        let people_client = connect_ready_people_client(&client, network).await;

        // Load cached identities immediately
        let mut identity_map: HashMap<String, String> = if let Some(ref db) = db {
            match db.get_validator_identities_within_age(
                network,
                stkopt_core::DEFAULT_IDENTITY_MAX_AGE_SECS,
            ) {
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
            let mut result: Option<stkopt_chain::ValidatorFetch> = None;
            let mut reconnect_attempts = 0;
            const MAX_RECONNECT_ATTEMPTS: u32 = 3;

            'outer: loop {
                let fetch_result = if client.is_light_client() {
                    tracing::info!(
                        "Fetching validators via light client (multi-source approach)..."
                    );
                    client.get_validators_light_client_with_completeness().await
                } else {
                    client
                        .get_validators()
                        .await
                        .map(|validators| stkopt_chain::ValidatorFetch {
                            validators,
                            complete: true,
                        })
                };

                match fetch_result {
                    Ok(fetch) => {
                        if client.is_light_client() {
                            tracing::info!(
                                "Light client: Found {} validators (complete={})",
                                fetch.validators.len(),
                                fetch.complete
                            );
                        } else {
                            tracing::info!(
                                "Found {} registered validators",
                                fetch.validators.len()
                            );
                        }
                        result = Some(fetch);
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
                Some(fetch) => fetch,
                None => {
                    tracing::error!("Could not fetch validators - cannot continue");
                    return;
                }
            }
        };
        let validators_complete = validators.complete;
        let validators = validators.validators;

        bytes_transferred += validators.len() as u64 * 200;
        let _ = action_tx
            .send(Action::SetLoadingProgress(
                0.3,
                Some(bytes_transferred),
                None,
            ))
            .await;

        // Fetch per-validator APY inputs and enrich validators.
        let query_era = era_info.index.saturating_sub(1);
        let enrichment_outcome = match fetch_and_enrich_validators(
            &client,
            &validators,
            people_client.as_ref(),
            identity_map.clone(),
            query_era,
            era_duration_ms,
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(e) => {
                tracing::error!("Failed to enrich validators: {}, using basic data", e);
                ValidatorEnrichmentOutcome {
                    enrichment: DisplayValidatorEnrichment {
                        validators: basic_display_validators(&validators),
                        apy_era: query_era,
                        validators_with_apy: 0,
                    },
                    fresh_identities: HashMap::new(),
                    updated_identity_map: identity_map,
                    apy_data: None,
                }
            }
        };

        let validator_apy_data = enrichment_outcome.apy_data;
        let exposures_len = validator_apy_data
            .as_ref()
            .map(|data| data.exposures.len())
            .unwrap_or_default();

        bytes_transferred += exposures_len as u64 * 100;
        let _ = action_tx
            .send(Action::SetLoadingProgress(
                0.6,
                Some(bytes_transferred),
                None,
            ))
            .await;

        let points_len = validator_apy_data
            .as_ref()
            .map(|data| data.points.len())
            .unwrap_or_default();

        bytes_transferred += 500 + points_len as u64 * 50;
        let _ = action_tx
            .send(Action::SetLoadingProgress(
                0.7,
                Some(bytes_transferred),
                None,
            ))
            .await;

        if !enrichment_outcome.fresh_identities.is_empty() {
            tracing::info!(
                "Found {} new validator display names from People chain",
                enrichment_outcome.fresh_identities.len()
            );

            if let Some(ref mut db) = db {
                match db
                    .set_validator_identities_batch(network, &enrichment_outcome.fresh_identities)
                {
                    Ok(count) => {
                        tracing::info!("Updated {} cached validator identities", count);
                    }
                    Err(e) => {
                        tracing::debug!("Failed to update identity cache: {}", e);
                    }
                }
            }
        }

        identity_map = enrichment_outcome.updated_identity_map;
        bytes_transferred += identity_map.len() as u64 * 100;
        let _ = action_tx
            .send(Action::SetLoadingProgress(
                0.8,
                Some(bytes_transferred),
                None,
            ))
            .await;

        let display_validators = enrichment_outcome.enrichment.validators;
        tracing::info!(
            "Calculated APY for {}/{} validators using era {}",
            enrichment_outcome.enrichment.validators_with_apy,
            display_validators.len(),
            enrichment_outcome.enrichment.apy_era
        );

        cached_validator_apy_map = validator_apy_map(&display_validators);

        tracing::info!(
            "Built validator APY map with {} entries for pool APY calculation",
            cached_validator_apy_map.len()
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
            match db.set_cached_validators_checked(
                network,
                era_info.index,
                &display_validators,
                validators_complete,
            ) {
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

        if pools_cached {
            let _ = action_tx
                .send(Action::SetLoadingProgress(
                    1.0,
                    Some(bytes_transferred),
                    None,
                ))
                .await;
            let _ = action_tx
                .send(Action::UpdateConnectionStatus(ConnectionStatus::Connected))
                .await;
        }
    }

    if !pools_cached {
        // Stabilize connection before pool queries
        if client.is_light_client() {
            tracing::info!("Stabilizing light client before pool queries...");
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

            if !client.is_connected().await {
                tracing::warn!(
                    "Light client disconnected after validator loading, reconnecting..."
                );
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

        let metadata_map = pool_metadata_map(&metadata);
        tracing::info!(
            "Built pool metadata map with {} entries (pool IDs: {:?})",
            metadata_map.len(),
            metadata_map.keys().take(10).collect::<Vec<_>>()
        );

        let initial_display_pools = basic_display_pools(&pools, &metadata_map);

        let _ = action_tx
            .send(Action::SetDisplayPools(initial_display_pools))
            .await;
        tracing::info!(
            "Sent {} pools to UI (fetching APY in background)",
            pools.len()
        );

        // Second pass: fetch nominations and enrich pools.
        let max_pools_to_query = 30.min(pools.len());
        let pool_outcome = match fetch_and_enrich_pools(
            &client,
            &pools,
            &cached_validator_apy_map,
            max_pools_to_query,
            Some(&metadata_map),
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(e) => {
                tracing::error!("Failed to enrich pools: {}, using basic data", e);
                PoolEnrichmentOutcome {
                    pools: basic_display_pools(&pools, &metadata_map),
                    metadata_map: metadata_map.clone(),
                    nominations_map: HashMap::new(),
                }
            }
        };

        let display_pools = pool_outcome.pools;

        bytes_transferred += display_pools.len() as u64 * 150;
        let _ = action_tx
            .send(Action::SetLoadingProgress(
                1.0,
                Some(bytes_transferred),
                None,
            ))
            .await;
        let _ = action_tx
            .send(Action::SetDisplayPools(display_pools.clone()))
            .await;
        let _ = action_tx
            .send(Action::UpdateConnectionStatus(ConnectionStatus::Connected))
            .await;

        if let Some(ref mut db) = db {
            match db.set_cached_pools_at_era(network, era_info.index, &display_pools) {
                Ok(count) => {
                    tracing::info!("Cached {} nomination pools", count);
                }
                Err(e) => {
                    tracing::warn!("Failed to cache pools: {}", e);
                }
            }
        }

        tracing::info!("Nomination pools loaded successfully");
    }

    // Listen for requests from the UI
    while let Some(request) = request_rx.recv().await {
        match request {
            ChainRequest::FetchAccount(account) => {
                tracing::info!("Fetching account status for {}", account);
                let cached_account_status = match db.as_ref().map(|db| {
                    AccountStatusService::load_cached(
                        db,
                        network,
                        &account.to_string(),
                        CachePolicy::default(),
                    )
                }) {
                    Some(Ok(status)) => status,
                    Some(Err(e)) => {
                        tracing::debug!("Failed to load cached account status: {}", e);
                        None
                    }
                    None => None,
                };
                if let Some(cached) = cached_account_status {
                    tracing::info!("Using recent cached account status for {}", account);
                    let cached_status = account_status_from_cache(account.clone(), &cached);
                    let _ = action_tx
                        .send(Action::SetAccountStatus(Box::new(cached_status)))
                        .await;
                }

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
                    address: account.clone(),
                    balance,
                    staking_ledger,
                    nominations,
                    pool_membership,
                };

                if let Some(ref db) = db
                    && let Err(e) = db.set_cached_account_status(
                        network,
                        &account.to_string(),
                        &cached_account_status_from_live(&status),
                    )
                {
                    tracing::warn!("Failed to cache account status: {}", e);
                }

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
                let db_path = prepare_db_path();
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

                let user_bonded = match client.get_staking_ledger(&account).await {
                    Ok(Some(ledger)) => ledger.active,
                    _ => 0,
                };

                let (start_era, end_era) = HistoryService::era_window(current_era, num_eras);

                if let Some(ref db) = db
                    && let Ok(cached) = HistoryService::load_cached_range(
                        db,
                        network,
                        &address,
                        start_era,
                        end_era,
                        current_era,
                        current_era_start_ms,
                        era_duration_ms,
                        CachePolicy::default(),
                    )
                    && !cached.is_empty()
                {
                    tracing::info!(
                        "Loaded {} cached history points for eras {}..={} (filtered)",
                        cached.len(),
                        start_era,
                        end_era
                    );
                    for point in cached {
                        let _ = action_tx.send(Action::AddStakingHistoryPoint(point)).await;
                    }
                }

                let eras_to_fetch: Vec<u32> = if let Some(ref db) = db {
                    HistoryService::missing_eras(
                        db,
                        network,
                        &address,
                        start_era,
                        end_era,
                        CachePolicy::default(),
                    )
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

                // Honour cancellation before starting the parallel fetch.
                if *cancel_rx.borrow() {
                    tracing::info!("History loading cancelled");
                    let _ = action_tx.send(Action::HistoryLoadingComplete).await;
                    continue;
                }

                let client = &client;
                let concurrency = 8;
                let fetched = stream::iter(eras_to_fetch)
                    .map(|era| {
                        let cancel_rx = cancel_rx.clone();
                        async move {
                            if *cancel_rx.borrow() {
                                return (era, None);
                            }
                            let point = fetch_history_era(
                                era,
                                current_era,
                                current_era_start_ms,
                                era_duration_ms,
                                user_bonded,
                                |era| client.get_era_validator_reward(era),
                                |era| client.get_era_total_stake_direct(era),
                            )
                            .await;
                            (era, point)
                        }
                    })
                    .buffer_unordered(concurrency)
                    .collect::<Vec<_>>()
                    .await;

                if *cancel_rx.borrow() {
                    tracing::info!("History loading cancelled");
                    let _ = action_tx.send(Action::HistoryLoadingComplete).await;
                    continue;
                }

                let mut fetched = fetched;
                fetched.sort_by_key(|(era, _)| *era);

                let mut new_points = Vec::new();
                for (era, point) in fetched {
                    if let Some(point) = point {
                        new_points.push(point.clone());
                        let _ = action_tx.send(Action::AddStakingHistoryPoint(point)).await;
                        tracing::debug!(
                            "Added history point for era {} (APY: {:.2}%)",
                            era,
                            new_points.last().map(|point| point.apy).unwrap_or(0.0) * 100.0
                        );
                    }
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

                let _ = action_tx.send(Action::HistoryLoadingComplete).await;
                tracing::info!("Staking history loaded");
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
                    StakingOp::WithdrawUnbonded { signer } => {
                        let spans =
                            slashing_spans_from_result(client.get_slashing_spans(signer).await);
                        (
                            signer,
                            client
                                .create_withdraw_unbonded_payload(signer, spans, use_mortal_era)
                                .await,
                        )
                    }
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
                    StakingOp::PoolWithdraw { signer } => {
                        let spans =
                            slashing_spans_from_result(client.get_slashing_spans(signer).await);
                        (
                            signer,
                            client
                                .create_pool_withdraw_payload(signer, signer, spans, use_mortal_era)
                                .await,
                        )
                    }
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

/// Path to the legacy TUI-specific history database.
pub fn old_tui_db_path() -> PathBuf {
    directories::ProjectDirs::from("io", "stkopt", "stkopt")
        .map(|dirs| dirs.data_dir().join("history.db"))
        .unwrap_or_else(|| PathBuf::from("stkopt_history.db"))
}

/// Copy the legacy TUI history database to `new_path` if it exists and
/// `new_path` does not yet exist.
pub fn maybe_migrate_old_history_db(
    old_path: &Path,
    new_path: &Path,
) -> Result<(), stkopt_core::config::ConfigError> {
    if old_path.exists() && !new_path.exists() {
        if let Some(parent) = new_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(old_path, new_path)?;
    }
    Ok(())
}

/// Migrate the legacy TUI history database to the unified path.
pub fn migrate_legacy_history_db(new_path: &Path) -> Result<(), stkopt_core::config::ConfigError> {
    maybe_migrate_old_history_db(&old_tui_db_path(), new_path)
}

/// Migrate any legacy database and ensure the parent directory exists.
fn prepare_db_path() -> PathBuf {
    let path =
        stkopt_core::config::get_db_path().unwrap_or_else(|_| PathBuf::from("stkopt_history.db"));
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = migrate_legacy_history_db(&path);
    path
}

/// Run in update mode: fetch missing history and store to database, then exit.
pub async fn run_update_mode(
    network: Network,
    config: ConnectionConfig,
    address: Option<String>,
    num_eras: u32,
) -> Result<()> {
    let address = match address {
        Some(addr) => addr,
        None => {
            tracing::error!("--address is required with --update");
            std::process::exit(1);
        }
    };

    let account: AccountId32 = address
        .parse()
        .map_err(|_| color_eyre::eyre::eyre!("Invalid address format: {}", address))?;

    tracing::info!("Updating staking history for {} on {}", address, network);

    let db_path = prepare_db_path();
    let mut db = db::HistoryDb::open(&db_path).map_err(|e| {
        color_eyre::eyre::eyre!("Failed to open database at {}: {}", db_path.display(), e)
    })?;

    tracing::info!("Database: {}", db_path.display());

    let (status_tx, _status_rx) = mpsc::channel::<ConnectionStatus>(1);

    tracing::info!("Connecting to {} Asset Hub...", network);
    let client = ChainClient::connect(network, &config, status_tx).await?;
    tracing::info!("Connected via {}", client.connection_mode());

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

    tracing::info!(
        "Current era: {} ({}ms per era)",
        current_era,
        era_duration_ms
    );

    let user_bonded = match client.get_staking_ledger(&account).await {
        Ok(Some(ledger)) => ledger.active,
        _ => 0,
    };

    let (start_era, end_era) = HistoryService::era_window(current_era, num_eras);

    let missing_eras = HistoryService::missing_eras(
        &db,
        network,
        &address,
        start_era,
        end_era,
        CachePolicy::default(),
    )
    .map_err(|e| color_eyre::eyre::eyre!("Database error: {}", e))?;

    if missing_eras.is_empty() {
        tracing::info!("All {} eras are already cached", num_eras);
        return Ok(());
    }

    tracing::info!(
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
                tracing::warn!("  Era {}: no reward data", era);
                continue;
            }
            Err(e) => {
                tracing::warn!("  Era {}: error getting reward: {}", era, e);
                continue;
            }
        };

        let total_staked = match client.get_era_total_stake_direct(era).await {
            Ok(staked) if staked > 0 => staked,
            Ok(_) => {
                tracing::warn!("  Era {}: no stake data", era);
                continue;
            }
            Err(e) => {
                tracing::warn!("  Era {}: error getting stake: {}", era, e);
                continue;
            }
        };

        let point = staking_history_point(
            era,
            current_era,
            current_era_start_ms,
            era_duration_ms,
            era_reward,
            user_bonded,
            total_staked,
        );

        if !HistoryService::is_valid_cached_apy(point.apy, CachePolicy::default()) {
            tracing::warn!(
                "  Era {}: unrealistic APY {:.2}%, skipping",
                era,
                point.apy * 100.0
            );
            continue;
        }

        points.push(point);
        fetched += 1;

        if fetched % 10 == 0 {
            tracing::info!("  Fetched {} eras...", fetched);
        }
    }

    if !points.is_empty() {
        db.insert_history_batch(network, &address, &points)
            .map_err(|e| color_eyre::eyre::eyre!("Failed to store history: {}", e))?;
        tracing::info!("Stored {} era records to database", points.len());
    }

    let total = db
        .count_history(network, &address)
        .map_err(|e| color_eyre::eyre::eyre!("Database error: {}", e))?;

    tracing::info!("Total records for {}: {}", address, total);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(byte: u8) -> AccountId32 {
        AccountId32::from([byte; 32])
    }

    fn display_validator(total_stake: u128, apy: Option<f64>) -> DisplayValidator {
        DisplayValidator {
            address: account(9).to_string(),
            name: None,
            commission: 0.05,
            blocked: false,
            total_stake,
            own_stake: total_stake / 10,
            nominator_count: 10,
            points: 100,
            apy,
        }
    }

    #[test]
    fn test_cached_validators_have_chain_data_requires_stake_and_apy() {
        assert!(cached_validators_have_chain_data(&[display_validator(
            1_000,
            Some(0.12)
        )]));
        assert!(!cached_validators_have_chain_data(&[display_validator(
            0,
            Some(0.12)
        )]));
        assert!(!cached_validators_have_chain_data(&[display_validator(
            1_000, None
        )]));
        assert!(!cached_validators_have_chain_data(&[]));
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

    // --- database path tests ---

    #[test]
    fn test_prepare_db_path_matches_core_helper() {
        let path = prepare_db_path();
        let core_path = stkopt_core::config::get_db_path()
            .unwrap_or_else(|_| PathBuf::from("stkopt_history.db"));
        assert_eq!(path, core_path);
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("history.db")
        );
    }

    #[test]
    fn test_maybe_migrate_old_history_db_copies_file() {
        let temp = std::env::temp_dir().join("stkopt_tui_migration_test");
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).unwrap();

        let old_path = temp.join("old_history.db");
        let new_path = temp.join("new_history.db");
        std::fs::write(&old_path, "legacy data").unwrap();

        maybe_migrate_old_history_db(&old_path, &new_path).unwrap();

        assert!(new_path.exists());
        assert_eq!(std::fs::read_to_string(&new_path).unwrap(), "legacy data");

        // A second call must be a no-op.
        std::fs::write(&old_path, "changed data").unwrap();
        maybe_migrate_old_history_db(&old_path, &new_path).unwrap();
        assert_eq!(std::fs::read_to_string(&new_path).unwrap(), "legacy data");

        let _ = std::fs::remove_dir_all(&temp);
    }

    // --- chain metadata tests ---

    #[test]
    fn test_paseo_cached_chain_metadata_uses_correct_ss58_prefix() {
        let network = Network::Paseo;
        let metadata = CachedChainMetadata {
            genesis_hash: "00".to_string(),
            spec_version: 1,
            tx_version: 1,
            ss58_prefix: network.ss58_format(),
            token_symbol: network.token_symbol().to_string(),
            token_decimals: network.token_decimals(),
            era_duration_ms: 24 * 60 * 60 * 1000,
            current_era: 1,
        };
        assert_eq!(metadata.ss58_prefix, 0);
        assert_eq!(metadata.token_symbol, "PAS");
        assert_eq!(metadata.token_decimals, 10);
    }

    // --- slashing spans tests ---

    #[test]
    fn test_slashing_spans_from_result_passes_ok_value() {
        assert_eq!(slashing_spans_from_result(Ok(7)), 7);
    }

    #[test]
    fn test_slashing_spans_from_result_defaults_to_zero_on_error() {
        assert_eq!(
            slashing_spans_from_result(Err(stkopt_chain::ChainError::Storage("fail".to_string()))),
            0
        );
    }

    // --- account status cache round-trip tests ---

    #[test]
    fn test_account_status_from_cache_populates_unlocking_and_pool_unbonding() {
        let address = account(1);
        let status = CachedAccountStatus {
            free_balance: 1_000_000,
            reserved_balance: 100_000,
            frozen_balance: 50_000,
            staked_amount: 500_000,
            nominations_json: None,
            pool_id: Some(7),
            pool_points: Some(1_234_567),
            unlocking_json: Some(
                r#"[{"value":10000,"era":1501},{"value":20000,"era":1502}]"#.to_string(),
            ),
            pool_unbonding_eras_json: Some(
                serde_json::to_string(&vec![(1601u32, 5_000u128), (1602u32, 8_000u128)]).unwrap(),
            ),
            pool_last_recorded_reward_counter: 99_999,
        };

        let live = account_status_from_cache(address.clone(), &status);

        assert_eq!(live.balance.free, status.free_balance);
        let ledger = live.staking_ledger.unwrap();
        assert_eq!(ledger.unlocking.len(), 2);
        assert_eq!(ledger.unlocking[0].value, 10_000);
        assert_eq!(ledger.unlocking[0].era, 1501);
        assert_eq!(ledger.unlocking[1].value, 20_000);
        assert_eq!(ledger.unlocking[1].era, 1502);

        let pool = live.pool_membership.unwrap();
        assert_eq!(pool.pool_id, 7);
        assert_eq!(pool.points, 1_234_567);
        assert_eq!(pool.unbonding_eras, vec![(1601, 5_000), (1602, 8_000)]);
        assert_eq!(pool.last_recorded_reward_counter, 99_999);
        assert_eq!(live.address, address);
    }

    #[test]
    fn test_cached_account_status_from_live_round_trips_unlocking_and_pool_unbonding() {
        let address = account(2);
        let live = AccountStatus {
            address: address.clone(),
            balance: AccountBalance {
                free: 1_000_000,
                reserved: 100_000,
                frozen: 50_000,
            },
            staking_ledger: Some(StakingLedger {
                stash: address.clone(),
                total: 500_000,
                active: 400_000,
                unlocking: vec![
                    UnlockChunk {
                        value: 10_000,
                        era: 1501,
                    },
                    UnlockChunk {
                        value: 20_000,
                        era: 1502,
                    },
                ],
            }),
            nominations: None,
            pool_membership: Some(PoolMembership {
                pool_id: 7,
                points: 1_234_567,
                unbonding_eras: vec![(1601, 5_000), (1602, 8_000)],
                last_recorded_reward_counter: 99_999,
            }),
        };

        let cached = cached_account_status_from_live(&live);
        assert_eq!(
            cached.unlocking_json,
            Some(r#"[{"value":10000,"era":1501},{"value":20000,"era":1502}]"#.to_string())
        );
        assert_eq!(
            cached.pool_unbonding_eras_json,
            Some(serde_json::to_string(&vec![(1601u32, 5_000u128), (1602u32, 8_000u128)]).unwrap())
        );
        assert_eq!(cached.pool_last_recorded_reward_counter, 99_999);

        let restored = account_status_from_cache(address.clone(), &cached);
        assert_eq!(restored.balance.free, live.balance.free);
        let restored_ledger = restored.staking_ledger.unwrap();
        let original_ledger = live.staking_ledger.unwrap();
        assert_eq!(restored_ledger.active, original_ledger.active);
        assert_eq!(restored_ledger.unlocking, original_ledger.unlocking);

        let restored_pool = restored.pool_membership.unwrap();
        let original_pool = live.pool_membership.unwrap();
        assert_eq!(restored_pool.pool_id, original_pool.pool_id);
        assert_eq!(restored_pool.points, original_pool.points);
        assert_eq!(restored_pool.unbonding_eras, original_pool.unbonding_eras);
        assert_eq!(
            restored_pool.last_recorded_reward_counter,
            original_pool.last_recorded_reward_counter
        );
    }

    // --- fetch_history_era tests ---

    fn run_async<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Runtime::new().unwrap().block_on(f)
    }

    #[test]
    fn test_fetch_history_era_returns_none_when_reward_missing() {
        let point = run_async(fetch_history_era(
            5,
            10,
            1_700_000_000_000,
            24 * 60 * 60 * 1000,
            1_000_000,
            |_era| async { Ok::<_, stkopt_chain::ChainError>(None) },
            |_era| async { Ok::<_, stkopt_chain::ChainError>(1_000_000_000) },
        ));
        assert!(point.is_none());
    }

    #[test]
    fn test_fetch_history_era_returns_none_when_stake_zero() {
        let point = run_async(fetch_history_era(
            5,
            10,
            1_700_000_000_000,
            24 * 60 * 60 * 1000,
            1_000_000,
            |_era| async { Ok::<_, stkopt_chain::ChainError>(Some(10_000)) },
            |_era| async { Ok::<_, stkopt_chain::ChainError>(0) },
        ));
        assert!(point.is_none());
    }

    #[test]
    fn test_fetch_history_era_returns_point_for_valid_data() {
        let point = run_async(fetch_history_era(
            5,
            10,
            1_700_000_000_000,
            24 * 60 * 60 * 1000,
            1_000_000,
            |_era| async { Ok::<_, stkopt_chain::ChainError>(Some(10_000)) },
            |_era| async { Ok::<_, stkopt_chain::ChainError>(1_000_000_000) },
        ));
        let point = point.expect("expected a history point");
        assert_eq!(point.era, 5);
        assert_eq!(point.bonded, 1_000_000);
        assert!(point.apy.is_finite());
        assert!(point.apy > 0.0 && point.apy <= 0.5);
    }

    #[test]
    fn test_fetch_history_era_skips_unrealistic_apy() {
        let point = run_async(fetch_history_era(
            5,
            10,
            1_700_000_000_000,
            24 * 60 * 60 * 1000,
            1_000,
            |_era| async { Ok::<_, stkopt_chain::ChainError>(Some(1_000)) },
            |_era| async { Ok::<_, stkopt_chain::ChainError>(1) },
        ));
        assert!(point.is_none());
    }
}
