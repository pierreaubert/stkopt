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
//! # Chain Specs
//!
//! Chain specs are bundled with the binary from substrate-connect.
//! Source: https://github.com/paritytech/substrate-connect/tree/main/packages/connect-known-chains/specs
//!
//! # Limitations
//!
//! Light clients cannot query historical state beyond what's in the current
//! block. For historical staking data, use the RPC fallback or indexer.

use crate::error::ChainError;
use stkopt_core::Network;
use subxt::lightclient::LightClient;
use subxt::{OnlineClient, PolkadotConfig};

// Embedded relay chain specs
const POLKADOT_SPEC: &str = include_str!("../chain_specs/polkadot.json");
const KUSAMA_SPEC: &str = include_str!("../chain_specs/kusama.json");
const WESTEND_SPEC: &str = include_str!("../chain_specs/westend.json");
const PASEO_SPEC: &str = include_str!("../chain_specs/paseo.json");

// Embedded Asset Hub chain specs
const POLKADOT_ASSET_HUB_SPEC: &str = include_str!("../chain_specs/polkadot_asset_hub.json");
const KUSAMA_ASSET_HUB_SPEC: &str = include_str!("../chain_specs/kusama_asset_hub.json");
const WESTEND_ASSET_HUB_SPEC: &str = include_str!("../chain_specs/westend_asset_hub.json");

// Embedded People chain specs
const POLKADOT_PEOPLE_SPEC: &str = include_str!("../chain_specs/polkadot_people.json");
const KUSAMA_PEOPLE_SPEC: &str = include_str!("../chain_specs/kusama_people.json");
const WESTEND_PEOPLE_SPEC: &str = include_str!("../chain_specs/westend_people.json");

/// Get the embedded relay chain spec for a network.
pub fn get_relay_chain_spec(network: Network) -> &'static str {
    match network {
        Network::Polkadot => POLKADOT_SPEC,
        Network::Kusama => KUSAMA_SPEC,
        Network::Westend => WESTEND_SPEC,
        Network::Paseo => PASEO_SPEC,
    }
}

/// Get the embedded Asset Hub chain spec for a network.
/// Returns None for networks without Asset Hub specs (e.g., Paseo).
pub fn get_asset_hub_chain_spec(network: Network) -> Option<&'static str> {
    match network {
        Network::Polkadot => Some(POLKADOT_ASSET_HUB_SPEC),
        Network::Kusama => Some(KUSAMA_ASSET_HUB_SPEC),
        Network::Westend => Some(WESTEND_ASSET_HUB_SPEC),
        Network::Paseo => None, // Paseo Asset Hub spec not available
    }
}

/// Get the embedded People chain spec for a network.
/// Returns None for networks without People chain specs (e.g., Paseo).
pub fn get_people_chain_spec(network: Network) -> Option<&'static str> {
    match network {
        Network::Polkadot => Some(POLKADOT_PEOPLE_SPEC),
        Network::Kusama => Some(KUSAMA_PEOPLE_SPEC),
        Network::Westend => Some(WESTEND_PEOPLE_SPEC),
        Network::Paseo => None, // Paseo People spec not available
    }
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
    /// Chain specs are bundled with the binary (no network fetch required).
    pub async fn connect(network: Network) -> Result<Self, ChainError> {
        let total_start = std::time::Instant::now();
        tracing::info!("Connecting to {} via light client (smoldot)...", network);
        tracing::info!("Light client provides trustless P2P connection - no RPC trust required");

        // Get embedded chain specs
        let relay_spec = get_relay_chain_spec(network);
        let asset_hub_spec = get_asset_hub_chain_spec(network).ok_or_else(|| {
            ChainError::LightClient(format!(
                "Asset Hub chain spec not available for {} - light client mode not supported",
                network
            ))
        })?;

        tracing::debug!(
            "Using embedded chain specs: relay={} bytes, asset_hub={} bytes",
            relay_spec.len(),
            asset_hub_spec.len()
        );

        // Create the light client for the relay chain
        tracing::info!(
            "Starting smoldot light client for {} relay chain...",
            network
        );
        tracing::info!("Smoldot will discover and connect to P2P network peers...");
        let start = std::time::Instant::now();
        let (light_client, relay_rpc) =
            LightClient::relay_chain(relay_spec).map_err(|e| {
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
            .parachain(asset_hub_spec)
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
    /// Returns an error if People chain spec is not available for this network.
    pub async fn connect_people_chain(&self) -> Result<OnlineClient<PolkadotConfig>, ChainError> {
        let total_start = std::time::Instant::now();

        let people_spec = get_people_chain_spec(self.network).ok_or_else(|| {
            ChainError::LightClient(format!(
                "People chain spec not available for {} - identity lookup not supported in light client mode",
                self.network
            ))
        })?;

        tracing::debug!("Using embedded People chain spec: {} bytes", people_spec.len());

        tracing::info!(
            "Adding {} People chain as parachain to light client...",
            self.network
        );
        let start = std::time::Instant::now();
        let people_rpc = self
            .light_client
            .parachain(people_spec)
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
