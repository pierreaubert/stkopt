//! Network configuration for chain connections.
//!
//! Since the Polkadot 2.0 migration (Nov 2025), staking data has moved to Asset Hub.
//! - Relay chain endpoints: For block/session data only
//! - Asset Hub endpoints: For all staking data (validators, pools, nominations)
//! - People chain endpoints: For identity data

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

/// Get the People chain RPC endpoints for a network.
/// This is where identity data lives.
pub fn get_people_chain_endpoints(network: Network) -> &'static [&'static str] {
    match network {
        Network::Polkadot => &[
            "wss://polkadot-people-rpc.polkadot.io",
            "wss://sys.ibp.network/people-polkadot",
            "wss://people-polkadot.dotters.network",
            "wss://rpc-people-polkadot.luckyfriday.io",
        ],
        Network::Kusama => &[
            "wss://kusama-people-rpc.polkadot.io",
            "wss://sys.ibp.network/people-kusama",
            "wss://people-kusama.dotters.network",
            "wss://rpc-people-kusama.luckyfriday.io",
        ],
        Network::Westend => &[
            "wss://westend-people-rpc.polkadot.io",
            "wss://sys.ibp.network/people-westend",
            "wss://people-westend.dotters.network",
        ],
        Network::Paseo => &[
            "wss://sys.ibp.network/people-paseo",
            "wss://people-paseo.dotters.network",
        ],
    }
}

/// Indexer URL for staking history data.
/// Light clients don't have historical state, so we use an indexer.
pub fn get_staking_indexer_url(network: Network) -> &'static str {
    match network {
        Network::Polkadot => "https://staking-eras.usepapi.app/dot",
        Network::Kusama => "https://staking-eras.usepapi.app/ksm",
        Network::Westend => "https://staking-eras.usepapi.app/wnd",
        Network::Paseo => "https://staking-eras.usepapi.app/pas",
    }
}
