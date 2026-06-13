//! Chain client abstraction using subxt.
//!
//! Supports both light client (smoldot) and WebSocket RPC connections.
//!
//! # Connection Modes
//!
//! - **LightClient** (default): Trustless P2P connection using smoldot.
//!   Does not require trusting any RPC provider. Cannot query historical state.
//!
//! - **Rpc**: Traditional WebSocket RPC connection. Used only when explicitly
//!   requested (e.g. `--rpc`). Required for historical data queries.
//!
//! # Architecture (Polkadot 2.0, Nov 2025+)
//!
//! - Asset Hub: All staking data (validators, pools, nominations) and staking
//!   transaction submission.
//! - Relay Chain: Block/session data only; not used for staking transactions.
//! - People Chain: Identity data

use crate::PeopleChainClient;
use crate::config::get_asset_hub_endpoints;
use crate::error::ChainError;
use crate::lightclient::LightClientConnections;
use std::sync::Arc;
use stkopt_core::{ConnectionStatus, Network};

use subxt::backend::CombinedBackend;
use subxt::tx::TransactionStatus;
use subxt::{OnlineClient, PolkadotConfig};
use tokio::sync::mpsc;

/// Connection mode for the chain client.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ConnectionMode {
    /// Light client (smoldot) - trustless P2P connection.
    /// This is the default and preferred mode.
    /// Cannot query historical state beyond current block.
    #[default]
    LightClient,
    /// RPC connection via WebSocket.
    /// Required for historical data queries.
    /// Use when light client is unavailable or for fallback.
    Rpc,
}

impl std::fmt::Display for ConnectionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionMode::LightClient => write!(f, "Light Client"),
            ConnectionMode::Rpc => write!(f, "RPC"),
        }
    }
}

/// RPC endpoint configuration for all chain types.
#[derive(Debug, Clone, Default)]
pub struct RpcEndpoints {
    /// Asset Hub RPC endpoint (for staking data).
    pub asset_hub: Option<String>,
    /// Relay chain RPC endpoint (for staking transactions).
    pub relay: Option<String>,
    /// People chain RPC endpoint (for identity data).
    pub people: Option<String>,
}

/// Connection configuration.
#[derive(Debug, Clone, Default)]
pub struct ConnectionConfig {
    /// Connection mode (LightClient or Rpc).
    pub mode: ConnectionMode,
    /// RPC endpoints (used when mode is Rpc, or as fallback).
    pub rpc_endpoints: RpcEndpoints,
}

/// Chain metadata and validation info.
#[derive(Debug, Clone)]
pub struct ChainInfo {
    /// Chain name as reported by system_chain RPC.
    pub chain_name: String,
    /// Runtime spec name (e.g., "asset-hub-polkadot").
    pub spec_name: String,
    /// Runtime spec version.
    pub spec_version: u32,
    /// Transaction version.
    pub tx_version: u32,
}

/// Chain client for interacting with Polkadot-SDK chains.
/// Since Polkadot 2.0 (Nov 2025), connects to Asset Hub where the Staking pallet lives.
/// Both staking data queries and staking transactions go to Asset Hub.
///
/// Supports reconnection when the connection drops via `reconnect()`.
pub struct ChainClient {
    network: Network,
    /// Connection mode used.
    connection_mode: ConnectionMode,
    /// RPC endpoints configuration (for reconnection).
    rpc_endpoints: RpcEndpoints,
    /// Asset Hub client (for reading staking data).
    client: OnlineClient<PolkadotConfig>,
    /// Asset Hub backend (for batched storage reads).
    asset_hub_backend: Option<Arc<CombinedBackend<PolkadotConfig>>>,
    /// Relay chain client (for block/session data, kept for potential future use).
    relay_client: Option<OnlineClient<PolkadotConfig>>,
    /// Light client connections (stored for connecting to People chain later).
    light_client_conns: Option<std::sync::Arc<LightClientConnections>>,
    /// Status channel for connection updates.
    status_tx: mpsc::Sender<ConnectionStatus>,
}

impl ChainClient {
    /// Connect to a network using the specified configuration.
    ///
    /// Uses light client by default (trustless P2P). RPC is only used when
    /// explicitly requested via `ConnectionMode::Rpc`; there is no automatic
    /// fallback.
    pub async fn connect(
        network: Network,
        config: &ConnectionConfig,
        status_tx: mpsc::Sender<ConnectionStatus>,
    ) -> Result<Self, ChainError> {
        match config.mode {
            ConnectionMode::LightClient => {
                tracing::info!("Connection mode: Light Client (trustless P2P)");
                Self::connect_light_client(network, status_tx.clone()).await
            }
            ConnectionMode::Rpc => {
                tracing::info!("Connection mode: RPC (explicit mode)");
                Self::connect_rpc(network, &config.rpc_endpoints, status_tx).await
            }
        }
    }

    /// Connect using the light client (smoldot).
    ///
    /// This is the preferred connection method as it doesn't require
    /// trusting any RPC provider. The light client verifies all data
    /// cryptographically using P2P connections.
    ///
    /// # Limitations
    ///
    /// Light clients cannot query historical state beyond the current block.
    /// For historical data, use RPC mode.
    pub async fn connect_light_client(
        network: Network,
        status_tx: mpsc::Sender<ConnectionStatus>,
    ) -> Result<Self, ChainError> {
        let _ = status_tx.send(ConnectionStatus::Connecting).await;

        tracing::info!("Connecting to {} via light client (smoldot)...", network);

        // Connect using light client - this fetches chain specs and establishes
        // P2P connections to relay chain and Asset Hub
        let light_client_conns =
            std::sync::Arc::new(LightClientConnections::connect(network).await?);

        let _ = status_tx.send(ConnectionStatus::Connected).await;

        tracing::info!(
            "Light client connected to {} (Asset Hub + Relay Chain)",
            network
        );

        Ok(Self {
            network,
            connection_mode: ConnectionMode::LightClient,
            rpc_endpoints: RpcEndpoints::default(),
            client: light_client_conns.asset_hub.clone(),
            asset_hub_backend: Some(light_client_conns.asset_hub_backend.clone()),
            relay_client: Some(light_client_conns.relay.clone()),
            light_client_conns: Some(light_client_conns),
            status_tx,
        })
    }

    /// Connect to a network's Asset Hub using WebSocket RPC.
    /// Uses custom endpoints from RpcEndpoints if provided, otherwise uses defaults.
    ///
    /// Since Polkadot 2.0 (Nov 2025), staking data and staking transactions
    /// both live on Asset Hub.
    pub async fn connect_rpc(
        network: Network,
        rpc_endpoints: &RpcEndpoints,
        status_tx: mpsc::Sender<ConnectionStatus>,
    ) -> Result<Self, ChainError> {
        let _ = status_tx.send(ConnectionStatus::Connecting).await;

        // Build list of endpoints to try (Asset Hub by default)
        let default_endpoints = get_asset_hub_endpoints(network);
        let endpoints: Vec<&str> = if let Some(ref endpoint) = rpc_endpoints.asset_hub {
            vec![endpoint.as_str()]
        } else {
            default_endpoints.to_vec()
        };

        if endpoints.is_empty() {
            return Err(ChainError::Connection(
                "No RPC endpoints configured".to_string(),
            ));
        }

        let mut endpoint_errors: Vec<String> = Vec::new();
        let mut asset_hub_connection = None;

        for endpoint in endpoints {
            tracing::info!("Trying {} Asset Hub via {}", network, endpoint);

            match connect_asset_hub_endpoint_rpc(endpoint).await {
                Ok(connection) => {
                    tracing::info!("Connected to {} Asset Hub via {}", network, endpoint);
                    asset_hub_connection = Some(connection);
                    break;
                }
                Err(e) => {
                    tracing::warn!("Failed to connect to {}: {}", endpoint, e);
                    endpoint_errors.push(format!("{}: {}", endpoint, e));
                }
            }
        }

        let (client, asset_hub_backend) = asset_hub_connection.ok_or_else(|| {
            if endpoint_errors.is_empty() {
                ChainError::Connection("All endpoints failed".to_string())
            } else {
                ChainError::Connection(format!(
                    "All endpoints failed: {}",
                    endpoint_errors.join("; ")
                ))
            }
        })?;

        // Also connect to relay chain for transaction submission
        let relay_client = Self::connect_relay_chain(network, rpc_endpoints.relay.as_deref())
            .await
            .ok();
        if relay_client.is_some() {
            tracing::info!("Also connected to {} relay chain for transactions", network);
        } else {
            tracing::warn!(
                "Could not connect to relay chain - QR codes will use Asset Hub genesis"
            );
        }

        let _ = status_tx.send(ConnectionStatus::Connected).await;

        Ok(Self {
            network,
            connection_mode: ConnectionMode::Rpc,
            rpc_endpoints: rpc_endpoints.clone(),
            client,
            asset_hub_backend: Some(asset_hub_backend),
            relay_client,
            light_client_conns: None,
            status_tx,
        })
    }

    /// Connect to the relay chain (for transaction submission).
    async fn connect_relay_chain(
        network: Network,
        custom_endpoint: Option<&str>,
    ) -> Result<OnlineClient<PolkadotConfig>, ChainError> {
        use crate::config::get_rpc_endpoints;

        let default_endpoints = get_rpc_endpoints(network);
        let endpoints: Vec<&str> = if let Some(endpoint) = custom_endpoint {
            vec![endpoint]
        } else {
            default_endpoints.to_vec()
        };

        if endpoints.is_empty() {
            return Err(ChainError::Connection(
                "No relay chain endpoints".to_string(),
            ));
        }

        for endpoint in endpoints {
            tracing::debug!("Trying {} relay chain via {}", network, endpoint);

            if let Ok(client) = OnlineClient::<PolkadotConfig>::from_url(endpoint).await {
                tracing::debug!("Connected to {} relay chain via {}", network, endpoint);
                return Ok(client);
            }
        }

        Err(ChainError::Connection(
            "All relay chain endpoints failed".to_string(),
        ))
    }

    /// Get the connected network.
    pub fn network(&self) -> Network {
        self.network
    }

    /// Get the underlying subxt client (Asset Hub).
    pub fn client(&self) -> &OnlineClient<PolkadotConfig> {
        &self.client
    }

    pub(crate) async fn batch_fetch_account_storage_values(
        &self,
        pallet_name: &str,
        entry_name: &str,
        addresses: &[subxt::utils::AccountId32],
    ) -> Result<std::collections::HashMap<[u8; 32], subxt::dynamic::Value>, ChainError> {
        let backend = self.asset_hub_backend.as_ref().ok_or_else(|| {
            ChainError::InvalidData("Asset Hub batch backend unavailable".to_string())
        })?;
        crate::batch_storage::fetch_account_storage_values(
            &self.client,
            backend,
            pallet_name,
            entry_name,
            addresses,
        )
        .await
    }

    /// Get the relay chain client (for block/session data only).
    /// Falls back to Asset Hub client if relay chain is not connected.
    pub fn relay_client(&self) -> &OnlineClient<PolkadotConfig> {
        self.relay_client.as_ref().unwrap_or(&self.client)
    }

    /// Check if relay chain is connected.
    pub fn has_relay_connection(&self) -> bool {
        self.relay_client.is_some()
    }

    /// Get the connection mode used.
    pub fn connection_mode(&self) -> ConnectionMode {
        self.connection_mode
    }

    /// Check if using light client mode.
    pub fn is_light_client(&self) -> bool {
        self.connection_mode == ConnectionMode::LightClient
    }

    /// Get the genesis hash (Asset Hub).
    pub fn genesis_hash(&self) -> [u8; 32] {
        self.client.genesis_hash().0
    }

    /// Get the relay chain genesis hash (for transactions).
    /// Returns error if relay chain is not connected.
    #[allow(clippy::result_large_err)]
    pub fn relay_genesis_hash(&self) -> Result<[u8; 32], ChainError> {
        self.relay_client
            .as_ref()
            .map(|c| c.genesis_hash().0)
            .ok_or_else(|| ChainError::Connection("Relay chain not connected".to_string()))
    }

    /// Get the latest block number and hash to verify connection.
    pub async fn get_latest_block(&self) -> Result<(u32, [u8; 32]), ChainError> {
        let block = self.client.at_current_block().await?;
        let number = block.block_number() as u32;
        let hash: [u8; 32] = block.block_hash().0;
        Ok((number, hash))
    }

    /// Get chain info with metadata validation.
    pub async fn get_chain_info(&self) -> Result<ChainInfo, ChainError> {
        let block = self.client.at_current_block().await?;
        let spec_version = block.spec_version();
        let tx_version = block.transaction_version();

        // Build a human-friendly chain name based on network
        let chain_name = format!("{} Asset Hub", self.network);

        // Use a simple spec name based on expected chain
        let spec_name = match self.network {
            Network::Polkadot => "asset-hub-polkadot",
            Network::Kusama => "asset-hub-kusama",
            Network::Westend => "asset-hub-westend",
            Network::Paseo => "asset-hub-paseo",
        }
        .to_string();

        tracing::info!(
            "Connected to {} (version: {}, tx_version: {})",
            chain_name,
            spec_version,
            tx_version
        );

        Ok(ChainInfo {
            chain_name,
            spec_name,
            spec_version,
            tx_version,
        })
    }

    /// Attempt to reconnect to the chain.
    /// Returns a new ChainClient instance with a fresh connection.
    /// Call this when you detect persistent connection errors.
    pub async fn reconnect(&self) -> Result<Self, ChainError> {
        tracing::info!(
            "Attempting to reconnect to {} ({})...",
            self.network,
            self.connection_mode
        );
        let _ = self.status_tx.send(ConnectionStatus::Connecting).await;

        match self.connection_mode {
            ConnectionMode::LightClient => {
                Self::connect_light_client(self.network, self.status_tx.clone()).await
            }
            ConnectionMode::Rpc => {
                Self::connect_rpc(self.network, &self.rpc_endpoints, self.status_tx.clone()).await
            }
        }
    }

    /// Check if the connection appears healthy by trying to fetch latest block.
    pub async fn is_connected(&self) -> bool {
        self.get_latest_block().await.is_ok()
    }

    /// Submit a signed extrinsic to the network.
    ///
    /// Returns a stream of transaction status updates.
    pub async fn submit_signed_extrinsic(
        &self,
        encoded: &[u8],
    ) -> Result<TxSubmissionProgress, ChainError> {
        tracing::info!(
            "Submitting signed extrinsic ({} bytes) to {}",
            encoded.len(),
            self.network
        );
        tracing::debug!("Extrinsic hex: 0x{}", hex::encode(encoded));

        let tx = self.client.tx().await?.from_bytes(encoded.to_vec());
        let status_stream = tx.submit_and_watch().await?;

        // Calculate the extrinsic hash for logging
        let tx_hash = sp_crypto_hashing::blake2_256(encoded);
        tracing::info!("Transaction submitted, hash: 0x{}", hex::encode(tx_hash));

        Ok(TxSubmissionProgress {
            tx_hash,
            status_stream,
        })
    }

    /// Connect to People chain for identity queries.
    ///
    /// Uses light client if available (when main connection is via light client),
    /// otherwise falls back to RPC.
    pub async fn connect_people_chain(&self) -> Result<OnlineClient<PolkadotConfig>, ChainError> {
        self.connect_people_chain_client()
            .await
            .map(|client| client.online_client().clone())
    }

    /// Connect to People chain for identity queries, preserving the backend for batch reads.
    pub async fn connect_people_chain_client(&self) -> Result<PeopleChainClient, ChainError> {
        // If we have light client connections, use them for People chain too
        if let Some(ref lc_conns) = self.light_client_conns {
            tracing::info!(
                "Connecting to {} People chain via light client...",
                self.network
            );
            return lc_conns.connect_people_chain_client().await;
        }

        // Fall back to RPC
        tracing::info!("Connecting to {} People chain via RPC...", self.network);
        connect_people_chain_rpc_client(self.network, self.rpc_endpoints.people.as_deref()).await
    }
}

/// Progress of a submitted transaction.
pub struct TxSubmissionProgress {
    /// The transaction hash (blake2-256 of encoded extrinsic).
    pub tx_hash: [u8; 32],
    /// Stream of status updates.
    pub status_stream: subxt::tx::TransactionProgress<
        subxt::PolkadotConfig,
        subxt::client::OnlineClientAtBlockImpl<subxt::PolkadotConfig>,
    >,
}

impl TxSubmissionProgress {
    /// Get the transaction hash.
    pub fn tx_hash(&self) -> [u8; 32] {
        self.tx_hash
    }

    /// Wait for the transaction to be included in a block.
    /// Returns the block hash when included.
    pub async fn wait_for_in_block(mut self) -> Result<TxInBlockResult, ChainError> {
        while let Some(status) = self.status_stream.next().await {
            match status? {
                TransactionStatus::InBestBlock(tx_in_block) => {
                    let block_hash: [u8; 32] = tx_in_block.block_hash().0;
                    tracing::info!("Transaction in best block: 0x{}", hex::encode(block_hash));
                    return Ok(TxInBlockResult {
                        tx_hash: self.tx_hash,
                        block_hash,
                        finalized: false,
                    });
                }
                TransactionStatus::InFinalizedBlock(tx_in_block) => {
                    let block_hash: [u8; 32] = tx_in_block.block_hash().0;
                    tracing::info!(
                        "Transaction finalized in block: 0x{}",
                        hex::encode(block_hash)
                    );
                    return Ok(TxInBlockResult {
                        tx_hash: self.tx_hash,
                        block_hash,
                        finalized: true,
                    });
                }
                TransactionStatus::Dropped { message } => {
                    tracing::warn!("Transaction dropped: {}", message);
                    return Err(ChainError::InvalidData(format!(
                        "Transaction dropped: {}",
                        message
                    )));
                }
                TransactionStatus::Invalid { message } => {
                    tracing::error!("Transaction invalid: {}", message);
                    return Err(ChainError::InvalidData(format!(
                        "Transaction invalid: {}",
                        message
                    )));
                }
                TransactionStatus::Error { message } => {
                    tracing::error!("Transaction error: {}", message);
                    return Err(ChainError::InvalidData(format!(
                        "Transaction error: {}",
                        message
                    )));
                }
                TransactionStatus::Validated => {
                    tracing::debug!("Transaction validated");
                }
                TransactionStatus::Broadcasted => {
                    tracing::debug!("Transaction broadcasted");
                }
                TransactionStatus::NoLongerInBestBlock => {
                    tracing::debug!("Transaction no longer in best block, waiting...");
                }
            }
        }

        Err(ChainError::InvalidData(
            "Transaction stream ended unexpectedly".to_string(),
        ))
    }

    /// Wait for the transaction to be finalized.
    /// Returns the block hash when finalized.
    pub async fn wait_for_finalized(mut self) -> Result<TxInBlockResult, ChainError> {
        while let Some(status) = self.status_stream.next().await {
            match status? {
                TransactionStatus::InFinalizedBlock(tx_in_block) => {
                    let block_hash: [u8; 32] = tx_in_block.block_hash().0;
                    tracing::info!(
                        "Transaction finalized in block: 0x{}",
                        hex::encode(block_hash)
                    );
                    return Ok(TxInBlockResult {
                        tx_hash: self.tx_hash,
                        block_hash,
                        finalized: true,
                    });
                }
                TransactionStatus::Dropped { message } => {
                    tracing::warn!("Transaction dropped: {}", message);
                    return Err(ChainError::InvalidData(format!(
                        "Transaction dropped: {}",
                        message
                    )));
                }
                TransactionStatus::Invalid { message } => {
                    tracing::error!("Transaction invalid: {}", message);
                    return Err(ChainError::InvalidData(format!(
                        "Transaction invalid: {}",
                        message
                    )));
                }
                TransactionStatus::Error { message } => {
                    tracing::error!("Transaction error: {}", message);
                    return Err(ChainError::InvalidData(format!(
                        "Transaction error: {}",
                        message
                    )));
                }
                TransactionStatus::Validated
                | TransactionStatus::Broadcasted
                | TransactionStatus::InBestBlock(_)
                | TransactionStatus::NoLongerInBestBlock => {
                    // Continue waiting for finalization
                }
            }
        }

        Err(ChainError::InvalidData(
            "Transaction stream ended unexpectedly".to_string(),
        ))
    }
}

/// Result when a transaction is included in a block.
#[derive(Debug, Clone)]
pub struct TxInBlockResult {
    /// The transaction hash.
    pub tx_hash: [u8; 32],
    /// The block hash where the transaction was included.
    pub block_hash: [u8; 32],
    /// Whether the block is finalized.
    pub finalized: bool,
}

/// Connect to a network's People chain using WebSocket RPC.
/// Returns a subxt client that can be used with PeopleChainClient.
///
/// Prefer using `ChainClient::connect_people_chain()` which automatically
/// uses light client when available.
pub async fn connect_people_chain_rpc(
    network: Network,
    custom_endpoint: Option<&str>,
) -> Result<OnlineClient<PolkadotConfig>, ChainError> {
    connect_people_chain_rpc_client(network, custom_endpoint)
        .await
        .map(|client| client.online_client().clone())
}

async fn connect_asset_hub_endpoint_rpc(
    endpoint: &str,
) -> Result<
    (
        OnlineClient<PolkadotConfig>,
        Arc<CombinedBackend<PolkadotConfig>>,
    ),
    ChainError,
> {
    let rpc_client = subxt::rpcs::RpcClient::from_url(endpoint)
        .await
        .map_err(|e| {
            ChainError::Connection(format!("Failed to create Asset Hub RPC client: {e}"))
        })?;
    let backend = CombinedBackend::<PolkadotConfig>::builder()
        .build_with_background_driver(rpc_client)
        .await
        .map_err(|e| ChainError::Connection(format!("Failed to create Asset Hub backend: {e}")))?;
    let backend = Arc::new(backend);
    let client = OnlineClient::<PolkadotConfig>::from_backend(backend.clone())
        .await
        .map_err(|e| ChainError::Connection(format!("Failed to create Asset Hub client: {e}")))?;
    Ok((client, backend))
}

/// Connect to a network's People chain using WebSocket RPC.
/// Returns a People chain client that can batch identity storage queries.
pub async fn connect_people_chain_rpc_client(
    network: Network,
    custom_endpoint: Option<&str>,
) -> Result<PeopleChainClient, ChainError> {
    use crate::config::get_people_chain_endpoints;

    let default_endpoints = get_people_chain_endpoints(network);
    let endpoints: Vec<&str> = if let Some(endpoint) = custom_endpoint {
        vec![endpoint]
    } else {
        default_endpoints.to_vec()
    };

    if endpoints.is_empty() {
        return Err(ChainError::Connection(
            "No People chain endpoints configured".to_string(),
        ));
    }

    let mut endpoint_errors: Vec<String> = Vec::new();
    for endpoint in endpoints {
        tracing::info!("Trying {} People chain RPC via {}", network, endpoint);

        match connect_people_endpoint_rpc(endpoint).await {
            Ok(client) => {
                tracing::info!("Connected to {} People chain via RPC {}", network, endpoint);
                return Ok(client);
            }
            Err(e) => {
                tracing::warn!("Failed to connect to People chain RPC {}: {}", endpoint, e);
                endpoint_errors.push(format!("{}: {}", endpoint, e));
            }
        }
    }

    if endpoint_errors.is_empty() {
        Err(ChainError::Connection(
            "All People chain RPC endpoints failed".to_string(),
        ))
    } else {
        Err(ChainError::Connection(format!(
            "All People chain RPC endpoints failed: {}",
            endpoint_errors.join("; ")
        )))
    }
}

async fn connect_people_endpoint_rpc(endpoint: &str) -> Result<PeopleChainClient, ChainError> {
    let rpc_client = subxt::rpcs::RpcClient::from_url(endpoint)
        .await
        .map_err(|e| ChainError::Connection(format!("Failed to create People RPC client: {e}")))?;
    let backend = CombinedBackend::<PolkadotConfig>::builder()
        .build_with_background_driver(rpc_client)
        .await
        .map_err(|e| ChainError::Connection(format!("Failed to create People backend: {}", e)))?;
    let backend = Arc::new(backend);
    let client = OnlineClient::<PolkadotConfig>::from_backend(backend.clone())
        .await
        .map_err(|e| ChainError::Connection(format!("Failed to create People client: {e}")))?;
    Ok(PeopleChainClient::with_backend(client, backend))
}
