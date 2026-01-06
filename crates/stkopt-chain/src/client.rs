//! Chain client abstraction using subxt.
//!
//! Supports WebSocket RPC connections to Polkadot-SDK chains.
//!
//! Since the Polkadot 2.0 migration (Nov 2025), staking data is on Asset Hub.
//! This client connects to Asset Hub by default for all staking operations.

use crate::config::get_asset_hub_endpoints;
use crate::error::ChainError;
use stkopt_core::{ConnectionStatus, Network};

use subxt::backend::rpc::RpcClient;
use subxt::{OnlineClient, PolkadotConfig};
use tokio::sync::mpsc;

/// Chain client for interacting with Polkadot-SDK chains.
/// Connects to Asset Hub where staking data lives since Polkadot 2.0.
pub struct ChainClient {
    network: Network,
    client: OnlineClient<PolkadotConfig>,
}

impl ChainClient {
    /// Connect to a network's Asset Hub using WebSocket RPC.
    /// If custom_endpoint is provided, uses that; otherwise tries default Asset Hub endpoints.
    ///
    /// Since Polkadot 2.0 (Nov 2025), all staking data lives on Asset Hub.
    pub async fn connect_rpc(
        network: Network,
        custom_endpoint: Option<&str>,
        status_tx: mpsc::UnboundedSender<ConnectionStatus>,
    ) -> Result<Self, ChainError> {
        let _ = status_tx.send(ConnectionStatus::Connecting);

        // Build list of endpoints to try (Asset Hub by default)
        let endpoints: Vec<&str> = if let Some(endpoint) = custom_endpoint {
            vec![endpoint]
        } else {
            get_asset_hub_endpoints(network).to_vec()
        };

        if endpoints.is_empty() {
            return Err(ChainError::Connection("No RPC endpoints configured".to_string()));
        }

        let mut last_error = None;
        for endpoint in endpoints {
            tracing::info!("Trying {} Asset Hub via {}", network, endpoint);

            match RpcClient::from_url(endpoint).await {
                Ok(rpc_client) => {
                    match OnlineClient::<PolkadotConfig>::from_rpc_client(rpc_client).await {
                        Ok(client) => {
                            let _ = status_tx.send(ConnectionStatus::Connected);
                            tracing::info!("Connected to {} Asset Hub via {}", network, endpoint);
                            return Ok(Self { network, client });
                        }
                        Err(e) => {
                            tracing::warn!("Failed to create client from {}: {}", endpoint, e);
                            last_error = Some(e.to_string());
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to connect to {}: {}", endpoint, e);
                    last_error = Some(e.to_string());
                }
            }
        }

        Err(ChainError::Connection(
            last_error.unwrap_or_else(|| "All endpoints failed".to_string()),
        ))
    }

    /// Get the connected network.
    pub fn network(&self) -> Network {
        self.network
    }

    /// Get the underlying subxt client.
    pub fn client(&self) -> &OnlineClient<PolkadotConfig> {
        &self.client
    }

    /// Get the genesis hash.
    pub fn genesis_hash(&self) -> [u8; 32] {
        self.client.genesis_hash().0
    }

    /// Get the latest block number and hash to verify connection.
    pub async fn get_latest_block(&self) -> Result<(u32, [u8; 32]), ChainError> {
        let block = self.client.blocks().at_latest().await?;
        let number = block.number();
        let hash: [u8; 32] = block.hash().0;
        Ok((number, hash))
    }
}
