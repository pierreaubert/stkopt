//! Network configuration for chain connections.
//!
//! Since the Polkadot 2.0 migration (Nov 2025), staking data has moved to Asset Hub.
//! - Relay chain endpoints: For block/session data only
//! - Asset Hub endpoints: For all staking data (validators, pools, nominations)

use stkopt_core::Network;

/// Get the Asset Hub RPC endpoints for a network.
/// This is where staking data lives after the Polkadot 2.0 migration.
pub fn get_asset_hub_endpoints(network: Network) -> &'static [&'static str] {
    match network {
        Network::Polkadot => &[
            "wss://polkadot-asset-hub-rpc.polkadot.io",
            "wss://rpc-asset-hub-polkadot.luckyfriday.io",
            "wss://sys.ibp.network/asset-hub-polkadot",
            "wss://sys.dotters.network/asset-hub-polkadot",
            "wss://asset-hub-polkadot-rpc.dwellir.com",
        ],
        Network::Kusama => &[
            "wss://kusama-asset-hub-rpc.polkadot.io",
            "wss://rpc-asset-hub-kusama.luckyfriday.io",
            "wss://sys.ibp.network/asset-hub-kusama",
            "wss://sys.dotters.network/asset-hub-kusama",
            "wss://asset-hub-kusama-rpc.dwellir.com",
        ],
        Network::Westend => &[
            "wss://westend-asset-hub-rpc.polkadot.io",
            "wss://sys.ibp.network/asset-hub-westend",
            "wss://sys.dotters.network/asset-hub-westend",
            "wss://asset-hub-westend-rpc.dwellir.com",
        ],
        Network::Paseo => &[
            "wss://sys.ibp.network/asset-hub-paseo",
            "wss://sys.dotters.network/asset-hub-paseo",
            "wss://asset-hub-paseo-rpc.dwellir.com",
        ],
    }
}

/// Get the relay chain RPC endpoints for a network.
/// Use for block/session data, not for staking queries.
pub fn get_rpc_endpoints(network: Network) -> &'static [&'static str] {
    match network {
        Network::Polkadot => &[
            "wss://rpc.ibp.network/polkadot",
            "wss://polkadot.dotters.network",
            "wss://rpc-polkadot.luckyfriday.io",
            "wss://dot-rpc.stakeworld.io",
            "wss://polkadot-rpc.dwellir.com",
        ],
        Network::Kusama => &[
            "wss://rpc.ibp.network/kusama",
            "wss://kusama.dotters.network",
            "wss://rpc-kusama.luckyfriday.io",
            "wss://ksm-rpc.stakeworld.io",
            "wss://kusama-rpc.dwellir.com",
        ],
        Network::Westend => &[
            "wss://westend-rpc.polkadot.io",
            "wss://westend-rpc.dwellir.com",
            "wss://rpc.ibp.network/westend",
            "wss://westend.dotters.network",
        ],
        Network::Paseo => &[
            "wss://rpc.ibp.network/paseo",
            "wss://paseo.dotters.network",
            "wss://paseo-rpc.dwellir.com",
            "wss://pas-rpc.stakeworld.io",
        ],
    }
}
