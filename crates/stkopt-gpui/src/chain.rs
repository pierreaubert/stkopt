//! Real chain integration using stkopt-chain.
//!
//! This module provides the actual blockchain connection and data fetching
//! for the GPUI app, using the stkopt-chain crate.

use stkopt_chain::{
    ChainClient, ConnectionConfig, ConnectionMode as ChainConnectionMode, RpcEndpoints,
    PoolState as ChainPoolState,
};
use stkopt_core::{ConnectionStatus, Network};
use tokio::sync::{mpsc, oneshot};

use crate::app::{HistoryPoint, PoolInfo, PoolState, ValidatorInfo};
use crate::db::{CachedAccountStatus, CachedChainMetadata};
use crate::db_service::DbService;

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
}

/// Account data fetched from chain.
#[derive(Debug, Clone)]
pub struct AccountData {
    pub free_balance: u128,
    pub reserved_balance: u128,
    pub staked_balance: Option<u128>,
    pub is_nominating: bool,
    pub nominations: Vec<String>,
    pub pool_id: Option<u32>,
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
            .send(ChainCommand::Connect { network, use_light_client })
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
            .send(ChainCommand::FetchAccount { address, reply: reply_tx })
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
}

/// Chain worker state.
struct ChainWorker {
    client: Option<ChainClient>,
    update_tx: mpsc::Sender<ChainUpdate>,
    db: Option<DbService>,
}

impl ChainWorker {
    fn new(update_tx: mpsc::Sender<ChainUpdate>, db: Option<DbService>) -> Self {
        Self {
            client: None,
            update_tx,
            db,
        }
    }

    async fn handle_connect(&mut self, network: Network, use_light_client: bool) {
        // Send connecting status
        let _ = self.update_tx.send(ChainUpdate::ConnectionStatus(ConnectionStatus::Connecting)).await;

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
                let _ = self.update_tx.send(ChainUpdate::ConnectionStatus(ConnectionStatus::Connected)).await;
                
                // Fetch and persist chain metadata
                if let Some(ref client) = self.client {
                    let info = client.get_chain_info();
                    let genesis_hash = hex::encode(client.genesis_hash());
                    
                    // Fetch dynamic data
                    let era_duration_ms = client.get_era_duration_ms().await.unwrap_or(24 * 60 * 60 * 1000);
                    let current_era = client.get_active_era().await.ok().flatten().map(|e| e.index).unwrap_or(0);
                    
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

                // Auto-fetch validators after connection
                self.fetch_validators_internal(network).await;
            }
            Err(e) => {
                tracing::error!("Failed to connect: {}", e);
                let _ = self.update_tx.send(ChainUpdate::Error(e.to_string())).await;
                let _ = self.update_tx.send(ChainUpdate::ConnectionStatus(ConnectionStatus::Error(e.to_string()))).await;
            }
        }
    }

    async fn handle_disconnect(&mut self) {
        self.client = None;
        let _ = self.update_tx.send(ChainUpdate::ConnectionStatus(ConnectionStatus::Disconnected)).await;
    }

    async fn fetch_validators_internal(&mut self, network: Network) {
        if let Some(ref client) = self.client {
            match client.get_validators().await {
                Ok(validators) => {
                    tracing::info!("Fetched {} raw validators", validators.len());
                    
                    // Convert raw validators to enriched validators
                    let enriched: Vec<ValidatorInfo> = validators
                        .iter()
                        .map(|v| ValidatorInfo {
                            address: v.address.to_string(),
                            name: None, // TODO: Fetch identity
                            commission: v.preferences.commission,
                            blocked: v.preferences.blocked,
                            total_stake: 0, // TODO: Fetch from exposure
                            own_stake: 0,   // TODO: Fetch from exposure
                            nominator_count: 0, // TODO: Fetch from exposure
                            apy: None, // TODO: Calculate from rewards
                        })
                        .collect();
                    
                    // Persist to DB
                    if let Some(ref db) = self.db {
                        // Get current era from metadata or default to 0
                        let era = if let Ok(Some(meta)) = db.get_chain_metadata(network).await {
                            meta.current_era
                        } else {
                            0
                        };

                        if let Err(e) = db.set_cached_validators(network, era, enriched.clone()).await {
                            tracing::warn!("Failed to cache validators: {}", e);
                        }
                    }

                    tracing::info!("Converted to {} enriched validators", enriched.len());
                    let _ = self.update_tx.send(ChainUpdate::ValidatorsLoaded(enriched)).await;
                }
                Err(e) => {
                    tracing::error!("Failed to fetch validators: {}", e);
                    let _ = self.update_tx.send(ChainUpdate::Error(format!("Failed to fetch validators: {}", e))).await;
                }
            }
        }
    }

    async fn handle_fetch_account(&mut self, network: Network, address: String, reply: oneshot::Sender<Result<AccountData, String>>) {
        let result = if let Some(ref client) = self.client {
            match address.parse::<subxt::utils::AccountId32>() {
                Ok(account_id) => {
                    // Fetch balance
                    let balance = client.get_account_balance(&account_id).await;
                    let staking = client.get_staking_ledger(&account_id).await;
                    let nominations = client.get_nominations(&account_id).await;
                    let pool = client.get_pool_membership(&account_id).await;

                    match balance {
                        Ok(bal) => {
                            let staked = staking.ok().flatten().map(|s| s.active);
                            let noms: Vec<String> = nominations.ok().flatten().map(|n| {
                                n.targets.iter().map(|t| t.to_string()).collect()
                            }).unwrap_or_default();
                            let pool_id = pool.ok().flatten().map(|p| p.pool_id);

                            let account_data = AccountData {
                                free_balance: bal.free,
                                reserved_balance: bal.reserved,
                                staked_balance: staked,
                                is_nominating: !noms.is_empty(),
                                nominations: noms.clone(),
                                pool_id,
                            };

                            // Persist to DB
                            if let Some(ref db) = self.db {
                                let cached_status = CachedAccountStatus {
                                    free_balance: account_data.free_balance,
                                    reserved_balance: account_data.reserved_balance,
                                    frozen_balance: bal.frozen,
                                    staked_amount: account_data.staked_balance.unwrap_or(0),
                                    nominations_json: if noms.is_empty() { None } else { serde_json::to_string(&noms).ok() },
                                    pool_id: account_data.pool_id,
                                    pool_points: None, // Need to fetch pool points
                                };
                                if let Err(e) = db.set_cached_account_status(network, address.clone(), cached_status).await {
                                    tracing::warn!("Failed to cache account status: {}", e);
                                }
                            }

                            Ok(account_data)
                        }
                        Err(e) => Err(format!("Failed to fetch balance: {}", e)),
                    }
                }
                Err(e) => Err(format!("Invalid address: {}", e)),
            }
        } else {
            Err("Not connected".to_string())
        };

        let _ = reply.send(result);
    }

    async fn handle_fetch_validators(&mut self, network: Network, reply: oneshot::Sender<Result<Vec<ValidatorInfo>, String>>) {
        let result = if let Some(ref client) = self.client {
            match client.get_validators().await {
                Ok(validators) => {
                    let enriched: Vec<ValidatorInfo> = validators
                        .iter()
                        .map(|v| ValidatorInfo {
                            address: v.address.to_string(),
                            name: None,
                            commission: v.preferences.commission,
                            blocked: v.preferences.blocked,
                            total_stake: 0,
                            own_stake: 0,
                            nominator_count: 0,
                            apy: None,
                        })
                        .collect();
                    
                    // Persist to DB (logic duplicated from internal fetch, but that's okay for now)
                    if let Some(ref db) = self.db {
                        let era = if let Ok(Some(meta)) = db.get_chain_metadata(network).await {
                            meta.current_era
                        } else {
                            0
                        };

                        if let Err(e) = db.set_cached_validators(network, era, enriched.clone()).await {
                            tracing::warn!("Failed to cache validators: {}", e);
                        }
                    }

                    Ok(enriched)
                }
                Err(e) => Err(e.to_string()),
            }
        } else {
            Err("Not connected".to_string())
        };
        let _ = reply.send(result);
    }

    async fn handle_fetch_pools(&mut self, network: Network, reply: oneshot::Sender<Result<Vec<PoolInfo>, String>>) {
        let result = if let Some(ref client) = self.client {
            match client.get_nomination_pools().await {
                Ok(pools) => {
                    let enriched: Vec<PoolInfo> = pools
                        .iter()
                        .map(|p| PoolInfo {
                            id: p.id,
                            name: format!("Pool #{}", p.id),
                            state: match p.state {
                                ChainPoolState::Open => PoolState::Open,
                                ChainPoolState::Blocked => PoolState::Blocked,
                                ChainPoolState::Destroying => PoolState::Destroying,
                            },
                            member_count: p.member_count,
                            total_bonded: p.points,
                            commission: None, // TODO: Fetch commission
                        })
                        .collect();

                    // Persist to DB
                    if let Some(ref db) = self.db {
                        if let Err(e) = db.set_cached_pools(network, enriched.clone()).await {
                            tracing::warn!("Failed to cache pools: {}", e);
                        }
                    }

                    Ok(enriched)
                }
                Err(e) => Err(e.to_string()),
            }
        } else {
            Err("Not connected".to_string())
        };
        let _ = reply.send(result);
    }
}

/// Spawn the chain worker and return a handle.
pub fn spawn_chain_worker(db: Option<DbService>, handle: tokio::runtime::Handle) -> (ChainHandle, mpsc::Receiver<ChainUpdate>) {
    let (command_tx, mut command_rx) = mpsc::channel::<ChainCommand>(32);
    let (update_tx, update_rx) = mpsc::channel::<ChainUpdate>(32);

    handle.spawn(async move {
        let mut worker = ChainWorker::new(update_tx, db);
        let mut current_network = Network::Polkadot; // Track current network for DB operations

        while let Some(command) = command_rx.recv().await {
            match command {
                ChainCommand::Connect { network, use_light_client } => {
                    current_network = network;
                    worker.handle_connect(network, use_light_client).await;
                }
                ChainCommand::Disconnect => {
                    worker.handle_disconnect().await;
                }
                ChainCommand::FetchAccount { address, reply } => {
                    worker.handle_fetch_account(current_network, address, reply).await;
                }
                ChainCommand::FetchValidators { reply } => {
                    worker.handle_fetch_validators(current_network, reply).await;
                }
                ChainCommand::FetchPools { reply } => {
                    worker.handle_fetch_pools(current_network, reply).await;
                }
                ChainCommand::FetchHistory { address: _, eras: _, reply } => {
                    // History fetching is more complex, simplified for now
                    let _ = reply.send(Err("History fetching not yet implemented".to_string()));
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
            is_nominating: true,
            nominations: vec!["validator1".to_string()],
            pool_id: None,
        };
        assert_eq!(data.free_balance, 1000);
        assert!(data.is_nominating);
    }

    #[test]
    fn test_chain_update_variants() {
        let update = ChainUpdate::ConnectionStatus(ConnectionStatus::Connected);
        assert!(matches!(update, ChainUpdate::ConnectionStatus(ConnectionStatus::Connected)));
    }
}
