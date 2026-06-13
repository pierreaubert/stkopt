//! Real chain integration using stkopt-chain.
//!
//! This module provides the actual blockchain connection and data fetching
//! for the GPUI app, using the stkopt-chain crate.

use std::collections::HashMap;
use stkopt_chain::{
    ChainClient, ConnectionConfig, ConnectionMode as ChainConnectionMode, PeopleChainClient,
    RewardDestination, RpcEndpoints, UnsignedPayload, basic_display_validators, encode_for_qr,
    eras_for_lookback_days, fetch_and_enrich_pools, fetch_and_enrich_validators,
    staking_history_point, validator_apy_map,
};
use stkopt_core::{CachePolicy, ConnectionStatus, HistoryService, Network};
use subxt::utils::AccountId32;
use tokio::sync::{mpsc, oneshot};

use futures::stream::{self, StreamExt};
use std::future::Future;

use crate::app::{HistoryPoint, PoolInfo, ValidatorInfo};
use crate::db_service::DbService;
use stkopt_core::db::{CachedAccountStatus, CachedChainMetadata};

const PEOPLE_READY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

async fn connect_ready_people_client(
    client: &ChainClient,
    network: Network,
) -> Option<PeopleChainClient> {
    match client.connect_people_chain_client().await {
        Ok(people) => match people.wait_until_ready(PEOPLE_READY_TIMEOUT).await {
            Ok(block) => {
                tracing::info!("Connected to {} People chain at block {}", network, block);
                Some(people)
            }
            Err(e) => {
                tracing::warn!(
                    "People chain connected but did not become ready (identity data will be unavailable): {}",
                    e
                );
                None
            }
        },
        Err(e) => {
            tracing::warn!(
                "Failed to connect to People chain (identity data will be unavailable): {}",
                e
            );
            None
        }
    }
}

fn cached_validators_have_chain_data(validators: &[ValidatorInfo]) -> bool {
    validators.iter().any(|validator| validator.total_stake > 0)
        && validators.iter().any(|validator| validator.apy.is_some())
}

/// Commands that can be sent to the chain worker.
#[derive(Debug)]
pub enum ChainCommand {
    /// Connect to a network.
    Connect {
        network: Network,
        use_light_client: bool,
    },
    /// Disconnect from the network.
    Disconnect,
    /// Fetch account data for an address.
    FetchAccount {
        address: String,
        reply: oneshot::Sender<Result<AccountData, String>>,
    },
    /// Fetch validators list.
    FetchValidators {
        reply: oneshot::Sender<Result<Vec<ValidatorInfo>, String>>,
    },
    /// Fetch pools list.
    FetchPools {
        reply: oneshot::Sender<Result<Vec<PoolInfo>, String>>,
    },
    /// Fetch staking history for an account.
    FetchHistory {
        address: String,
        lookback_days: u32,
        reply: oneshot::Sender<Result<Vec<HistoryPoint>, String>>,
    },
    // === Transaction Payload Generation ===
    /// Create bond transaction payload.
    CreateBondPayload {
        signer: AccountId32,
        value: u128,
        reply: oneshot::Sender<Result<TransactionPayload, String>>,
    },
    /// Create unbond transaction payload.
    CreateUnbondPayload {
        signer: AccountId32,
        value: u128,
        reply: oneshot::Sender<Result<TransactionPayload, String>>,
    },
    /// Create bond_extra transaction payload.
    CreateBondExtraPayload {
        signer: AccountId32,
        value: u128,
        reply: oneshot::Sender<Result<TransactionPayload, String>>,
    },
    /// Create rebond transaction payload.
    CreateRebondPayload {
        signer: AccountId32,
        value: u128,
        reply: oneshot::Sender<Result<TransactionPayload, String>>,
    },
    /// Create set_payee transaction payload.
    CreateSetPayeePayload {
        signer: AccountId32,
        destination: RewardDestination,
        reply: oneshot::Sender<Result<TransactionPayload, String>>,
    },
    /// Create withdraw_unbonded transaction payload.
    CreateWithdrawUnbondedPayload {
        signer: AccountId32,
        reply: oneshot::Sender<Result<TransactionPayload, String>>,
    },
    /// Create chill transaction payload.
    CreateChillPayload {
        signer: AccountId32,
        reply: oneshot::Sender<Result<TransactionPayload, String>>,
    },
    /// Create nominate transaction payload.
    CreateNominatePayload {
        signer: AccountId32,
        targets: Vec<AccountId32>,
        reply: oneshot::Sender<Result<TransactionPayload, String>>,
    },
    // === Pool Operations ===
    /// Create pool join transaction payload.
    CreatePoolJoinPayload {
        signer: AccountId32,
        pool_id: u32,
        amount: u128,
        reply: oneshot::Sender<Result<TransactionPayload, String>>,
    },
    /// Create pool bond_extra transaction payload.
    CreatePoolBondExtraPayload {
        signer: AccountId32,
        amount: u128,
        reply: oneshot::Sender<Result<TransactionPayload, String>>,
    },
    /// Create pool claim_payout transaction payload.
    CreatePoolClaimPayload {
        signer: AccountId32,
        reply: oneshot::Sender<Result<TransactionPayload, String>>,
    },
    /// Create pool unbond transaction payload.
    CreatePoolUnbondPayload {
        signer: AccountId32,
        amount: u128,
        reply: oneshot::Sender<Result<TransactionPayload, String>>,
    },
    /// Create pool withdraw_unbonded transaction payload.
    CreatePoolWithdrawPayload {
        signer: AccountId32,
        reply: oneshot::Sender<Result<TransactionPayload, String>>,
    },
    // === Transaction Submission ===
    /// Submit a signed extrinsic to the network.
    SubmitSignedExtrinsic {
        extrinsic: Vec<u8>,
        reply: oneshot::Sender<Result<TxSubmissionResult, String>>,
    },
}

/// Account data fetched from chain.
#[derive(Debug, Clone)]
pub struct AccountData {
    pub free_balance: u128,
    pub reserved_balance: u128,
    /// Balance locked for staking/governance that reduces the transferable amount.
    pub frozen_balance: u128,
    pub staked_balance: Option<u128>,
    pub unbonding_balance: u128,
    pub is_nominating: bool,
    pub nominations: Vec<String>,
    pub pool_id: Option<u32>,
    /// Pending rewards from nomination pool (if member of a pool).
    pub pool_pending_rewards: u128,
    /// Pool member unbonding eras and balances (if member of a pool).
    pub pool_unbonding_eras: Vec<(u32, u128)>,
    /// Last recorded reward counter for pool membership.
    pub pool_last_recorded_reward_counter: u128,
}

fn account_data_from_cache(status: &CachedAccountStatus) -> AccountData {
    let nominations = status
        .nominations_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<Vec<String>>(json).ok())
        .unwrap_or_default();

    let pool_unbonding_eras = status
        .pool_unbonding_eras_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<Vec<(u32, u128)>>(json).ok())
        .unwrap_or_default();

    AccountData {
        free_balance: status.free_balance,
        reserved_balance: status.reserved_balance,
        frozen_balance: status.frozen_balance,
        staked_balance: (status.staked_amount > 0).then_some(status.staked_amount),
        unbonding_balance: 0,
        is_nominating: !nominations.is_empty(),
        nominations,
        pool_id: status.pool_id,
        pool_pending_rewards: 0,
        pool_unbonding_eras,
        pool_last_recorded_reward_counter: status.pool_last_recorded_reward_counter,
    }
}

/// Transaction payload ready for QR encoding.
#[derive(Debug, Clone)]
pub struct TransactionPayload {
    /// Raw QR code data (binary, for Polkadot Vault).
    pub qr_data: Vec<u8>,
    /// The unsigned payload (for building signed extrinsic later).
    pub unsigned_payload: UnsignedPayload,
    /// The signer account.
    pub signer: AccountId32,
    /// Human-readable description.
    pub description: String,
}

/// Enrich validators with stake, points, identity, and APY data.
///
/// This function fetches additional data from the chain to populate all validator fields:
/// - Stake data from ErasStakersOverview
/// - Points from ErasRewardPoints
/// - Identity names from People chain
/// - APY calculated from era rewards
async fn enrich_validators(
    client: &ChainClient,
    validators: &[stkopt_chain::ValidatorInfo],
    people_client: Option<&PeopleChainClient>,
    db: Option<&DbService>,
    network: Network,
) -> Vec<ValidatorInfo> {
    // Get active era for staking queries
    let era = match client.get_active_era().await {
        Ok(Some(era)) => era,
        Ok(None) => {
            tracing::warn!("No active era found, returning basic validator data");
            return basic_display_validators(validators);
        }
        Err(e) => {
            tracing::warn!(
                "Failed to get active era: {}, returning basic validator data",
                e
            );
            return basic_display_validators(validators);
        }
    };
    let era_duration_ms = era.duration_ms;

    // Use the previous completed era for APY calculations.
    // The current era doesn't have rewards yet (only paid after era ends),
    // and points are still accumulating. The previous era has complete data.
    let query_era = era.index.saturating_sub(1);
    tracing::info!(
        "Enriching validators using era {} (current era: {})",
        query_era,
        era.index
    );

    let identity_map: HashMap<String, String> = if let Some(db) = db {
        match db
            .get_validator_identities_within_age(
                network,
                stkopt_core::DEFAULT_IDENTITY_MAX_AGE_SECS,
            )
            .await
        {
            Ok(cached) => {
                if !cached.is_empty() {
                    tracing::info!("Loaded {} cached validator identities", cached.len());
                }
                cached
            }
            Err(e) => {
                tracing::debug!("Failed to load cached validator identities: {}", e);
                HashMap::new()
            }
        }
    } else {
        HashMap::new()
    };

    let outcome = match fetch_and_enrich_validators(
        client,
        validators,
        people_client,
        identity_map,
        query_era,
        era_duration_ms,
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(e) => {
            tracing::error!("Failed to enrich validators: {}, returning basic data", e);
            return basic_display_validators(validators);
        }
    };

    if !outcome.fresh_identities.is_empty() {
        let fresh_count = outcome.fresh_identities.len();
        tracing::info!(
            "Fetched {} new validator identities from People chain",
            fresh_count
        );
        if let Some(db) = db
            && let Err(e) = db
                .set_validator_identities_batch(network, outcome.fresh_identities)
                .await
        {
            tracing::debug!("Failed to update validator identity cache: {}", e);
        }
    }

    tracing::info!(
        "Enriched {} validators with stake/identity data; calculated APY for {} using era {}",
        outcome.enrichment.validators.len(),
        outcome.enrichment.validators_with_apy,
        outcome.enrichment.apy_era
    );
    outcome.enrichment.validators
}

/// Convert an unsigned payload to a transaction payload ready for QR display.
fn make_transaction_payload(
    payload: UnsignedPayload,
    signer: AccountId32,
) -> Result<TransactionPayload, String> {
    let qr_data = encode_for_qr(&payload, &signer)
        .map_err(|e| format!("Failed to encode transaction QR: {}", e))?;
    let description = payload.description.clone();
    Ok(TransactionPayload {
        qr_data,
        unsigned_payload: payload,
        signer,
        description,
    })
}

/// Returns the number of slashing spans to provide for a withdraw payload.
///
/// On chain error we fall back to 0; the extrinsic will fail if the value is
/// too low, but a fallback lets us build the payload for QR signing.
fn slashing_spans_for_withdraw(spans: Result<u32, stkopt_chain::ChainError>) -> u32 {
    spans.unwrap_or(0)
}

/// Transaction submission result.
#[derive(Debug, Clone)]
pub enum TxSubmissionResult {
    /// Transaction was included in a block.
    InBlock { block_hash: [u8; 32] },
    /// Transaction was finalized.
    Finalized { block_hash: [u8; 32] },
    /// Transaction was dropped.
    Dropped(String),
}

/// Updates sent from chain worker to UI.
#[derive(Debug, Clone)]
pub enum ChainUpdate {
    /// Connection status changed.
    ConnectionStatus(ConnectionStatus),
    /// Loading progress changed for one connection step.
    LoadingProgress { step: LoadingStep, progress: f32 },
    /// Validators loaded.
    ValidatorsLoaded(Vec<ValidatorInfo>),
    /// Pools loaded.
    PoolsLoaded(Vec<PoolInfo>),
    /// Account data loaded.
    AccountLoaded(AccountData),
    /// History loaded.
    HistoryLoaded(Vec<HistoryPoint>),
    /// QR payload generated for signing.
    QrPayloadGenerated(TransactionPayload),
    /// Transaction submission status update.
    TxSubmissionUpdate(TxSubmissionResult),
    /// Error occurred.
    Error(String),
}

/// Initial connection loading step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadingStep {
    Operations,
    Validators,
    Pools,
    History,
}

/// Handle to communicate with the chain worker.
#[derive(Clone)]
pub struct ChainHandle {
    command_tx: mpsc::Sender<ChainCommand>,
}

impl ChainHandle {
    /// Request connection to a network.
    pub async fn connect(&self, network: Network, use_light_client: bool) -> Result<(), String> {
        self.command_tx
            .send(ChainCommand::Connect {
                network,
                use_light_client,
            })
            .await
            .map_err(|e| format!("Failed to send connect command: {}", e))
    }

    /// Request disconnection.
    pub async fn disconnect(&self) -> Result<(), String> {
        self.command_tx
            .send(ChainCommand::Disconnect)
            .await
            .map_err(|e| format!("Failed to send disconnect command: {}", e))
    }

    /// Fetch account data.
    pub async fn fetch_account(&self, address: String) -> Result<AccountData, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(ChainCommand::FetchAccount {
                address,
                reply: reply_tx,
            })
            .await
            .map_err(|e| format!("Failed to send fetch account command: {}", e))?;
        reply_rx.await.map_err(|_| "Channel closed".to_string())?
    }

    /// Fetch validators.
    pub async fn fetch_validators(&self) -> Result<Vec<ValidatorInfo>, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(ChainCommand::FetchValidators { reply: reply_tx })
            .await
            .map_err(|e| format!("Failed to send fetch validators command: {}", e))?;
        reply_rx.await.map_err(|_| "Channel closed".to_string())?
    }

    /// Fetch pools.
    pub async fn fetch_pools(&self) -> Result<Vec<PoolInfo>, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(ChainCommand::FetchPools { reply: reply_tx })
            .await
            .map_err(|e| format!("Failed to send fetch pools command: {}", e))?;
        reply_rx.await.map_err(|_| "Channel closed".to_string())?
    }

    /// Fetch staking history for an account.
    ///
    /// `lookback_days` is converted to eras using the chain's current era duration
    /// before querying on-chain or cached data.
    pub async fn fetch_history(
        &self,
        address: String,
        lookback_days: u32,
    ) -> Result<Vec<HistoryPoint>, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(ChainCommand::FetchHistory {
                address,
                lookback_days,
                reply: reply_tx,
            })
            .await
            .map_err(|e| format!("Failed to send fetch history command: {}", e))?;
        reply_rx.await.map_err(|_| "Channel closed".to_string())?
    }

    // === Transaction Payload Generation ===

    /// Create a bond transaction payload.
    pub async fn create_bond_payload(
        &self,
        signer: AccountId32,
        value: u128,
    ) -> Result<TransactionPayload, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(ChainCommand::CreateBondPayload {
                signer,
                value,
                reply: reply_tx,
            })
            .await
            .map_err(|e| format!("Failed to send command: {}", e))?;
        reply_rx.await.map_err(|_| "Channel closed".to_string())?
    }

    /// Create an unbond transaction payload.
    pub async fn create_unbond_payload(
        &self,
        signer: AccountId32,
        value: u128,
    ) -> Result<TransactionPayload, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(ChainCommand::CreateUnbondPayload {
                signer,
                value,
                reply: reply_tx,
            })
            .await
            .map_err(|e| format!("Failed to send command: {}", e))?;
        reply_rx.await.map_err(|_| "Channel closed".to_string())?
    }

    /// Create a bond_extra transaction payload.
    pub async fn create_bond_extra_payload(
        &self,
        signer: AccountId32,
        value: u128,
    ) -> Result<TransactionPayload, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(ChainCommand::CreateBondExtraPayload {
                signer,
                value,
                reply: reply_tx,
            })
            .await
            .map_err(|e| format!("Failed to send command: {}", e))?;
        reply_rx.await.map_err(|_| "Channel closed".to_string())?
    }

    /// Create a rebond transaction payload.
    pub async fn create_rebond_payload(
        &self,
        signer: AccountId32,
        value: u128,
    ) -> Result<TransactionPayload, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(ChainCommand::CreateRebondPayload {
                signer,
                value,
                reply: reply_tx,
            })
            .await
            .map_err(|e| format!("Failed to send command: {}", e))?;
        reply_rx.await.map_err(|_| "Channel closed".to_string())?
    }

    /// Create a set_payee transaction payload.
    pub async fn create_set_payee_payload(
        &self,
        signer: AccountId32,
        destination: RewardDestination,
    ) -> Result<TransactionPayload, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(ChainCommand::CreateSetPayeePayload {
                signer,
                destination,
                reply: reply_tx,
            })
            .await
            .map_err(|e| format!("Failed to send command: {}", e))?;
        reply_rx.await.map_err(|_| "Channel closed".to_string())?
    }

    /// Create a withdraw_unbonded transaction payload.
    pub async fn create_withdraw_unbonded_payload(
        &self,
        signer: AccountId32,
    ) -> Result<TransactionPayload, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(ChainCommand::CreateWithdrawUnbondedPayload {
                signer,
                reply: reply_tx,
            })
            .await
            .map_err(|e| format!("Failed to send command: {}", e))?;
        reply_rx.await.map_err(|_| "Channel closed".to_string())?
    }

    /// Create a chill transaction payload.
    pub async fn create_chill_payload(
        &self,
        signer: AccountId32,
    ) -> Result<TransactionPayload, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(ChainCommand::CreateChillPayload {
                signer,
                reply: reply_tx,
            })
            .await
            .map_err(|e| format!("Failed to send command: {}", e))?;
        reply_rx.await.map_err(|_| "Channel closed".to_string())?
    }

    /// Create a nominate transaction payload.
    pub async fn create_nominate_payload(
        &self,
        signer: AccountId32,
        targets: Vec<AccountId32>,
    ) -> Result<TransactionPayload, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(ChainCommand::CreateNominatePayload {
                signer,
                targets,
                reply: reply_tx,
            })
            .await
            .map_err(|e| format!("Failed to send command: {}", e))?;
        reply_rx.await.map_err(|_| "Channel closed".to_string())?
    }

    // === Pool Operations ===

    /// Create a pool join transaction payload.
    pub async fn create_pool_join_payload(
        &self,
        signer: AccountId32,
        pool_id: u32,
        amount: u128,
    ) -> Result<TransactionPayload, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(ChainCommand::CreatePoolJoinPayload {
                signer,
                pool_id,
                amount,
                reply: reply_tx,
            })
            .await
            .map_err(|e| format!("Failed to send command: {}", e))?;
        reply_rx.await.map_err(|_| "Channel closed".to_string())?
    }

    /// Create a pool bond_extra transaction payload.
    pub async fn create_pool_bond_extra_payload(
        &self,
        signer: AccountId32,
        amount: u128,
    ) -> Result<TransactionPayload, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(ChainCommand::CreatePoolBondExtraPayload {
                signer,
                amount,
                reply: reply_tx,
            })
            .await
            .map_err(|e| format!("Failed to send command: {}", e))?;
        reply_rx.await.map_err(|_| "Channel closed".to_string())?
    }

    /// Create a pool claim_payout transaction payload.
    pub async fn create_pool_claim_payload(
        &self,
        signer: AccountId32,
    ) -> Result<TransactionPayload, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(ChainCommand::CreatePoolClaimPayload {
                signer,
                reply: reply_tx,
            })
            .await
            .map_err(|e| format!("Failed to send command: {}", e))?;
        reply_rx.await.map_err(|_| "Channel closed".to_string())?
    }

    /// Create a pool unbond transaction payload.
    pub async fn create_pool_unbond_payload(
        &self,
        signer: AccountId32,
        amount: u128,
    ) -> Result<TransactionPayload, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(ChainCommand::CreatePoolUnbondPayload {
                signer,
                amount,
                reply: reply_tx,
            })
            .await
            .map_err(|e| format!("Failed to send command: {}", e))?;
        reply_rx.await.map_err(|_| "Channel closed".to_string())?
    }

    /// Create a pool withdraw_unbonded transaction payload.
    pub async fn create_pool_withdraw_payload(
        &self,
        signer: AccountId32,
    ) -> Result<TransactionPayload, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(ChainCommand::CreatePoolWithdrawPayload {
                signer,
                reply: reply_tx,
            })
            .await
            .map_err(|e| format!("Failed to send command: {}", e))?;
        reply_rx.await.map_err(|_| "Channel closed".to_string())?
    }

    // === Transaction Submission ===

    /// Submit a signed extrinsic to the network.
    pub async fn submit_signed_extrinsic(
        &self,
        extrinsic: Vec<u8>,
    ) -> Result<TxSubmissionResult, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(ChainCommand::SubmitSignedExtrinsic {
                extrinsic,
                reply: reply_tx,
            })
            .await
            .map_err(|e| format!("Failed to send command: {}", e))?;
        reply_rx.await.map_err(|_| "Channel closed".to_string())?
    }
}

/// Chain worker state.
struct ChainWorker {
    client: Option<ChainClient>,
    people_client: Option<PeopleChainClient>,
    update_tx: mpsc::Sender<ChainUpdate>,
    db: Option<DbService>,
    network: Option<Network>,
    use_light_client: bool,
}

impl ChainWorker {
    fn new(update_tx: mpsc::Sender<ChainUpdate>, db: Option<DbService>) -> Self {
        Self {
            client: None,
            people_client: None,
            update_tx,
            db,
            network: None,
            use_light_client: false,
        }
    }

    async fn set_progress(&self, step: LoadingStep, progress: f32) {
        let _ = self
            .update_tx
            .send(ChainUpdate::LoadingProgress { step, progress })
            .await;
    }

    async fn load_cached_startup_data(&self, network: Network, current_era: u32) -> (bool, bool) {
        let mut validators_fresh = false;
        let mut pools_fresh = false;

        if let Some(ref db) = self.db {
            match db.get_startup_cache(network, current_era).await {
                Ok(startup) => {
                    if startup.validators.is_displayable() && !startup.validators.data.is_empty() {
                        let validator_apy_entries =
                            validator_apy_map(&startup.validators.data).len();
                        let has_chain_data =
                            cached_validators_have_chain_data(&startup.validators.data);
                        validators_fresh = !startup.validators.needs_refresh() && has_chain_data;
                        let freshness = startup.validators.freshness;
                        let validators = startup.validators.data;
                        let count = validators.len();
                        tracing::info!(
                            "Using {:?} startup validator cache: {} validators for era {} ({} APY entries, chain_data={})",
                            freshness,
                            count,
                            current_era,
                            validator_apy_entries,
                            has_chain_data
                        );
                        if has_chain_data {
                            self.set_progress(LoadingStep::Validators, 1.0).await;
                            let _ = self
                                .update_tx
                                .send(ChainUpdate::ValidatorsLoaded(validators))
                                .await;
                        }
                    }

                    if startup.pools.is_displayable() && !startup.pools.data.is_empty() {
                        pools_fresh = !startup.pools.needs_refresh();
                        let freshness = startup.pools.freshness;
                        let pools = startup.pools.data;
                        let count = pools.len();
                        tracing::info!(
                            "Using {:?} startup pool cache: {} pools for era {}",
                            freshness,
                            count,
                            current_era
                        );
                        self.set_progress(LoadingStep::Pools, 1.0).await;
                        let _ = self.update_tx.send(ChainUpdate::PoolsLoaded(pools)).await;
                    }
                }
                Err(e) => tracing::debug!("Failed to load startup cache: {}", e),
            }
        }

        (validators_fresh, pools_fresh)
    }

    async fn handle_connect(&mut self, network: Network, use_light_client: bool) {
        self.network = Some(network);
        self.use_light_client = use_light_client;

        // Send connecting status
        let _ = self
            .update_tx
            .send(ChainUpdate::ConnectionStatus(ConnectionStatus::Connecting))
            .await;

        let config = ConnectionConfig {
            mode: if use_light_client {
                ChainConnectionMode::LightClient
            } else {
                ChainConnectionMode::Rpc
            },
            rpc_endpoints: RpcEndpoints::default(),
        };

        // Create status channel
        let (status_tx, mut status_rx) = mpsc::channel::<ConnectionStatus>(10);

        // Forward status updates
        let update_tx = self.update_tx.clone();
        tokio::spawn(async move {
            while let Some(status) = status_rx.recv().await {
                if matches!(status, ConnectionStatus::Connected) {
                    tracing::debug!(
                        "Ignoring low-level chain Connected status until app startup data is ready"
                    );
                    continue;
                }
                let _ = update_tx.send(ChainUpdate::ConnectionStatus(status)).await;
            }
        });

        match ChainClient::connect(network, &config, status_tx).await {
            Ok(client) => {
                tracing::info!("Connected to {} via {:?}", network, config.mode);
                self.client = Some(client);
                self.set_progress(LoadingStep::Operations, 0.35).await;

                // Fetch and persist chain metadata
                if let Some(ref client) = self.client {
                    let info = match client.get_chain_info().await {
                        Ok(info) => info,
                        Err(e) => {
                            tracing::warn!("Failed to get chain info: {}", e);
                            return;
                        }
                    };
                    let genesis_hash = hex::encode(client.genesis_hash());

                    // Fetch dynamic data
                    let era_duration_ms = client
                        .get_era_duration_ms()
                        .await
                        .unwrap_or(24 * 60 * 60 * 1000);
                    let current_era = client
                        .get_active_era()
                        .await
                        .ok()
                        .flatten()
                        .map(|e| e.index)
                        .unwrap_or(0);

                    // Token properties based on network
                    let token_symbol = network.token_symbol().to_string();
                    let token_decimals = network.token_decimals();
                    let ss58_prefix = network.ss58_format();

                    if let Some(ref db) = self.db {
                        let cached_meta = CachedChainMetadata {
                            genesis_hash,
                            spec_version: info.spec_version,
                            tx_version: info.tx_version,
                            ss58_prefix,
                            token_symbol,
                            token_decimals,
                            era_duration_ms,
                            current_era,
                        };
                        if let Err(e) = db.set_chain_metadata(network, cached_meta).await {
                            tracing::warn!("Failed to cache chain metadata: {}", e);
                        }
                    }
                }
                self.set_progress(LoadingStep::Operations, 1.0).await;

                let current_era = if let Some(ref db) = self.db {
                    db.get_chain_metadata(network)
                        .await
                        .ok()
                        .flatten()
                        .map(|meta| meta.current_era)
                        .unwrap_or(0)
                } else {
                    0
                };
                let (validators_cached, pools_cached) =
                    self.load_cached_startup_data(network, current_era).await;

                // Auto-fetch only the startup data that was not already cached.
                if !validators_cached {
                    self.set_progress(LoadingStep::Validators, 0.1).await;
                    if let Some(ref client) = self.client {
                        self.people_client = connect_ready_people_client(client, network).await;
                    }
                    self.fetch_validators_internal(network).await;
                }
                if !pools_cached {
                    self.set_progress(LoadingStep::Pools, 0.1).await;
                    // Delay before pool fetch: light client needs time for storage iteration
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    self.fetch_pools_internal(network).await;
                }
                let _ = self
                    .update_tx
                    .send(ChainUpdate::ConnectionStatus(ConnectionStatus::Connected))
                    .await;
            }
            Err(e) => {
                tracing::error!("Failed to connect: {}", e);
                let _ = self.update_tx.send(ChainUpdate::Error(e.to_string())).await;
                let _ = self
                    .update_tx
                    .send(ChainUpdate::ConnectionStatus(ConnectionStatus::Error(
                        e.to_string(),
                    )))
                    .await;
            }
        }
    }

    async fn handle_disconnect(&mut self) {
        self.client = None;
        self.people_client = None;
        let _ = self
            .update_tx
            .send(ChainUpdate::ConnectionStatus(
                ConnectionStatus::Disconnected,
            ))
            .await;
    }

    /// Check if an error string indicates a lost connection.
    fn is_connection_error(err: &str) -> bool {
        err.contains("ConnectionShutdown")
            || err.contains("Not connected")
            || err.contains("Rpc error")
            || err.contains("connection closed")
            || err.contains("channel closed")
    }

    /// Attempt to reconnect using stored connection params.
    async fn try_reconnect(&mut self) -> bool {
        let Some(network) = self.network else {
            return false;
        };
        tracing::warn!("Connection lost, attempting reconnection to {}...", network);
        self.client = None;
        self.people_client = None;
        self.handle_connect(network, self.use_light_client).await;
        self.client.is_some()
    }

    async fn fetch_validators_internal(&mut self, network: Network) {
        if let Some(ref client) = self.client {
            self.set_progress(LoadingStep::Validators, 0.2).await;
            let fetch_result = if client.is_light_client() {
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
                    let validators = fetch.validators;
                    self.set_progress(LoadingStep::Validators, 0.45).await;
                    tracing::info!(
                        "Fetched {} raw validators, enriching with stake/identity/APY data...",
                        validators.len()
                    );

                    if self.people_client.is_none() {
                        self.people_client = connect_ready_people_client(client, network).await;
                    }

                    // Enrich validators with full data (stake, identity, APY)
                    let enriched = enrich_validators(
                        client,
                        &validators,
                        self.people_client.as_ref(),
                        self.db.as_ref(),
                        network,
                    )
                    .await;
                    self.set_progress(LoadingStep::Validators, 0.85).await;

                    // Persist to DB
                    if let Some(ref db) = self.db {
                        // Get current era from metadata or default to 0
                        let era = if let Ok(Some(meta)) = db.get_chain_metadata(network).await {
                            meta.current_era
                        } else {
                            0
                        };

                        if let Err(e) = db
                            .set_cached_validators_checked(
                                network,
                                era,
                                enriched.clone(),
                                fetch.complete,
                            )
                            .await
                        {
                            tracing::warn!("Failed to cache validators: {}", e);
                        }
                    }

                    tracing::info!("Sending {} enriched validators to UI", enriched.len());
                    self.set_progress(LoadingStep::Validators, 1.0).await;
                    let _ = self
                        .update_tx
                        .send(ChainUpdate::ValidatorsLoaded(enriched))
                        .await;
                }
                Err(e) => {
                    tracing::error!("Failed to fetch validators: {}", e);
                    let _ = self
                        .update_tx
                        .send(ChainUpdate::Error(format!(
                            "Failed to fetch validators: {}",
                            e
                        )))
                        .await;
                }
            }
        }
    }

    async fn fetch_account_once(
        &self,
        network: Network,
        address: &str,
    ) -> Result<AccountData, String> {
        let Some(ref client) = self.client else {
            return Err("Not connected".to_string());
        };
        let account_id: subxt::utils::AccountId32 = address
            .parse()
            .map_err(|e| format!("Invalid address: {}", e))?;

        // Fetch balance
        let balance = client.get_account_balance(&account_id).await;
        let staking = client.get_staking_ledger(&account_id).await;
        let nominations = client.get_nominations(&account_id).await;
        let pool = client.get_pool_membership(&account_id).await;

        let bal = balance.map_err(|e| format!("Failed to fetch balance: {}", e))?;

        let staking_ledger = staking.ok().flatten();
        let staked = staking_ledger.as_ref().map(|s| s.active);
        let unbonding = staking_ledger
            .as_ref()
            .map(|s| s.unlocking.iter().map(|c| c.value).sum())
            .unwrap_or(0);
        let noms: Vec<String> = nominations
            .ok()
            .flatten()
            .map(|n| n.targets.iter().map(|t| t.to_string()).collect())
            .unwrap_or_default();

        // Get pool membership and calculate pending rewards
        let pool_membership = pool.ok().flatten();
        let pool_id = pool_membership.as_ref().map(|p| p.pool_id);
        let pool_pending_rewards = if let Some(ref membership) = pool_membership {
            match client
                .get_pool_pending_rewards(
                    membership.pool_id,
                    membership.points,
                    membership.last_recorded_reward_counter,
                )
                .await
            {
                Ok(pending) => {
                    tracing::info!("Pool {} pending rewards: {}", membership.pool_id, pending);
                    pending
                }
                Err(e) => {
                    tracing::warn!("Failed to calculate pool pending rewards: {}", e);
                    0
                }
            }
        } else {
            0
        };

        let account_data = AccountData {
            free_balance: bal.free,
            reserved_balance: bal.reserved,
            frozen_balance: bal.frozen,
            staked_balance: staked,
            unbonding_balance: unbonding,
            is_nominating: !noms.is_empty(),
            nominations: noms.clone(),
            pool_id,
            pool_pending_rewards,
            pool_unbonding_eras: pool_membership
                .as_ref()
                .map(|membership| membership.unbonding_eras.clone())
                .unwrap_or_default(),
            pool_last_recorded_reward_counter: pool_membership
                .as_ref()
                .map(|membership| membership.last_recorded_reward_counter)
                .unwrap_or_default(),
        };

        // Persist to DB
        if let Some(ref db) = self.db {
            let cached_status = CachedAccountStatus {
                free_balance: account_data.free_balance,
                reserved_balance: account_data.reserved_balance,
                frozen_balance: bal.frozen,
                staked_amount: account_data.staked_balance.unwrap_or(0),
                nominations_json: if noms.is_empty() {
                    None
                } else {
                    serde_json::to_string(&noms).ok()
                },
                pool_id: account_data.pool_id,
                pool_points: pool_membership.as_ref().map(|membership| membership.points),
                unlocking_json: staking_ledger.as_ref().and_then(|ledger| {
                    if ledger.unlocking.is_empty() {
                        None
                    } else {
                        serde_json::to_string(&ledger.unlocking).ok()
                    }
                }),
                pool_unbonding_eras_json: pool_membership.as_ref().and_then(|membership| {
                    if membership.unbonding_eras.is_empty() {
                        None
                    } else {
                        serde_json::to_string(&membership.unbonding_eras).ok()
                    }
                }),
                pool_last_recorded_reward_counter: pool_membership
                    .as_ref()
                    .map(|membership| membership.last_recorded_reward_counter)
                    .unwrap_or_default(),
            };
            if let Err(e) = db
                .set_cached_account_status(network, address.to_string(), cached_status)
                .await
            {
                tracing::warn!("Failed to cache account status: {}", e);
            }
        }

        Ok(account_data)
    }

    async fn handle_fetch_account(
        &mut self,
        network: Network,
        address: String,
        reply: oneshot::Sender<Result<AccountData, String>>,
    ) {
        if let Some(ref db) = self.db {
            match db
                .get_recent_cached_account_status(network, address.clone())
                .await
            {
                Ok(Some(status)) => {
                    tracing::info!("Using recent cached account status for {}", address);
                    let _ = reply.send(Ok(account_data_from_cache(&status)));

                    let mut result = self.fetch_account_once(network, &address).await;
                    if let Err(ref e) = result
                        && Self::is_connection_error(e)
                        && self.try_reconnect().await
                    {
                        result = self.fetch_account_once(network, &address).await;
                    }
                    match result {
                        Ok(account_data) => {
                            let _ = self
                                .update_tx
                                .send(ChainUpdate::AccountLoaded(account_data))
                                .await;
                        }
                        Err(e) => {
                            tracing::warn!("Failed to refresh cached account status: {}", e);
                        }
                    }
                    return;
                }
                Ok(None) => {}
                Err(e) => tracing::debug!("Failed to load cached account status: {}", e),
            }
        }

        let mut result = self.fetch_account_once(network, &address).await;
        if let Err(ref e) = result
            && Self::is_connection_error(e)
            && self.try_reconnect().await
        {
            result = self.fetch_account_once(network, &address).await;
        }
        let _ = reply.send(result);
    }

    async fn fetch_validators_once(
        &mut self,
        network: Network,
    ) -> Result<Vec<ValidatorInfo>, String> {
        let Some(ref client) = self.client else {
            return Err("Not connected".to_string());
        };
        let fetch_result = if client.is_light_client() {
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
                let validators = fetch.validators;
                tracing::info!("Fetched {} raw validators, enriching...", validators.len());
                if self.people_client.is_none() {
                    self.people_client = connect_ready_people_client(client, network).await;
                }
                let enriched = enrich_validators(
                    client,
                    &validators,
                    self.people_client.as_ref(),
                    self.db.as_ref(),
                    network,
                )
                .await;

                // Persist to DB
                if let Some(ref db) = self.db {
                    let era = if let Ok(Some(meta)) = db.get_chain_metadata(network).await {
                        meta.current_era
                    } else {
                        0
                    };

                    if let Err(e) = db
                        .set_cached_validators_checked(
                            network,
                            era,
                            enriched.clone(),
                            fetch.complete,
                        )
                        .await
                    {
                        tracing::warn!("Failed to cache validators: {}", e);
                    }
                }

                Ok(enriched)
            }
            Err(e) => Err(e.to_string()),
        }
    }

    async fn validator_apy_map_for_pools(&mut self, network: Network) -> HashMap<String, f64> {
        if let Some(ref db) = self.db {
            let current_era = db
                .get_chain_metadata(network)
                .await
                .ok()
                .flatten()
                .map(|meta| meta.current_era)
                .unwrap_or(0);
            match db.get_fresh_cached_validators(network, current_era).await {
                Ok(validators) => {
                    let cached_map = validator_apy_map(&validators);
                    if !cached_map.is_empty() {
                        tracing::info!(
                            "Using {} cached validator APY entries for pool APY calculation",
                            cached_map.len()
                        );
                        return cached_map;
                    }
                }
                Err(e) => tracing::debug!("Failed to load cached validators for pool APY: {}", e),
            }
        }

        match self.fetch_validators_once(network).await {
            Ok(validators) => {
                let map = validator_apy_map(&validators);
                tracing::info!(
                    "Fetched {} validator APY entries for pool APY calculation",
                    map.len()
                );
                map
            }
            Err(e) => {
                tracing::warn!("Failed to fetch validators for pool APY calculation: {}", e);
                HashMap::new()
            }
        }
    }

    async fn handle_fetch_validators(
        &mut self,
        network: Network,
        reply: oneshot::Sender<Result<Vec<ValidatorInfo>, String>>,
    ) {
        let mut result = self.fetch_validators_once(network).await;
        if let Err(ref e) = result
            && Self::is_connection_error(e)
            && self.try_reconnect().await
        {
            result = self.fetch_validators_once(network).await;
        }
        let _ = reply.send(result);
    }

    async fn fetch_pools_once(&mut self, network: Network) -> Result<Vec<PoolInfo>, String> {
        self.set_progress(LoadingStep::Pools, 0.2).await;
        let pools = {
            let Some(client) = self.client.as_ref() else {
                return Err("Not connected".to_string());
            };
            client
                .get_nomination_pools()
                .await
                .map_err(|error| error.to_string())?
        };

        self.set_progress(LoadingStep::Pools, 0.45).await;
        let validator_apy_map = self.validator_apy_map_for_pools(network).await;
        self.set_progress(LoadingStep::Pools, 0.65).await;
        tracing::info!(
            "Built validator APY map with {} entries for pool APY calculation",
            validator_apy_map.len()
        );

        let max_pools_to_query = 30.min(pools.len());
        let outcome = {
            let Some(client) = self.client.as_ref() else {
                return Err("Not connected".to_string());
            };
            match fetch_and_enrich_pools(
                client,
                &pools,
                &validator_apy_map,
                max_pools_to_query,
                None,
            )
            .await
            {
                Ok(outcome) => outcome,
                Err(e) => {
                    tracing::error!("Failed to enrich pools: {}", e);
                    return Err(e.to_string());
                }
            }
        };
        tracing::info!("Fetched {} pool names", outcome.metadata_map.len());
        self.set_progress(LoadingStep::Pools, 0.85).await;

        if let Some(ref db) = self.db {
            let era = db
                .get_chain_metadata(network)
                .await
                .ok()
                .flatten()
                .map(|meta| meta.current_era)
                .unwrap_or(0);
            if let Err(e) = db
                .set_cached_pools_at_era(network, era, outcome.pools.clone())
                .await
            {
                tracing::warn!("Failed to cache pools: {:?}", e);
            }
        }

        self.set_progress(LoadingStep::Pools, 1.0).await;
        Ok(outcome.pools)
    }

    async fn fetch_pools_internal(&mut self, network: Network) {
        match self.fetch_pools_once(network).await {
            Ok(pools) => {
                tracing::info!("Sending {} pools to UI", pools.len());
                let _ = self.update_tx.send(ChainUpdate::PoolsLoaded(pools)).await;
            }
            Err(e) => {
                tracing::error!("Failed to fetch pools: {}", e);
                let _ = self
                    .update_tx
                    .send(ChainUpdate::Error(format!("Failed to fetch pools: {}", e)))
                    .await;
            }
        }
    }

    async fn handle_fetch_pools(
        &mut self,
        network: Network,
        reply: oneshot::Sender<Result<Vec<PoolInfo>, String>>,
    ) {
        let mut result = self.fetch_pools_once(network).await;
        if let Err(ref e) = result
            && Self::is_connection_error(e)
            && self.try_reconnect().await
        {
            result = self.fetch_pools_once(network).await;
        }
        let _ = reply.send(result);
    }

    // === Transaction Payload Handlers ===

    async fn handle_create_bond_payload(
        &self,
        signer: AccountId32,
        value: u128,
        reply: oneshot::Sender<Result<TransactionPayload, String>>,
    ) {
        let result = if let Some(ref client) = self.client {
            client
                .create_bond_payload(&signer, value, true)
                .await
                .map_err(|e| format!("Failed to create bond payload: {}", e))
                .and_then(|p| make_transaction_payload(p, signer))
        } else {
            Err("Not connected".to_string())
        };
        let _ = reply.send(result);
    }

    async fn handle_create_unbond_payload(
        &self,
        signer: AccountId32,
        value: u128,
        reply: oneshot::Sender<Result<TransactionPayload, String>>,
    ) {
        let result = if let Some(ref client) = self.client {
            client
                .create_unbond_payload(&signer, value, true)
                .await
                .map_err(|e| format!("Failed to create unbond payload: {}", e))
                .and_then(|p| make_transaction_payload(p, signer))
        } else {
            Err("Not connected".to_string())
        };
        let _ = reply.send(result);
    }

    async fn handle_create_bond_extra_payload(
        &self,
        signer: AccountId32,
        value: u128,
        reply: oneshot::Sender<Result<TransactionPayload, String>>,
    ) {
        let result = if let Some(ref client) = self.client {
            client
                .create_bond_extra_payload(&signer, value, true)
                .await
                .map_err(|e| format!("Failed to create bond_extra payload: {}", e))
                .and_then(|p| make_transaction_payload(p, signer))
        } else {
            Err("Not connected".to_string())
        };
        let _ = reply.send(result);
    }

    async fn handle_create_rebond_payload(
        &self,
        signer: AccountId32,
        value: u128,
        reply: oneshot::Sender<Result<TransactionPayload, String>>,
    ) {
        let result = if let Some(ref client) = self.client {
            client
                .create_rebond_payload(&signer, value, true)
                .await
                .map_err(|e| format!("Failed to create rebond payload: {}", e))
                .and_then(|p| make_transaction_payload(p, signer))
        } else {
            Err("Not connected".to_string())
        };
        let _ = reply.send(result);
    }

    async fn handle_create_set_payee_payload(
        &self,
        signer: AccountId32,
        destination: RewardDestination,
        reply: oneshot::Sender<Result<TransactionPayload, String>>,
    ) {
        let result = if let Some(ref client) = self.client {
            client
                .create_set_payee_payload(&signer, destination, true)
                .await
                .map_err(|e| format!("Failed to create set_payee payload: {}", e))
                .and_then(|p| make_transaction_payload(p, signer))
        } else {
            Err("Not connected".to_string())
        };
        let _ = reply.send(result);
    }

    async fn handle_create_withdraw_unbonded_payload(
        &self,
        signer: AccountId32,
        reply: oneshot::Sender<Result<TransactionPayload, String>>,
    ) {
        let result = if let Some(ref client) = self.client {
            let slashing_spans =
                slashing_spans_for_withdraw(client.get_slashing_spans(&signer).await);
            client
                .create_withdraw_unbonded_payload(&signer, slashing_spans, true)
                .await
                .map_err(|e| format!("Failed to create withdraw_unbonded payload: {}", e))
                .and_then(|p| make_transaction_payload(p, signer))
        } else {
            Err("Not connected".to_string())
        };
        let _ = reply.send(result);
    }

    async fn handle_create_chill_payload(
        &self,
        signer: AccountId32,
        reply: oneshot::Sender<Result<TransactionPayload, String>>,
    ) {
        let result = if let Some(ref client) = self.client {
            client
                .create_chill_payload(&signer, true)
                .await
                .map_err(|e| format!("Failed to create chill payload: {}", e))
                .and_then(|p| make_transaction_payload(p, signer))
        } else {
            Err("Not connected".to_string())
        };
        let _ = reply.send(result);
    }

    async fn handle_create_nominate_payload(
        &self,
        signer: AccountId32,
        targets: Vec<AccountId32>,
        reply: oneshot::Sender<Result<TransactionPayload, String>>,
    ) {
        let result = if let Some(ref client) = self.client {
            client
                .create_nominate_payload(&signer, &targets, true)
                .await
                .map_err(|e| format!("Failed to create nominate payload: {}", e))
                .and_then(|p| make_transaction_payload(p, signer))
        } else {
            Err("Not connected".to_string())
        };
        let _ = reply.send(result);
    }

    // === Pool Operation Handlers ===

    async fn handle_create_pool_join_payload(
        &self,
        signer: AccountId32,
        pool_id: u32,
        amount: u128,
        reply: oneshot::Sender<Result<TransactionPayload, String>>,
    ) {
        let result = if let Some(ref client) = self.client {
            client
                .create_pool_join_payload(&signer, pool_id, amount, true)
                .await
                .map_err(|e| format!("Failed to create pool_join payload: {}", e))
                .and_then(|p| make_transaction_payload(p, signer))
        } else {
            Err("Not connected".to_string())
        };
        let _ = reply.send(result);
    }

    async fn handle_create_pool_bond_extra_payload(
        &self,
        signer: AccountId32,
        amount: u128,
        reply: oneshot::Sender<Result<TransactionPayload, String>>,
    ) {
        let result = if let Some(ref client) = self.client {
            client
                .create_pool_bond_extra_payload(&signer, amount, true)
                .await
                .map_err(|e| format!("Failed to create pool_bond_extra payload: {}", e))
                .and_then(|p| make_transaction_payload(p, signer))
        } else {
            Err("Not connected".to_string())
        };
        let _ = reply.send(result);
    }

    async fn handle_create_pool_claim_payload(
        &self,
        signer: AccountId32,
        reply: oneshot::Sender<Result<TransactionPayload, String>>,
    ) {
        let result = if let Some(ref client) = self.client {
            client
                .create_pool_claim_payload(&signer, true)
                .await
                .map_err(|e| format!("Failed to create pool_claim payload: {}", e))
                .and_then(|p| make_transaction_payload(p, signer))
        } else {
            Err("Not connected".to_string())
        };
        let _ = reply.send(result);
    }

    async fn handle_create_pool_unbond_payload(
        &self,
        signer: AccountId32,
        amount: u128,
        reply: oneshot::Sender<Result<TransactionPayload, String>>,
    ) {
        let result = if let Some(ref client) = self.client {
            // For pool unbond, member_account is the same as signer
            client
                .create_pool_unbond_payload(&signer, &signer, amount, true)
                .await
                .map_err(|e| format!("Failed to create pool_unbond payload: {}", e))
                .and_then(|p| make_transaction_payload(p, signer))
        } else {
            Err("Not connected".to_string())
        };
        let _ = reply.send(result);
    }

    async fn handle_create_pool_withdraw_payload(
        &self,
        signer: AccountId32,
        reply: oneshot::Sender<Result<TransactionPayload, String>>,
    ) {
        let result = if let Some(ref client) = self.client {
            // For pool withdraw, member_account is the same as signer
            let slashing_spans =
                slashing_spans_for_withdraw(client.get_slashing_spans(&signer).await);
            client
                .create_pool_withdraw_payload(&signer, &signer, slashing_spans, true)
                .await
                .map_err(|e| format!("Failed to create pool_withdraw payload: {}", e))
                .and_then(|p| make_transaction_payload(p, signer))
        } else {
            Err("Not connected".to_string())
        };
        let _ = reply.send(result);
    }

    // === Transaction Submission Handler ===

    async fn handle_submit_signed_extrinsic(
        &self,
        extrinsic: Vec<u8>,
        reply: oneshot::Sender<Result<TxSubmissionResult, String>>,
    ) {
        let result = if let Some(ref client) = self.client {
            match client.submit_signed_extrinsic(&extrinsic).await {
                Ok(progress) => {
                    // Wait for inclusion in block
                    match progress.wait_for_in_block().await {
                        Ok(in_block) => {
                            if in_block.finalized {
                                Ok(TxSubmissionResult::Finalized {
                                    block_hash: in_block.block_hash,
                                })
                            } else {
                                Ok(TxSubmissionResult::InBlock {
                                    block_hash: in_block.block_hash,
                                })
                            }
                        }
                        Err(e) => Err(format!("Transaction failed: {}", e)),
                    }
                }
                Err(e) => Err(format!("Failed to submit transaction: {}", e)),
            }
        } else {
            Err("Not connected".to_string())
        };
        let _ = reply.send(result);
    }

    // === History Fetching Handler ===

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
    ) -> Option<HistoryPoint>
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

        // Log raw values for debugging
        tracing::debug!(
            "Era {} raw data: era_reward={}, total_staked={}, user_bonded={}, apy={:.4}",
            era,
            era_reward,
            total_staked,
            user_bonded,
            point.apy
        );

        // Skip eras with unrealistic APY (likely corrupted data)
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

        tracing::debug!(
            "Added history point for era {} (APY: {:.2}%)",
            era,
            point.apy * 100.0
        );

        Some(point)
    }

    async fn handle_fetch_history(
        &mut self,
        network: Network,
        address: String,
        lookback_days: u32,
        reply: oneshot::Sender<Result<Vec<HistoryPoint>, String>>,
    ) {
        if self.client.is_none() && !self.try_reconnect().await {
            let _ = reply.send(Err("Not connected".to_string()));
            return;
        }
        let Some(ref client) = self.client else {
            let _ = reply.send(Err("Not connected".to_string()));
            return;
        };

        tracing::info!(
            "Loading staking history for {} (last {} days)",
            address,
            lookback_days
        );

        // Parse address to AccountId32 for chain queries
        let account: AccountId32 = match address.parse() {
            Ok(a) => a,
            Err(e) => {
                let _ = reply.send(Err(format!("Invalid address: {}", e)));
                return;
            }
        };

        // Approximate fallback limit using the default era duration in case the active era
        // lookup fails before we know the real duration.
        let fallback_eras = eras_for_lookback_days(lookback_days, 24 * 60 * 60 * 1000);

        // Try to load fallback cached history first, in case active era lookup fails.
        let mut fallback_cached_history = Vec::new();
        if let Some(ref db) = self.db {
            match db
                .get_latest_history_cache(network, address.clone(), Some(fallback_eras))
                .await
            {
                Ok(history) if !history.is_empty() => {
                    tracing::info!("Loaded {} cached history points", history.len());
                    fallback_cached_history = history;
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("Failed to load cached history: {}", e);
                }
            }
        }

        // Get current era
        let current_era_info = match client.get_active_era().await {
            Ok(Some(era)) => era,
            Ok(None) => {
                // No active era, return cached data if any
                let _ = reply.send(Ok(fallback_cached_history));
                return;
            }
            Err(e) => {
                tracing::error!("Failed to get active era: {}", e);
                let _ = reply.send(Ok(fallback_cached_history));
                return;
            }
        };
        let current_era = current_era_info.index;
        let current_era_start_ms = current_era_info.start_timestamp_ms;
        let era_duration_ms = current_era_info.duration_ms;
        let num_eras = eras_for_lookback_days(lookback_days, era_duration_ms);
        let (start_era, end_era) = HistoryService::era_window(current_era, num_eras);

        let cached_history = if let Some(ref db) = self.db {
            match db
                .get_history_cache_range(
                    network,
                    address.clone(),
                    start_era,
                    end_era,
                    current_era,
                    current_era_start_ms,
                    era_duration_ms,
                )
                .await
            {
                Ok(history) if !history.is_empty() => {
                    tracing::info!(
                        "Loaded {} valid cached history points for eras {}..={}",
                        history.len(),
                        start_era,
                        end_era
                    );
                    history
                }
                Ok(_) => Vec::new(),
                Err(e) => {
                    tracing::warn!("Failed to load cached history range: {}", e);
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };

        // Get user's bonded amount for APY calculation
        let user_bonded = match client.get_staking_ledger(&account).await {
            Ok(Some(ledger)) => ledger.active,
            _ => 0,
        };

        let eras_to_fetch: Vec<u32> = if let Some(ref db) = self.db {
            db.get_missing_history_eras(network, address.clone(), start_era, end_era)
                .await
                .unwrap_or_else(|_| (start_era..current_era).collect())
        } else {
            (start_era..current_era).collect()
        };

        if eras_to_fetch.is_empty() {
            tracing::info!("All eras already cached");
            let _ = reply.send(Ok(cached_history));
            return;
        }

        tracing::info!("Fetching {} missing eras from chain", eras_to_fetch.len());

        let concurrency = 8;
        let mut fetched = stream::iter(eras_to_fetch)
            .map(|era| async move {
                let point = Self::fetch_history_era(
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
            })
            .buffer_unordered(concurrency)
            .collect::<Vec<_>>()
            .await;

        // Preserve era ordering when emitting/processing the parallel results.
        fetched.sort_by_key(|(era, _)| *era);

        let mut new_points = Vec::new();
        for (_era, point) in fetched {
            if let Some(point) = point {
                new_points.push(point);
            }
        }

        // Cache new points to database
        if let Some(ref db) = self.db
            && !new_points.is_empty()
        {
            if let Err(e) = db
                .insert_history_batch(network, address.clone(), new_points.clone())
                .await
            {
                tracing::warn!("Failed to cache history: {}", e);
            } else {
                tracing::info!("Cached {} new history points", new_points.len());
            }
        }

        // Combine cached and new points, sort by era
        let mut all_points = cached_history;
        all_points.extend(new_points);
        all_points.sort_by_key(|p| p.era);

        tracing::info!("Staking history loaded: {} total points", all_points.len());
        let _ = reply.send(Ok(all_points));
    }
}

/// Spawn the chain worker and return a handle.
pub fn spawn_chain_worker(
    db: Option<DbService>,
    handle: tokio::runtime::Handle,
) -> (ChainHandle, mpsc::Receiver<ChainUpdate>) {
    let (command_tx, mut command_rx) = mpsc::channel::<ChainCommand>(32);
    let (update_tx, update_rx) = mpsc::channel::<ChainUpdate>(32);

    handle.spawn(async move {
        let mut worker = ChainWorker::new(update_tx, db);
        let mut current_network = Network::Polkadot; // Track current network for DB operations

        while let Some(command) = command_rx.recv().await {
            match command {
                ChainCommand::Connect {
                    network,
                    use_light_client,
                } => {
                    current_network = network;
                    worker.handle_connect(network, use_light_client).await;
                }
                ChainCommand::Disconnect => {
                    worker.handle_disconnect().await;
                }
                ChainCommand::FetchAccount { address, reply } => {
                    worker
                        .handle_fetch_account(current_network, address, reply)
                        .await;
                }
                ChainCommand::FetchValidators { reply } => {
                    worker.handle_fetch_validators(current_network, reply).await;
                }
                ChainCommand::FetchPools { reply } => {
                    worker.handle_fetch_pools(current_network, reply).await;
                }
                ChainCommand::FetchHistory {
                    address,
                    lookback_days,
                    reply,
                } => {
                    worker
                        .handle_fetch_history(current_network, address, lookback_days, reply)
                        .await;
                }
                // === Transaction Payload Generation ===
                ChainCommand::CreateBondPayload {
                    signer,
                    value,
                    reply,
                } => {
                    worker
                        .handle_create_bond_payload(signer, value, reply)
                        .await;
                }
                ChainCommand::CreateUnbondPayload {
                    signer,
                    value,
                    reply,
                } => {
                    worker
                        .handle_create_unbond_payload(signer, value, reply)
                        .await;
                }
                ChainCommand::CreateBondExtraPayload {
                    signer,
                    value,
                    reply,
                } => {
                    worker
                        .handle_create_bond_extra_payload(signer, value, reply)
                        .await;
                }
                ChainCommand::CreateRebondPayload {
                    signer,
                    value,
                    reply,
                } => {
                    worker
                        .handle_create_rebond_payload(signer, value, reply)
                        .await;
                }
                ChainCommand::CreateSetPayeePayload {
                    signer,
                    destination,
                    reply,
                } => {
                    worker
                        .handle_create_set_payee_payload(signer, destination, reply)
                        .await;
                }
                ChainCommand::CreateWithdrawUnbondedPayload { signer, reply } => {
                    worker
                        .handle_create_withdraw_unbonded_payload(signer, reply)
                        .await;
                }
                ChainCommand::CreateChillPayload { signer, reply } => {
                    worker.handle_create_chill_payload(signer, reply).await;
                }
                ChainCommand::CreateNominatePayload {
                    signer,
                    targets,
                    reply,
                } => {
                    worker
                        .handle_create_nominate_payload(signer, targets, reply)
                        .await;
                }
                // === Pool Operations ===
                ChainCommand::CreatePoolJoinPayload {
                    signer,
                    pool_id,
                    amount,
                    reply,
                } => {
                    worker
                        .handle_create_pool_join_payload(signer, pool_id, amount, reply)
                        .await;
                }
                ChainCommand::CreatePoolBondExtraPayload {
                    signer,
                    amount,
                    reply,
                } => {
                    worker
                        .handle_create_pool_bond_extra_payload(signer, amount, reply)
                        .await;
                }
                ChainCommand::CreatePoolClaimPayload { signer, reply } => {
                    worker.handle_create_pool_claim_payload(signer, reply).await;
                }
                ChainCommand::CreatePoolUnbondPayload {
                    signer,
                    amount,
                    reply,
                } => {
                    worker
                        .handle_create_pool_unbond_payload(signer, amount, reply)
                        .await;
                }
                ChainCommand::CreatePoolWithdrawPayload { signer, reply } => {
                    worker
                        .handle_create_pool_withdraw_payload(signer, reply)
                        .await;
                }
                // === Transaction Submission ===
                ChainCommand::SubmitSignedExtrinsic { extrinsic, reply } => {
                    worker
                        .handle_submit_signed_extrinsic(extrinsic, reply)
                        .await;
                }
            }
        }
    });

    (ChainHandle { command_tx }, update_rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_data_default() {
        let data = AccountData {
            free_balance: 1000,
            reserved_balance: 100,
            frozen_balance: 50,
            staked_balance: Some(500),
            unbonding_balance: 200,
            is_nominating: true,
            nominations: vec!["validator1".to_string()],
            pool_id: None,
            pool_pending_rewards: 0,
            pool_unbonding_eras: vec![],
            pool_last_recorded_reward_counter: 0,
        };
        assert_eq!(data.free_balance, 1000);
        assert_eq!(data.frozen_balance, 50);
        assert_eq!(data.unbonding_balance, 200);
        assert!(data.is_nominating);
    }

    #[test]
    fn test_account_data_from_cache_preserves_pool_unbonding_and_reward_counter() {
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

        let data = account_data_from_cache(&status);
        assert_eq!(data.free_balance, status.free_balance);
        assert_eq!(data.pool_id, status.pool_id);
        assert_eq!(data.pool_unbonding_eras, vec![(1601, 5_000), (1602, 8_000)]);
        assert_eq!(data.pool_last_recorded_reward_counter, 99_999);
    }

    #[test]
    fn test_chain_update_variants() {
        let update = ChainUpdate::ConnectionStatus(ConnectionStatus::Connected);
        assert!(matches!(
            update,
            ChainUpdate::ConnectionStatus(ConnectionStatus::Connected)
        ));
    }

    fn account(byte: u8) -> AccountId32 {
        AccountId32::from([byte; 32])
    }

    fn display_validator(total_stake: u128, apy: Option<f64>) -> ValidatorInfo {
        ValidatorInfo {
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

    // === make_transaction_payload ===

    #[test]
    fn test_make_transaction_payload_success() {
        let payload = UnsignedPayload {
            call_data: vec![0x01, 0x02],
            description: "Bond 10 DOT".to_string(),
            metadata_hash: [0u8; 32],
            genesis_hash: [0u8; 32],
            block_hash: [0u8; 32],
            spec_version: 1,
            tx_version: 1,
            nonce: 0,
            era: stkopt_chain::Era::Immortal,
            include_metadata_hash: false,
            use_asset_payment: false,
            extension_ids: vec![],
        };
        let signer = account(1);
        let result = make_transaction_payload(payload.clone(), signer).unwrap();
        assert_eq!(result.signer, signer);
        assert_eq!(result.description, "Bond 10 DOT");
        assert!(!result.qr_data.is_empty());
    }

    // === slashing_spans_for_withdraw ===

    #[test]
    fn test_slashing_spans_for_withdraw_uses_value() {
        assert_eq!(slashing_spans_for_withdraw(Ok(7)), 7);
    }

    #[test]
    fn test_slashing_spans_for_withdraw_defaults_to_zero() {
        assert_eq!(
            slashing_spans_for_withdraw(Err(stkopt_chain::ChainError::Storage(
                "no spans".to_string()
            ))),
            0
        );
    }

    // === CachedChainMetadata ===

    #[test]
    fn test_paseo_cached_chain_metadata_ss58_prefix() {
        let meta = CachedChainMetadata {
            genesis_hash: String::new(),
            spec_version: 0,
            tx_version: 0,
            ss58_prefix: Network::Paseo.ss58_format(),
            token_symbol: Network::Paseo.token_symbol().to_string(),
            token_decimals: Network::Paseo.token_decimals(),
            era_duration_ms: 0,
            current_era: 0,
        };
        assert_eq!(meta.ss58_prefix, 0);
        assert_eq!(meta.token_symbol, "PAS");
        assert_eq!(meta.token_decimals, 10);
    }

    #[test]
    fn test_history_lookback_days_to_eras_conversion() {
        let ms_per_day = 24 * 60 * 60 * 1000;
        assert_eq!(stkopt_chain::eras_for_lookback_days(30, ms_per_day), 30);
        assert_eq!(
            stkopt_chain::eras_for_lookback_days(30, 6 * 60 * 60 * 1000),
            120
        );
        assert_eq!(
            stkopt_chain::eras_for_lookback_days(1, 7 * 60 * 60 * 1000),
            4
        );
        assert_eq!(stkopt_chain::eras_for_lookback_days(0, ms_per_day), 1);
    }

    #[test]
    fn test_startup_cache_considered_fresh_at_current_era() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let service = DbService::new_memory(runtime.handle().clone()).unwrap();

        runtime.block_on(async {
            let network = Network::Polkadot;
            service
                .set_chain_metadata(
                    network,
                    CachedChainMetadata {
                        genesis_hash: "0x00".to_string(),
                        spec_version: 1,
                        tx_version: 1,
                        ss58_prefix: network.ss58_format(),
                        token_symbol: network.token_symbol().to_string(),
                        token_decimals: network.token_decimals(),
                        era_duration_ms: 24 * 60 * 60 * 1000,
                        current_era: 1500,
                    },
                )
                .await
                .unwrap();

            service
                .set_cached_validators_checked(
                    network,
                    1500,
                    vec![ValidatorInfo {
                        address: account(1).to_string(),
                        name: None,
                        commission: 0.05,
                        blocked: false,
                        total_stake: 1_000_000,
                        own_stake: 100_000,
                        nominator_count: 10,
                        points: 100,
                        apy: Some(0.12),
                    }],
                    true,
                )
                .await
                .unwrap();

            let startup = service.get_startup_cache(network, 1500).await.unwrap();
            assert!(startup.validators.is_displayable());
            assert!(!startup.validators.data.is_empty());
        });
    }

    // === ChainWorker::is_connection_error ===

    #[test]
    fn test_is_connection_error_connection_shutdown() {
        assert!(ChainWorker::is_connection_error(
            "ConnectionShutdown occurred"
        ));
    }

    #[test]
    fn test_is_connection_error_not_connected() {
        assert!(ChainWorker::is_connection_error("Not connected to node"));
    }

    #[test]
    fn test_is_connection_error_rpc_error() {
        assert!(ChainWorker::is_connection_error("Rpc error: timeout"));
    }

    #[test]
    fn test_is_connection_error_connection_closed() {
        assert!(ChainWorker::is_connection_error(
            "connection closed by peer"
        ));
    }

    #[test]
    fn test_is_connection_error_channel_closed() {
        assert!(ChainWorker::is_connection_error(
            "channel closed unexpectedly"
        ));
    }

    #[test]
    fn test_is_connection_error_unrelated() {
        assert!(!ChainWorker::is_connection_error(" parsing failed"));
        assert!(!ChainWorker::is_connection_error("random text"));
    }

    #[test]
    fn test_is_connection_error_empty() {
        assert!(!ChainWorker::is_connection_error(""));
    }

    // --- ChainWorker::fetch_history_era tests ---

    fn run_async<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Runtime::new().unwrap().block_on(f)
    }

    #[test]
    fn test_fetch_history_era_returns_none_when_reward_missing() {
        let point = run_async(ChainWorker::fetch_history_era(
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
        let point = run_async(ChainWorker::fetch_history_era(
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
        let point = run_async(ChainWorker::fetch_history_era(
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
        let point = run_async(ChainWorker::fetch_history_era(
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
