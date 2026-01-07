//! Chain client abstraction using subxt.
//!
//! Supports both light client (smoldot) and WebSocket RPC connections.
//!
//! # Connection Modes
//!
//! - **LightClient** (default): Trustless P2P connection using smoldot.
//!   Does not require trusting any RPC provider. Cannot query historical state.
//!
//! - **Rpc**: Traditional WebSocket RPC connection. Required for historical
//!   data queries (past eras). Used as fallback when light client fails.
//!
//! # Architecture (Polkadot 2.0, Nov 2025+)
//!
//! - Asset Hub: All staking data (validators, pools, nominations)
//! - Relay Chain: Transaction submission, block/session data
//! - People Chain: Identity data

use crate::config::get_asset_hub_endpoints;
use crate::error::ChainError;
use crate::lightclient::LightClientConnections;
use stkopt_core::{ConnectionStatus, Network};

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
    /// Whether the chain matches expected network.
    pub validated: bool,
    /// Validation message (empty if valid, error message if not).
    pub validation_message: String,
}

/// Chain client for interacting with Polkadot-SDK chains.
/// Connects to Asset Hub where staking data lives since Polkadot 2.0.
/// Note: Staking transactions still go to the relay chain, not Asset Hub.
pub struct ChainClient {
    network: Network,
    /// Connection mode used.
    connection_mode: ConnectionMode,
    /// Asset Hub client (for reading staking data).
    client: OnlineClient<PolkadotConfig>,
    /// Relay chain client (for submitting staking transactions).
    relay_client: Option<OnlineClient<PolkadotConfig>>,
}

impl ChainClient {
    /// Connect to a network using the specified configuration.
    ///
    /// Uses light client by default (trustless P2P), falling back to RPC
    /// if light client connection fails.
    pub async fn connect(
        network: Network,
        config: &ConnectionConfig,
        status_tx: mpsc::Sender<ConnectionStatus>,
    ) -> Result<Self, ChainError> {
        match config.mode {
            ConnectionMode::LightClient => {
                tracing::info!("Connection mode: Light Client (trustless P2P)");
                // Try light client first
                match Self::connect_light_client(network, status_tx.clone()).await {
                    Ok(client) => Ok(client),
                    Err(e) => {
                        tracing::warn!("Light client connection failed: {}", e);
                        tracing::info!("Falling back to RPC mode...");
                        // Fall back to RPC
                        Self::connect_rpc(network, &config.rpc_endpoints, status_tx).await
                    }
                }
            }
            ConnectionMode::Rpc => {
                tracing::info!("Connection mode: RPC (forced via --rpc flag)");
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
        let light_client_conns = LightClientConnections::connect(network).await?;

        let _ = status_tx.send(ConnectionStatus::Connected).await;

        tracing::info!(
            "Light client connected to {} (Asset Hub + Relay Chain)",
            network
        );

        Ok(Self {
            network,
            connection_mode: ConnectionMode::LightClient,
            client: light_client_conns.asset_hub,
            relay_client: Some(light_client_conns.relay),
        })
    }

    /// Connect to a network's Asset Hub using WebSocket RPC.
    /// Uses custom endpoints from RpcEndpoints if provided, otherwise uses defaults.
    ///
    /// Since Polkadot 2.0 (Nov 2025), staking data lives on Asset Hub, but
    /// staking transactions still go to the relay chain.
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

        let mut last_error = None;
        let mut asset_hub_client = None;

        for endpoint in endpoints {
            tracing::info!("Trying {} Asset Hub via {}", network, endpoint);

            match OnlineClient::<PolkadotConfig>::from_url(endpoint).await {
                Ok(client) => {
                    tracing::info!("Connected to {} Asset Hub via {}", network, endpoint);
                    asset_hub_client = Some(client);
                    break;
                }
                Err(e) => {
                    tracing::warn!("Failed to connect to {}: {}", endpoint, e);
                    last_error = Some(e.to_string());
                }
            }
        }

        let client = asset_hub_client.ok_or_else(|| {
            ChainError::Connection(
                last_error
                    .clone()
                    .unwrap_or_else(|| "All endpoints failed".to_string()),
            )
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
            client,
            relay_client,
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

    /// Get the relay chain client (for transactions).
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
    /// Falls back to Asset Hub genesis if relay chain is not connected.
    pub fn relay_genesis_hash(&self) -> [u8; 32] {
        self.relay_client
            .as_ref()
            .map(|c| c.genesis_hash().0)
            .unwrap_or_else(|| self.client.genesis_hash().0)
    }

    /// Get the latest block number and hash to verify connection.
    pub async fn get_latest_block(&self) -> Result<(u32, [u8; 32]), ChainError> {
        let block = self.client.blocks().at_latest().await?;
        let number = block.number();
        let hash: [u8; 32] = block.hash().0;
        Ok((number, hash))
    }

    /// Get chain info with metadata validation.
    pub fn get_chain_info(&self) -> ChainInfo {
        let runtime = self.client.runtime_version();
        let spec_version = runtime.spec_version;
        let tx_version = runtime.transaction_version;

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

        // Validate spec version - just a basic sanity check that we're on a modern runtime
        // Asset Hub spec versions are typically >= 1,000,000
        let validated = spec_version >= 1_000_000;

        let validation_message = if validated {
            String::new()
        } else {
            format!(
                "Warning: Unexpected spec_version {} for {} (expected >= 1000000)",
                spec_version, self.network
            )
        };

        if !validated {
            tracing::warn!(
                "Chain validation warning for {}: {}",
                self.network,
                validation_message
            );
        }

        tracing::info!(
            "Connected to {} (version: {}, tx_version: {})",
            chain_name,
            spec_version,
            tx_version
        );

        ChainInfo {
            chain_name,
            spec_name,
            spec_version,
            tx_version,
            validated,
            validation_message,
        }
    }
}

/// Connect to a network's People chain using WebSocket RPC.
/// Returns a subxt client that can be used with PeopleChainClient.
pub async fn connect_people_chain(
    network: Network,
    custom_endpoint: Option<&str>,
) -> Result<OnlineClient<PolkadotConfig>, ChainError> {
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

    let mut last_error = None;
    for endpoint in endpoints {
        tracing::info!("Trying {} People chain via {}", network, endpoint);

        match OnlineClient::<PolkadotConfig>::from_url(endpoint).await {
            Ok(client) => {
                tracing::info!("Connected to {} People chain via {}", network, endpoint);
                return Ok(client);
            }
            Err(e) => {
                tracing::warn!("Failed to connect to People chain {}: {}", endpoint, e);
                last_error = Some(e.to_string());
            }
        }
    }

    Err(ChainError::Connection(last_error.unwrap_or_else(|| {
        "All People chain endpoints failed".to_string()
    })))
}
