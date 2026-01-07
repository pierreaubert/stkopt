//! Light client connection support using smoldot.
//!
//! Provides trustless connections to Polkadot-SDK chains without relying on
//! centralized RPC endpoints. The light client verifies all data cryptographically.
//!
//! # Architecture
//!
//! The light client connects to:
//! 1. Relay chain (Polkadot/Kusama) - as the main chain
//! 2. Asset Hub - as a parachain for staking data
//! 3. People chain - as a parachain for identity data
//!
//! # Limitations
//!
//! Light clients cannot query historical state beyond what's in the current
//! block. For historical staking data, use the RPC fallback or indexer.

use crate::error::ChainError;
use stkopt_core::Network;
use subxt::lightclient::LightClient;
use subxt::{OnlineClient, PolkadotConfig};

/// Get the relay chain spec URL for a network.
pub fn get_relay_chain_spec_url(network: Network) -> &'static str {
    match network {
        Network::Polkadot => {
            "https://raw.githubusercontent.com/paritytech/subxt/master/artifacts/demo_chain_specs/polkadot.json"
        }
        Network::Kusama => {
            "https://raw.githubusercontent.com/nickvntaele/chainspecs/main/kusama.json"
        }
        Network::Westend => {
            "https://raw.githubusercontent.com/nickvntaele/chainspecs/main/westend.json"
        }
        Network::Paseo => {
            "https://raw.githubusercontent.com/nickvntaele/chainspecs/main/paseo.json"
        }
    }
}

/// Get the Asset Hub chain spec URL for a network.
pub fn get_asset_hub_chain_spec_url(network: Network) -> &'static str {
    match network {
        Network::Polkadot => {
            "https://raw.githubusercontent.com/paritytech/subxt/master/artifacts/demo_chain_specs/polkadot_asset_hub.json"
        }
        Network::Kusama => {
            "https://raw.githubusercontent.com/nickvntaele/chainspecs/main/kusama_asset_hub.json"
        }
        Network::Westend => {
            "https://raw.githubusercontent.com/nickvntaele/chainspecs/main/westend_asset_hub.json"
        }
        Network::Paseo => {
            "https://raw.githubusercontent.com/nickvntaele/chainspecs/main/paseo_asset_hub.json"
        }
    }
}

/// Get the People chain spec URL for a network.
pub fn get_people_chain_spec_url(network: Network) -> &'static str {
    match network {
        Network::Polkadot => {
            "https://raw.githubusercontent.com/nickvntaele/chainspecs/main/polkadot_people.json"
        }
        Network::Kusama => {
            "https://raw.githubusercontent.com/nickvntaele/chainspecs/main/kusama_people.json"
        }
        Network::Westend => {
            "https://raw.githubusercontent.com/nickvntaele/chainspecs/main/westend_people.json"
        }
        Network::Paseo => {
            "https://raw.githubusercontent.com/nickvntaele/chainspecs/main/paseo_people.json"
        }
    }
}

/// Fetch a chain spec from a URL.
async fn fetch_chain_spec(url: &str) -> Result<String, ChainError> {
    tracing::debug!("Fetching chain spec from {}", url);

    let start = std::time::Instant::now();
    let response = reqwest::get(url)
        .await
        .map_err(|e| ChainError::LightClient(format!("Failed to fetch chain spec: {}", e)))?;

    if !response.status().is_success() {
        return Err(ChainError::LightClient(format!(
            "Failed to fetch chain spec from {}: HTTP {}",
            url,
            response.status()
        )));
    }

    let spec = response
        .text()
        .await
        .map_err(|e| ChainError::LightClient(format!("Failed to read chain spec: {}", e)))?;

    tracing::debug!(
        "Fetched chain spec ({} bytes) in {:?}",
        spec.len(),
        start.elapsed()
    );

    Ok(spec)
}

/// Light client connections for a network.
///
/// Holds the light client instance and provides access to relay chain,
/// Asset Hub, and People chain connections.
pub struct LightClientConnections {
    /// The underlying smoldot light client.
    light_client: LightClient,
    /// Asset Hub subxt client.
    pub asset_hub: OnlineClient<PolkadotConfig>,
    /// Relay chain subxt client.
    pub relay: OnlineClient<PolkadotConfig>,
    /// Network this connection is for.
    network: Network,
}

impl LightClientConnections {
    /// Connect to a network using the light client.
    ///
    /// This establishes trustless P2P connections to:
    /// - The relay chain (Polkadot, Kusama, etc.)
    /// - Asset Hub parachain (for staking data)
    ///
    /// Chain specs are fetched from well-known URLs.
    pub async fn connect(network: Network) -> Result<Self, ChainError> {
        let total_start = std::time::Instant::now();
        tracing::info!("Connecting to {} via light client (smoldot)...", network);
        tracing::info!("Light client provides trustless P2P connection - no RPC trust required");

        // Fetch chain specs
        let relay_spec_url = get_relay_chain_spec_url(network);
        let asset_hub_spec_url = get_asset_hub_chain_spec_url(network);

        tracing::info!("Fetching {} relay chain spec from GitHub...", network);
        let relay_spec = fetch_chain_spec(relay_spec_url).await?;
        tracing::debug!("Relay chain spec: {} bytes", relay_spec.len());

        tracing::info!("Fetching {} Asset Hub chain spec from GitHub...", network);
        let asset_hub_spec = fetch_chain_spec(asset_hub_spec_url).await?;
        tracing::debug!("Asset Hub chain spec: {} bytes", asset_hub_spec.len());

        // Create the light client for the relay chain
        tracing::info!(
            "Starting smoldot light client for {} relay chain...",
            network
        );
        tracing::info!("Smoldot will discover and connect to P2P network peers...");
        let start = std::time::Instant::now();
        let (light_client, relay_rpc) =
            LightClient::relay_chain(relay_spec.as_str()).map_err(|e| {
                ChainError::LightClient(format!("Failed to start relay chain light client: {}", e))
            })?;
        tracing::debug!("Relay chain light client started in {:?}", start.elapsed());

        // Connect to Asset Hub as a parachain
        tracing::info!(
            "Adding {} Asset Hub as parachain to light client...",
            network
        );
        let start = std::time::Instant::now();
        let asset_hub_rpc = light_client
            .parachain(asset_hub_spec.as_str())
            .map_err(|e| {
                ChainError::LightClient(format!("Failed to connect to Asset Hub: {}", e))
            })?;
        tracing::debug!("Asset Hub parachain added in {:?}", start.elapsed());

        // Create subxt clients from the light client RPCs
        // This is where the actual chain sync happens - waiting for finalized blocks
        tracing::info!("Waiting for relay chain to sync and finalize blocks...");
        tracing::info!("This may take 15-30 seconds on first connection...");
        let start = std::time::Instant::now();
        let relay = OnlineClient::<PolkadotConfig>::from_rpc_client(relay_rpc)
            .await
            .map_err(|e| {
                ChainError::LightClient(format!("Failed to create relay client: {}", e))
            })?;
        tracing::info!("Relay chain synced in {:?}", start.elapsed());

        tracing::info!("Waiting for Asset Hub to sync...");
        let start = std::time::Instant::now();
        let asset_hub = OnlineClient::<PolkadotConfig>::from_rpc_client(asset_hub_rpc)
            .await
            .map_err(|e| {
                ChainError::LightClient(format!("Failed to create Asset Hub client: {}", e))
            })?;
        tracing::info!("Asset Hub synced in {:?}", start.elapsed());

        tracing::info!(
            "Light client connected to {} successfully! Total time: {:?}",
            network,
            total_start.elapsed()
        );

        Ok(Self {
            light_client,
            asset_hub,
            relay,
            network,
        })
    }

    /// Get the network this connection is for.
    pub fn network(&self) -> Network {
        self.network
    }

    /// Connect to the People chain for identity data.
    ///
    /// This is called separately since identity data is optional.
    pub async fn connect_people_chain(&self) -> Result<OnlineClient<PolkadotConfig>, ChainError> {
        let total_start = std::time::Instant::now();
        let people_spec_url = get_people_chain_spec_url(self.network);

        tracing::info!("Fetching {} People chain spec from GitHub...", self.network);
        let people_spec = fetch_chain_spec(people_spec_url).await?;
        tracing::debug!("People chain spec: {} bytes", people_spec.len());

        tracing::info!(
            "Adding {} People chain as parachain to light client...",
            self.network
        );
        let start = std::time::Instant::now();
        let people_rpc = self
            .light_client
            .parachain(people_spec.as_str())
            .map_err(|e| {
                ChainError::LightClient(format!("Failed to connect to People chain: {}", e))
            })?;
        tracing::debug!("People chain parachain added in {:?}", start.elapsed());

        tracing::info!("Waiting for People chain to sync...");
        let start = std::time::Instant::now();
        let client = OnlineClient::<PolkadotConfig>::from_rpc_client(people_rpc)
            .await
            .map_err(|e| {
                ChainError::LightClient(format!("Failed to create People chain client: {}", e))
            })?;
        tracing::info!("People chain synced in {:?}", start.elapsed());

        tracing::info!(
            "People chain connected! Total time: {:?}",
            total_start.elapsed()
        );

        Ok(client)
    }
}
