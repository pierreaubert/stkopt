//! Real chain integration using stkopt-chain.
//!
//! This module provides the actual blockchain connection and data fetching
//! for the GPUI app, using the stkopt-chain crate.

use std::collections::HashMap;
use stkopt_chain::{
    ChainClient, ConnectionConfig, ConnectionMode as ChainConnectionMode, PeopleChainClient,
    PoolState as ChainPoolState, RewardDestination, RpcEndpoints, UnsignedPayload, encode_for_qr,
};
use stkopt_core::{ConnectionStatus, Network, get_era_apy};
use subxt::utils::AccountId32;
use tokio::sync::{mpsc, oneshot};

use crate::app::{HistoryPoint, PoolInfo, PoolState, ValidatorInfo};
use crate::db_service::DbService;
use stkopt_core::db::{CachedAccountStatus, CachedChainMetadata};

/// Maximum realistic APY (50%). Higher values indicate data issues.
const MAX_REALISTIC_APY: f64 = 0.50;

/// Maximum reward as fraction of stake (0.5% per era).
const MAX_REWARD_FRACTION: u128 = 200;

/// Check if an APY value is realistic (not corrupted data).
fn is_realistic_apy(apy: f64) -> bool {
    apy <= MAX_REALISTIC_APY
}

/// Estimate user's reward for an era, capped to avoid unrealistic values.
fn estimate_user_reward(era_reward: u128, user_bonded: u128, total_staked: u128) -> u128 {
    if user_bonded == 0 || total_staked == 0 {
        return 0;
    }
    let estimated = (era_reward as f64 * user_bonded as f64 / total_staked as f64) as u128;
    let max_reasonable = user_bonded / MAX_REWARD_FRACTION;
    if estimated > max_reasonable && max_reasonable > 0 {
        max_reasonable
    } else {
        estimated
    }
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
        eras: u32,
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
    pub staked_balance: Option<u128>,
    pub unbonding_balance: u128,
    pub is_nominating: bool,
    pub nominations: Vec<String>,
    pub pool_id: Option<u32>,
    /// Pending rewards from nomination pool (if member of a pool).
    pub pool_pending_rewards: u128,
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
) -> Vec<ValidatorInfo> {
    // Get active era for staking queries
    let era = match client.get_active_era().await {
        Ok(Some(era)) => era,
        Ok(None) => {
            tracing::warn!("No active era found, returning basic validator data");
            return basic_validator_conversion(validators);
        }
        Err(e) => {
            tracing::warn!(
                "Failed to get active era: {}, returning basic validator data",
                e
            );
            return basic_validator_conversion(validators);
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

    // Get era stakers overview (stake data)
    let exposures = match client.get_era_stakers_overview(query_era).await {
        Ok(exp) => exp,
        Err(e) => {
            tracing::warn!(
                "Failed to get era stakers: {}, returning basic validator data",
                e
            );
            return basic_validator_conversion(validators);
        }
    };
    let exposure_map: HashMap<[u8; 32], _> = exposures
        .iter()
        .map(|e| (*e.address.as_ref(), e.clone()))
        .collect();
    tracing::info!("Got {} era stakers exposures", exposure_map.len());

    // Get era reward points
    let (total_points, points_vec) = match client.get_era_reward_points(query_era).await {
        Ok(result) => result,
        Err(e) => {
            tracing::warn!("Failed to get era reward points: {}", e);
            (0, Vec::new())
        }
    };
    let points_map: HashMap<[u8; 32], u32> = points_vec
        .iter()
        .map(|p| (*p.address.as_ref(), p.points))
        .collect();
    tracing::info!(
        "Got {} validator points, total: {}",
        points_map.len(),
        total_points
    );

    // Get era validator reward for APY calculation
    let era_reward = match client.get_era_validator_reward(query_era).await {
        Ok(Some(reward)) => {
            tracing::info!("Era {} reward: {}", query_era, reward);
            reward
        }
        Ok(None) => {
            tracing::debug!("No era reward for era {}", query_era);
            0
        }
        Err(e) => {
            tracing::warn!("Failed to get era reward: {}", e);
            0
        }
    };

    // Fetch identities from People chain
    let identity_map: HashMap<String, String> = if let Some(people) = people_client {
        let addresses: Vec<AccountId32> = validators.iter().map(|v| v.address.clone()).collect();
        match people.get_identities(&addresses).await {
            Ok(identities) => {
                tracing::info!("Fetched {} identities from People chain", identities.len());
                identities
                    .into_iter()
                    .filter_map(|id| id.display_name.map(|name| (id.address.to_string(), name)))
                    .collect()
            }
            Err(e) => {
                tracing::warn!("Failed to fetch identities: {}", e);
                HashMap::new()
            }
        }
    } else {
        tracing::debug!("People chain client not available, skipping identity fetch");
        HashMap::new()
    };

    // Log diagnostic info for APY calculation
    tracing::info!(
        "APY calculation inputs: era_reward={}, total_points={}, era_duration_ms={}, exposure_count={}",
        era_reward,
        total_points,
        era_duration_ms,
        exposure_map.len()
    );

    // Build enriched validators
    let mut enriched: Vec<ValidatorInfo> = validators
        .iter()
        .map(|v| {
            let addr_bytes: [u8; 32] = *v.address.as_ref();

            // Get exposure data (skip validators not active in this era)
            let exposure = exposure_map.get(&addr_bytes);
            let (total_stake, own_stake, nominator_count) = match exposure {
                Some(e) => (e.total, e.own, e.nominator_count),
                None => {
                    // Include validators without exposure but with zeroed stake data
                    (0, 0, 0)
                }
            };

            let points = points_map.get(&addr_bytes).copied().unwrap_or(0);

            // Calculate APY based on era points
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

            ValidatorInfo {
                address: address_str,
                name,
                commission: v.preferences.commission,
                blocked: v.preferences.blocked,
                total_stake,
                own_stake,
                nominator_count,
                points,
                apy,
            }
        })
        .collect();

    // Sort by APY descending (validators with APY first, then by APY value)
    enriched.sort_by(|a, b| match (a.apy, b.apy) {
        (Some(a_apy), Some(b_apy)) => b_apy
            .partial_cmp(&a_apy)
            .unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });

    tracing::info!(
        "Enriched {} validators with stake/identity/APY data",
        enriched.len()
    );
    enriched
}

/// Basic validator conversion without enrichment (fallback).
fn basic_validator_conversion(validators: &[stkopt_chain::ValidatorInfo]) -> Vec<ValidatorInfo> {
    validators
        .iter()
        .map(|v| ValidatorInfo {
            address: v.address.to_string(),
            name: None,
            commission: v.preferences.commission,
            blocked: v.preferences.blocked,
            total_stake: 0,
            own_stake: 0,
            nominator_count: 0,
            points: 0,
            apy: None,
        })
        .collect()
}

/// Convert an unsigned payload to a transaction payload ready for QR display.
fn make_transaction_payload(payload: UnsignedPayload, signer: AccountId32) -> TransactionPayload {
    let qr_data = encode_for_qr(&payload, &signer);
    let description = payload.description.clone();
    TransactionPayload {
        qr_data,
        unsigned_payload: payload,
        signer,
        description,
    }
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
    pub async fn fetch_history(
        &self,
        address: String,
        eras: u32,
    ) -> Result<Vec<HistoryPoint>, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(ChainCommand::FetchHistory {
                address,
                eras,
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
                let _ = update_tx.send(ChainUpdate::ConnectionStatus(status)).await;
            }
        });

        match ChainClient::connect(network, &config, status_tx).await {
            Ok(client) => {
                tracing::info!("Connected to {} via {:?}", network, config.mode);
                self.client = Some(client);
                let _ = self
                    .update_tx
                    .send(ChainUpdate::ConnectionStatus(ConnectionStatus::Connected))
                    .await;

                // Connect to People chain for identity queries
                if let Some(ref client) = self.client {
                    match client.connect_people_chain().await {
                        Ok(people_subxt) => {
                            tracing::info!("Connected to {} People chain", network);
                            self.people_client = Some(PeopleChainClient::new(people_subxt));
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to connect to People chain (identity data will be unavailable): {}",
                                e
                            );
                            self.people_client = None;
                        }
                    }
                }

                // Fetch and persist chain metadata
                if let Some(ref client) = self.client {
                    let info = client.get_chain_info();
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
                    let (token_symbol, token_decimals, ss58_prefix) = match network {
                        Network::Polkadot => ("DOT".to_string(), 10, 0),
                        Network::Kusama => ("KSM".to_string(), 12, 2),
                        Network::Westend => ("WND".to_string(), 12, 42),
                        Network::Paseo => ("PAS".to_string(), 10, 42),
                    };

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

                // Auto-fetch validators and pools after connection
                self.fetch_validators_internal(network).await;
                // Delay before pool fetch: light client needs time for storage iteration
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                self.fetch_pools_internal(network).await;
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
            match client.get_validators().await {
                Ok(validators) => {
                    tracing::info!(
                        "Fetched {} raw validators, enriching with stake/identity/APY data...",
                        validators.len()
                    );

                    // Enrich validators with full data (stake, identity, APY)
                    let enriched =
                        enrich_validators(client, &validators, self.people_client.as_ref()).await;

                    // Persist to DB
                    if let Some(ref db) = self.db {
                        // Get current era from metadata or default to 0
                        let era = if let Ok(Some(meta)) = db.get_chain_metadata(network).await {
                            meta.current_era
                        } else {
                            0
                        };

                        if let Err(e) = db
                            .set_cached_validators(network, era, enriched.clone())
                            .await
                        {
                            tracing::warn!("Failed to cache validators: {}", e);
                        }
                    }

                    tracing::info!("Sending {} enriched validators to UI", enriched.len());
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
            staked_balance: staked,
            unbonding_balance: unbonding,
            is_nominating: !noms.is_empty(),
            nominations: noms.clone(),
            pool_id,
            pool_pending_rewards,
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
                pool_points: None,
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
        let mut result = self.fetch_account_once(network, &address).await;
        if let Err(ref e) = result
            && Self::is_connection_error(e)
            && self.try_reconnect().await
        {
            result = self.fetch_account_once(network, &address).await;
        }
        let _ = reply.send(result);
    }

    async fn fetch_validators_once(&self, network: Network) -> Result<Vec<ValidatorInfo>, String> {
        let Some(ref client) = self.client else {
            return Err("Not connected".to_string());
        };
        match client.get_validators().await {
            Ok(validators) => {
                tracing::info!("Fetched {} raw validators, enriching...", validators.len());
                let enriched =
                    enrich_validators(client, &validators, self.people_client.as_ref()).await;

                // Persist to DB
                if let Some(ref db) = self.db {
                    let era = if let Ok(Some(meta)) = db.get_chain_metadata(network).await {
                        meta.current_era
                    } else {
                        0
                    };

                    if let Err(e) = db
                        .set_cached_validators(network, era, enriched.clone())
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

    async fn fetch_pools_once(&self, network: Network) -> Result<Vec<PoolInfo>, String> {
        let Some(ref client) = self.client else {
            return Err("Not connected".to_string());
        };
        match client.get_nomination_pools().await {
            Ok(pools) => {
                // Fetch pool metadata (names)
                let metadata_map: HashMap<u32, String> = match client.get_pool_metadata().await {
                    Ok(metadata) => {
                        tracing::info!("Fetched {} pool names", metadata.len());
                        metadata.into_iter().map(|m| (m.id, m.name)).collect()
                    }
                    Err(e) => {
                        tracing::warn!("Failed to fetch pool metadata: {}", e);
                        HashMap::new()
                    }
                };

                // Calculate network average APY for pool estimation
                let network_apy = self.calculate_network_apy(client).await;
                tracing::info!("Network average APY for pools: {:?}", network_apy);

                let enriched: Vec<PoolInfo> = pools
                    .iter()
                    .map(|p| {
                        // Pool APY = network APY * (1 - pool commission)
                        let pool_apy = network_apy.map(|apy| {
                            let commission = p.commission.unwrap_or(0.0);
                            let after_commission = apy * (1.0 - commission);
                            // Cap at realistic maximum
                            after_commission.min(MAX_REALISTIC_APY)
                        });

                        // Use metadata name if available, otherwise default
                        let name = metadata_map
                            .get(&p.id)
                            .cloned()
                            .unwrap_or_else(|| format!("Pool #{}", p.id));

                        PoolInfo {
                            id: p.id,
                            name,
                            state: match p.state {
                                ChainPoolState::Open => PoolState::Open,
                                ChainPoolState::Blocked => PoolState::Blocked,
                                ChainPoolState::Destroying => PoolState::Destroying,
                            },
                            member_count: p.member_count,
                            total_bonded: p.points,
                            commission: p.commission,
                            apy: pool_apy,
                        }
                    })
                    .collect();

                // Persist to DB
                if let Some(ref db) = self.db
                    && let Err(e) = db.set_cached_pools(network, enriched.clone()).await
                {
                    tracing::warn!("Failed to cache pools: {}", e);
                }

                Ok(enriched)
            }
            Err(e) => Err(e.to_string()),
        }
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

    // === Helper Methods ===

    /// Calculate network average APY based on era rewards and total staked.
    async fn calculate_network_apy(&self, client: &ChainClient) -> Option<f64> {
        // Get era info for duration
        let era = match client.get_active_era().await {
            Ok(Some(era)) => era,
            _ => {
                tracing::debug!("No active era for network APY calculation");
                return None;
            }
        };
        let query_era = era.index.saturating_sub(1);
        let era_duration_ms = era.duration_ms;

        // Get era reward
        let era_reward = match client.get_era_validator_reward(query_era).await {
            Ok(Some(reward)) => reward,
            _ => {
                tracing::debug!("No era reward for network APY calculation");
                return None;
            }
        };

        // Get total staked (direct point query, no iteration)
        let total_staked = match client.get_era_total_stake_direct(query_era).await {
            Ok(staked) => staked,
            Err(e) => {
                tracing::debug!("Failed to get era total staked for APY: {}", e);
                return None;
            }
        };
        if total_staked == 0 {
            tracing::debug!("Total staked is 0, cannot calculate APY");
            return None;
        }

        let apy = get_era_apy(era_reward, total_staked, era_duration_ms);
        tracing::debug!(
            "Network APY: {:.2}% (reward={}, staked={}, era_duration={}ms)",
            apy * 100.0,
            era_reward,
            total_staked,
            era_duration_ms
        );

        // Sanity check
        if apy > MAX_REALISTIC_APY {
            tracing::warn!(
                "Network APY {:.2}% exceeds realistic maximum, data may be incomplete",
                apy * 100.0
            );
            return None;
        }

        Some(apy)
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
                .map(|p| make_transaction_payload(p, signer))
                .map_err(|e| format!("Failed to create bond payload: {}", e))
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
                .map(|p| make_transaction_payload(p, signer))
                .map_err(|e| format!("Failed to create unbond payload: {}", e))
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
                .map(|p| make_transaction_payload(p, signer))
                .map_err(|e| format!("Failed to create bond_extra payload: {}", e))
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
                .map(|p| make_transaction_payload(p, signer))
                .map_err(|e| format!("Failed to create rebond payload: {}", e))
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
                .map(|p| make_transaction_payload(p, signer))
                .map_err(|e| format!("Failed to create set_payee payload: {}", e))
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
            // Use 0 slashing spans as default
            client
                .create_withdraw_unbonded_payload(&signer, 0, true)
                .await
                .map(|p| make_transaction_payload(p, signer))
                .map_err(|e| format!("Failed to create withdraw_unbonded payload: {}", e))
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
                .map(|p| make_transaction_payload(p, signer))
                .map_err(|e| format!("Failed to create chill payload: {}", e))
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
                .map(|p| make_transaction_payload(p, signer))
                .map_err(|e| format!("Failed to create nominate payload: {}", e))
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
                .map(|p| make_transaction_payload(p, signer))
                .map_err(|e| format!("Failed to create pool_join payload: {}", e))
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
                .map(|p| make_transaction_payload(p, signer))
                .map_err(|e| format!("Failed to create pool_bond_extra payload: {}", e))
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
                .map(|p| make_transaction_payload(p, signer))
                .map_err(|e| format!("Failed to create pool_claim payload: {}", e))
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
                .map(|p| make_transaction_payload(p, signer))
                .map_err(|e| format!("Failed to create pool_unbond payload: {}", e))
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
            // For pool withdraw, member_account is the same as signer, 0 slashing spans
            client
                .create_pool_withdraw_payload(&signer, &signer, 0, true)
                .await
                .map(|p| make_transaction_payload(p, signer))
                .map_err(|e| format!("Failed to create pool_withdraw payload: {}", e))
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

    async fn handle_fetch_history(
        &mut self,
        network: Network,
        address: String,
        num_eras: u32,
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
            "Loading staking history for {} ({} eras)",
            address,
            num_eras
        );

        // Parse address to AccountId32 for chain queries
        let account: AccountId32 = match address.parse() {
            Ok(a) => a,
            Err(e) => {
                let _ = reply.send(Err(format!("Invalid address: {}", e)));
                return;
            }
        };

        // Try to load cached history first
        let mut cached_history = Vec::new();
        if let Some(ref db) = self.db {
            match db
                .get_history(network, address.clone(), Some(num_eras))
                .await
            {
                Ok(history) if !history.is_empty() => {
                    tracing::info!("Loaded {} cached history points", history.len());
                    cached_history = history;
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
                let _ = reply.send(Ok(cached_history));
                return;
            }
            Err(e) => {
                tracing::error!("Failed to get active era: {}", e);
                let _ = reply.send(Ok(cached_history));
                return;
            }
        };
        let current_era = current_era_info.index;
        let era_duration_ms = current_era_info.duration_ms;

        // Get user's bonded amount for APY calculation
        let user_bonded = match client.get_staking_ledger(&account).await {
            Ok(Some(ledger)) => ledger.active,
            _ => 0,
        };

        // Determine which eras we need to fetch
        let start_era = current_era.saturating_sub(num_eras);

        // Filter out cached entries with unrealistic APY (likely bad data that should be re-fetched)
        let (good_cached, bad_cached): (Vec<_>, Vec<_>) = cached_history
            .into_iter()
            .partition(|h| is_realistic_apy(h.apy));

        if !bad_cached.is_empty() {
            tracing::info!(
                "Filtering {} cached entries with unrealistic APY, will re-fetch",
                bad_cached.len()
            );
        }
        cached_history = good_cached;

        let cached_eras: std::collections::HashSet<u32> =
            cached_history.iter().map(|h| h.era).collect();
        let eras_to_fetch: Vec<u32> = (start_era..current_era)
            .filter(|era| !cached_eras.contains(era))
            .collect();

        if eras_to_fetch.is_empty() {
            tracing::info!("All eras already cached");
            let _ = reply.send(Ok(cached_history));
            return;
        }

        tracing::info!("Fetching {} missing eras from chain", eras_to_fetch.len());
        let mut new_points = Vec::new();

        // Fetch missing eras
        for era in eras_to_fetch {
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

            // Calculate network-wide APY
            let apy = stkopt_core::get_era_apy(era_reward, total_staked, era_duration_ms);

            // Log raw values for debugging
            tracing::debug!(
                "Era {} raw data: era_reward={}, total_staked={}, user_bonded={}, apy={:.4}",
                era,
                era_reward,
                total_staked,
                user_bonded,
                apy
            );

            // Skip eras with unrealistic APY (likely corrupted data)
            if !is_realistic_apy(apy) {
                tracing::warn!(
                    "Era {} has unrealistic APY {:.2}% (reward={}, staked={}), skipping",
                    era,
                    apy * 100.0,
                    era_reward,
                    total_staked
                );
                continue;
            }

            // Estimate user's reward (capped to avoid unrealistic values)
            let user_reward = estimate_user_reward(era_reward, user_bonded, total_staked);

            let point = HistoryPoint {
                era,
                date: None,
                bonded: user_bonded,
                reward: user_reward,
                apy,
            };

            new_points.push(point);
            tracing::debug!(
                "Added history point for era {} (APY: {:.2}%)",
                era,
                apy * 100.0
            );
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
                    eras,
                    reply,
                } => {
                    worker
                        .handle_fetch_history(current_network, address, eras, reply)
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
            staked_balance: Some(500),
            unbonding_balance: 200,
            is_nominating: true,
            nominations: vec!["validator1".to_string()],
            pool_id: None,
            pool_pending_rewards: 0,
        };
        assert_eq!(data.free_balance, 1000);
        assert_eq!(data.unbonding_balance, 200);
        assert!(data.is_nominating);
    }

    #[test]
    fn test_chain_update_variants() {
        let update = ChainUpdate::ConnectionStatus(ConnectionStatus::Connected);
        assert!(matches!(
            update,
            ChainUpdate::ConnectionStatus(ConnectionStatus::Connected)
        ));
    }
}
