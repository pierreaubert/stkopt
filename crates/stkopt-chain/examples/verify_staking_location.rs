//! Verify which chain has the Staking pallet.
//!
//! This example tests both RPC and light client connections to verify
//! whether the Staking pallet exists and is functional on each chain.
//!
//! Run with: cargo run -p stkopt-chain --example verify_staking_location
//! For light client test: cargo run -p stkopt-chain --example verify_staking_location -- --light-client

use std::env;
use subxt::dynamic::At;
use subxt::lightclient::LightClient;
use subxt::{OnlineClient, PolkadotConfig};

const KUSAMA_RELAY: &str = "wss://rpc.ibp.network/kusama";
const KUSAMA_ASSET_HUB: &str = "wss://kusama-asset-hub-rpc.polkadot.io";

// Embedded chain specs (same as in lightclient.rs)
const KUSAMA_SPEC: &str = include_str!("../chain_specs/kusama.json");
const KUSAMA_ASSET_HUB_SPEC: &str = include_str!("../chain_specs/kusama_asset_hub.json");

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let use_light_client = args.iter().any(|a| a == "--light-client");

    println!("===========================================");
    println!("Verifying Staking Pallet Location");
    println!("Mode: {}", if use_light_client { "Light Client" } else { "RPC" });
    println!("===========================================\n");

    if use_light_client {
        test_light_client().await?;
    } else {
        test_rpc().await?;
    }

    Ok(())
}

async fn test_rpc() -> Result<(), Box<dyn std::error::Error>> {
    // Test Kusama Relay Chain
    println!("--- Kusama Relay Chain (RPC) ---");
    println!("Connecting to {}...", KUSAMA_RELAY);
    let relay_client = OnlineClient::<PolkadotConfig>::from_url(KUSAMA_RELAY).await?;
    println!("Connected!");
    println!("Latest block: {:?}", relay_client.blocks().at_latest().await?.number());

    let relay_has_staking = check_staking_pallet(&relay_client);
    println!(
        "Staking pallet present: {}\n",
        if relay_has_staking { "YES ✓" } else { "NO ✗" }
    );

    if relay_has_staking {
        test_active_era(&relay_client).await;
    }

    // Test Kusama Asset Hub
    println!("\n--- Kusama Asset Hub (RPC) ---");
    println!("Connecting to {}...", KUSAMA_ASSET_HUB);
    let asset_hub_client = OnlineClient::<PolkadotConfig>::from_url(KUSAMA_ASSET_HUB).await?;
    println!("Connected!");

    let asset_hub_has_staking = check_staking_pallet(&asset_hub_client);
    println!(
        "Staking pallet present: {}\n",
        if asset_hub_has_staking {
            "YES ✓"
        } else {
            "NO ✗"
        }
    );

    if asset_hub_has_staking {
        test_active_era(&asset_hub_client).await;
    }

    // Summary
    print_summary(relay_has_staking, asset_hub_has_staking);

    Ok(())
}

async fn test_light_client() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting light client (smoldot)...");
    println!("This may take 30-60 seconds for initial sync...\n");

    // Start the light client with relay chain
    println!("--- Kusama Relay Chain (Light Client) ---");
    let (light_client, relay_rpc) = LightClient::relay_chain(KUSAMA_SPEC)?;
    println!("Light client started, waiting for sync...");

    let relay_client = OnlineClient::<PolkadotConfig>::from_rpc_client(relay_rpc).await?;
    println!("Relay chain synced!");

    let relay_has_staking = check_staking_pallet(&relay_client);
    println!(
        "Staking pallet present: {}\n",
        if relay_has_staking { "YES ✓" } else { "NO ✗" }
    );

    if relay_has_staking {
        test_active_era(&relay_client).await;
    }

    // Add Asset Hub as parachain
    println!("\n--- Kusama Asset Hub (Light Client) ---");
    println!("Adding Asset Hub as parachain...");
    let asset_hub_rpc = light_client.parachain(KUSAMA_ASSET_HUB_SPEC)?;
    println!("Waiting for Asset Hub sync...");

    let asset_hub_client = OnlineClient::<PolkadotConfig>::from_rpc_client(asset_hub_rpc).await?;
    println!("Asset Hub synced!");

    let asset_hub_has_staking = check_staking_pallet(&asset_hub_client);
    println!(
        "Staking pallet present: {}\n",
        if asset_hub_has_staking {
            "YES ✓"
        } else {
            "NO ✗"
        }
    );

    if asset_hub_has_staking {
        test_active_era(&asset_hub_client).await;
    }

    // Summary
    print_summary(relay_has_staking, asset_hub_has_staking);

    Ok(())
}

fn print_summary(relay_has_staking: bool, asset_hub_has_staking: bool) {
    println!("\n===========================================");
    println!("SUMMARY");
    println!("===========================================");
    println!("Relay chain has Staking pallet: {}", relay_has_staking);
    println!("Asset Hub has Staking pallet:   {}", asset_hub_has_staking);

    if relay_has_staking && !asset_hub_has_staking {
        println!("\n✓ CONFIRMED: Staking pallet is on the RELAY CHAIN, not Asset Hub.");
        println!("  The codebase architecture is INCORRECT - it tries to query");
        println!("  staking data from Asset Hub where the pallet doesn't exist.");
    } else if asset_hub_has_staking {
        println!("\n✓ Staking pallet found on Asset Hub - architecture is correct.");
        println!("  Staking has migrated to Asset Hub as expected.");
    } else {
        println!("\n✗ ERROR: Staking pallet not found on either chain!");
    }
}

fn check_staking_pallet(client: &OnlineClient<PolkadotConfig>) -> bool {
    let metadata = client.metadata();

    // Show runtime version
    let runtime = client.runtime_version();
    println!(
        "Runtime: spec_version={}, tx_version={}",
        runtime.spec_version, runtime.transaction_version
    );

    // List all pallets for debugging
    println!("Pallets on this chain ({} total):", metadata.pallets().count());
    let pallets: Vec<_> = metadata.pallets().map(|p| p.name()).collect();
    for (i, name) in pallets.iter().enumerate() {
        if *name == "Staking" {
            println!("  {:2}. {} <-- FOUND", i, name);
        } else {
            println!("  {:2}. {}", i, name);
        }
    }

    metadata.pallet_by_name("Staking").is_some()
}

async fn test_active_era(client: &OnlineClient<PolkadotConfig>) {
    println!("\nTesting ActiveEra query...");

    let storage_query = subxt::dynamic::storage("Staking", "ActiveEra", ());
    let storage = match client.storage().at_latest().await {
        Ok(s) => s,
        Err(e) => {
            println!("  Failed to get storage: {}", e);
            return;
        }
    };

    match storage.fetch(&storage_query).await {
        Ok(Some(value)) => {
            let decoded = value.to_value().unwrap();
            if let Some(index) = decoded.at("index").and_then(|v: &subxt::dynamic::Value<u32>| v.as_u128()) {
                println!("  ActiveEra index: {} ✓", index);
            } else {
                println!("  Got value but couldn't decode era index");
            }
        }
        Ok(None) => {
            println!("  ActiveEra returned None");
        }
        Err(e) => {
            println!("  Failed to fetch ActiveEra: {}", e);
        }
    }
}
