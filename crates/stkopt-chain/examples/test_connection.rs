//! Simple test to verify chain connection works.
//!
//! Run with: cargo run -p stkopt-chain --example test_connection -- [RPC_URL]
//!
//! Examples:
//!   cargo run -p stkopt-chain --example test_connection
//!   cargo run -p stkopt-chain --example test_connection -- wss://polkadot-asset-hub-rpc.polkadot.io

use stkopt_chain::{ChainClient, RpcEndpoints};
use stkopt_core::{ConnectionStatus, Network};
use tokio::sync::mpsc;

type DynamicStorageAddress =
    subxt::storage::DynamicAddress<Vec<subxt::dynamic::Value<()>>, subxt::dynamic::Value<()>>;

fn dynamic_storage(pallet: &str, entry: &str) -> DynamicStorageAddress {
    subxt::dynamic::storage(pallet, entry)
}

fn no_keys() -> Vec<subxt::dynamic::Value<()>> {
    Vec::new()
}

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::fmt().with_env_filter("debug").init();

    let args: Vec<String> = std::env::args().collect();
    let custom_url = args.get(1).map(|s| s.as_str());

    println!("Testing chain connection...");
    if let Some(url) = custom_url {
        println!("Using custom endpoint: {}", url);
    } else {
        println!("Using default Asset Hub endpoints");
    }

    // Create a dummy status channel
    let (status_tx, mut status_rx) = mpsc::channel::<ConnectionStatus>(10);

    // Spawn task to print status updates
    tokio::spawn(async move {
        while let Some(status) = status_rx.recv().await {
            println!("Status: {:?}", status);
        }
    });

    // Build RPC endpoints config
    let rpc_endpoints = RpcEndpoints {
        asset_hub: custom_url.map(|s| s.to_string()),
        relay: None,
        people: None,
    };

    // Try to connect
    match ChainClient::connect_rpc(Network::Polkadot, &rpc_endpoints, status_tx).await {
        Ok(client) => {
            println!("Connected successfully!");
            println!("Genesis hash: 0x{}", hex::encode(client.genesis_hash()));

            // Try to get the latest block
            match client.get_latest_block().await {
                Ok((number, hash)) => {
                    println!("Latest block: #{}", number);
                    println!("Block hash: 0x{}", hex::encode(hash));
                    println!("\n✓ Connection test PASSED\n");
                }
                Err(e) => {
                    println!("Failed to get latest block: {}", e);
                    println!("\n✗ Connection test FAILED");
                    return;
                }
            }

            // Debug: try raw storage query for ActiveEra
            println!("--- Debugging ActiveEra storage ---");
            let block = client.client().at_current_block().await.unwrap();
            let storage = block.storage();
            let metadata = block.metadata();

            // First, let's check if we can get runtime constants (to verify metadata works)
            println!("Checking runtime constants...");
            let sessions_per_era =
                subxt::dynamic::constant::<subxt::dynamic::Value<()>>("Staking", "SessionsPerEra");
            match block.constants().entry(&sessions_per_era) {
                Ok(val) => {
                    println!("SessionsPerEra constant: {:?}", val);
                }
                Err(e) => {
                    println!("Failed to get SessionsPerEra: {}", e);
                }
            }

            // Try to get validators count (a simple value)
            println!("\nTrying Staking.ValidatorCount...");
            let validator_count_query = dynamic_storage("Staking", "ValidatorCount");
            match storage.try_fetch(&validator_count_query, no_keys()).await {
                Ok(Some(value)) => {
                    let decoded: subxt::dynamic::Value<()> = value.decode().unwrap();
                    println!("ValidatorCount: {:?}", decoded);
                }
                Ok(None) => println!("ValidatorCount: None"),
                Err(e) => println!("ValidatorCount error: {}", e),
            }

            // Try using fetch, which applies the runtime default if present
            println!("\nTrying Staking.ValidatorCount with fetch_or_default...");
            match storage.fetch(&validator_count_query, no_keys()).await {
                Ok(value) => {
                    let decoded: subxt::dynamic::Value<()> = value.decode().unwrap();
                    println!("ValidatorCount (default): {:?}", decoded);
                }
                Err(e) => println!("ValidatorCount (default) error: {}", e),
            }

            // Try to get raw bytes
            println!("\nTrying raw storage fetch for ValidatorCount...");
            let storage_key = storage
                .entry(&validator_count_query)
                .and_then(|entry| entry.fetch_key(no_keys()))
                .unwrap();
            println!("Storage key: 0x{}", hex::encode(&storage_key));
            match storage.fetch_raw(storage_key).await {
                Ok(bytes) => {
                    println!("Raw bytes: 0x{}", hex::encode(&bytes));
                }
                Err(e) => println!("Raw fetch error: {}", e),
            }

            // Try ActiveEra raw
            println!("\nTrying raw storage fetch for ActiveEra...");
            let storage_query = dynamic_storage("Staking", "ActiveEra");
            let active_era_key = storage
                .entry(&storage_query)
                .and_then(|entry| entry.fetch_key(no_keys()))
                .unwrap();
            println!("ActiveEra key: 0x{}", hex::encode(&active_era_key));
            match storage.fetch_raw(active_era_key).await {
                Ok(bytes) => {
                    println!("Raw bytes: 0x{}", hex::encode(&bytes));
                }
                Err(e) => println!("Raw fetch error: {}", e),
            }

            // Try ActiveEra
            println!("\nTrying Staking.ActiveEra...");
            match storage.try_fetch(&storage_query, no_keys()).await {
                Ok(Some(value)) => {
                    println!("ActiveEra raw value found!");
                    let decoded: subxt::dynamic::Value<()> = value.decode().unwrap();
                    println!("Decoded value: {:?}", decoded);
                }
                Ok(None) => {
                    println!("ActiveEra returned None from fetch()");

                    // Try to list available storage entries
                    println!("\nTrying to fetch CurrentEra instead...");
                    let current_era_query = dynamic_storage("Staking", "CurrentEra");
                    match storage.try_fetch(&current_era_query, no_keys()).await {
                        Ok(Some(value)) => {
                            let decoded: subxt::dynamic::Value<()> = value.decode().unwrap();
                            println!("CurrentEra: {:?}", decoded);
                        }
                        Ok(None) => println!("CurrentEra also None"),
                        Err(e) => println!("CurrentEra error: {}", e),
                    }
                }
                Err(e) => {
                    println!("ActiveEra fetch error: {}", e);
                }
            }

            // Try System.Number to verify we can read storage at all
            println!("\nTrying System.Number (block number)...");
            let number_query = dynamic_storage("System", "Number");
            match storage.try_fetch(&number_query, no_keys()).await {
                Ok(Some(value)) => {
                    let decoded: subxt::dynamic::Value<()> = value.decode().unwrap();
                    println!("System.Number: {:?}", decoded);
                }
                Ok(None) => println!("System.Number: None"),
                Err(e) => println!("System.Number error: {}", e),
            }

            // List all pallets to see what's available
            println!("\n--- Checking available pallets ---");
            for pallet in metadata.pallets() {
                if pallet.name().to_lowercase().contains("stak") {
                    println!("Found pallet: {}", pallet.name());
                    // List storage entries for staking-related pallets
                    if let Some(storage) = pallet.storage() {
                        for entry in storage.entries() {
                            println!("  - {}", entry.name());
                        }
                    }
                }
            }

            // Try to get active era using the method
            println!("\n--- Using get_active_era() method ---");
            match client.get_active_era().await {
                Ok(Some(era)) => {
                    println!(
                        "Active era: {} ({:.1}% complete)",
                        era.index,
                        era.pct_complete * 100.0
                    );
                }
                Ok(None) => {
                    println!("Active era: None returned");
                }
                Err(e) => {
                    println!("Failed to get active era: {}", e);
                }
            }

            // Try iterating over Staking storage to see if anything exists
            println!("\n--- Iterating over some Staking storage ---");
            let bonded_iter = dynamic_storage("Staking", "Bonded");
            let iter = storage.iter(&bonded_iter, no_keys()).await;
            match iter {
                Ok(mut stream) => {
                    println!("Bonded iterator created");
                    let mut count = 0;
                    while let Some(item) = stream.next().await {
                        if count >= 3 {
                            break;
                        }
                        match item {
                            Ok(kv) => {
                                println!("  Bonded entry key: 0x{}", hex::encode(kv.key_bytes()));
                                let val: subxt::dynamic::Value<()> = kv.value().decode().unwrap();
                                println!("  Value: {:?}", val);
                            }
                            Err(e) => println!("  Error: {}", e),
                        }
                        count += 1;
                    }
                    if count == 0 {
                        println!("  No entries found!");
                    }
                }
                Err(e) => println!("Failed to create iterator: {}", e),
            }

            // Check StakingAhClient pallet
            println!("\n--- Checking StakingAhClient pallet ---");
            let mode_query = dynamic_storage("StakingAhClient", "Mode");
            match storage.try_fetch(&mode_query, no_keys()).await {
                Ok(Some(value)) => {
                    let decoded: subxt::dynamic::Value<()> = value.decode().unwrap();
                    println!("StakingAhClient.Mode: {:?}", decoded);
                }
                Ok(None) => println!("StakingAhClient.Mode: None"),
                Err(e) => println!("StakingAhClient.Mode error: {}", e),
            }

            // Check Session.CurrentIndex
            println!("\n--- Checking Session pallet ---");
            let session_idx = dynamic_storage("Session", "CurrentIndex");
            match storage.try_fetch(&session_idx, no_keys()).await {
                Ok(Some(value)) => {
                    let decoded: subxt::dynamic::Value<()> = value.decode().unwrap();
                    println!("Session.CurrentIndex: {:?}", decoded);
                }
                Ok(None) => println!("Session.CurrentIndex: None"),
                Err(e) => println!("Session.CurrentIndex error: {}", e),
            }

            // Check StakingAhClient.ValidatorSet
            println!("\n--- Checking StakingAhClient.ValidatorSet ---");
            let validator_set_query = dynamic_storage("StakingAhClient", "ValidatorSet");
            let iter = storage.iter(&validator_set_query, no_keys()).await;
            match iter {
                Ok(mut stream) => {
                    let mut count = 0;
                    while let Some(item) = stream.next().await {
                        if count >= 5 {
                            println!("  ... (showing first 5)");
                            break;
                        }
                        match item {
                            Ok(kv) => {
                                let val: subxt::dynamic::Value<()> = kv.value().decode().unwrap();
                                println!("  Validator: {:?}", val);
                            }
                            Err(e) => println!("  Error: {}", e),
                        }
                        count += 1;
                    }
                    if count == 0 {
                        println!("  No entries!");
                    }
                }
                Err(e) => println!("Failed to iterate: {}", e),
            }

            // Check Session.Validators for the actual validator set
            println!("\n--- Checking Session.Validators ---");
            let session_validators = dynamic_storage("Session", "Validators");
            match storage.try_fetch(&session_validators, no_keys()).await {
                Ok(Some(value)) => {
                    let decoded: subxt::dynamic::Value<()> = value.decode().unwrap();
                    println!(
                        "Session.Validators (first part): {:?}",
                        format!("{:?}", decoded)
                            .chars()
                            .take(500)
                            .collect::<String>()
                    );
                }
                Ok(None) => println!("Session.Validators: None"),
                Err(e) => println!("Session.Validators error: {}", e),
            }

            // Check available constants for era duration
            println!("\n--- Checking era duration constants ---");

            // Check if Aura exists
            let aura_slot =
                subxt::dynamic::constant::<subxt::dynamic::Value<()>>("Aura", "SlotDuration");
            match block.constants().entry(&aura_slot) {
                Ok(val) => {
                    println!("Aura.SlotDuration: {:?}", val);
                }
                Err(e) => println!("Aura.SlotDuration error: {}", e),
            }

            // Check ParachainSystem for block time
            let system_block_time =
                subxt::dynamic::constant::<subxt::dynamic::Value<()>>("System", "BlockTime");
            match block.constants().entry(&system_block_time) {
                Ok(val) => {
                    println!("System.BlockTime: {:?}", val);
                }
                Err(e) => println!("System.BlockTime error: {}", e),
            }

            // Check Staking.SessionsPerEra
            let sessions =
                subxt::dynamic::constant::<subxt::dynamic::Value<()>>("Staking", "SessionsPerEra");
            match block.constants().entry(&sessions) {
                Ok(val) => {
                    println!("Staking.SessionsPerEra: {:?}", val);
                }
                Err(e) => println!("Staking.SessionsPerEra error: {}", e),
            }

            // Check Staking.MaxEraDuration
            let max_era =
                subxt::dynamic::constant::<subxt::dynamic::Value<()>>("Staking", "MaxEraDuration");
            match block.constants().entry(&max_era) {
                Ok(val) => {
                    println!("Staking.MaxEraDuration: {:?}", val);
                }
                Err(e) => println!("Staking.MaxEraDuration error: {}", e),
            }

            // Check Session.Period if it exists
            let session_period =
                subxt::dynamic::constant::<subxt::dynamic::Value<()>>("Session", "Period");
            match block.constants().entry(&session_period) {
                Ok(val) => {
                    println!("Session.Period: {:?}", val);
                }
                Err(e) => println!("Session.Period error: {}", e),
            }

            // List all Aura constants
            println!("\n--- Listing Aura pallet ---");
            for pallet in metadata.pallets() {
                if pallet.name() == "Aura" {
                    println!("Aura constants:");
                    for constant in pallet.constants() {
                        println!("  - {}", constant.name());
                    }
                }
            }

            // List Staking constants
            println!("\n--- Listing Staking pallet constants ---");
            for pallet in metadata.pallets() {
                if pallet.name() == "Staking" {
                    println!("Staking constants:");
                    for constant in pallet.constants() {
                        println!("  - {}", constant.name());
                    }
                }
            }

            // Test People chain connection and identity fetching
            println!("\n\n=== PEOPLE CHAIN TEST ===");
            test_people_chain(&client, Network::Polkadot).await;
        }
        Err(e) => {
            println!("Failed to connect: {}", e);
            println!("\n✗ Connection test FAILED");
        }
    }
}

async fn test_people_chain(client: &stkopt_chain::ChainClient, _network: Network) {
    println!("Connecting to People chain...");

    match client.connect_people_chain().await {
        Ok(subxt_client) => {
            println!("✓ Connected to People chain");

            // List available pallets to verify Identity pallet exists
            println!("\n--- Checking People chain pallets ---");
            let block = match subxt_client.at_current_block().await {
                Ok(block) => block,
                Err(e) => {
                    println!("Failed to get People chain block: {}", e);
                    return;
                }
            };
            let metadata = block.metadata();
            for pallet in metadata.pallets() {
                if pallet.name() == "Identity" {
                    println!("Found Identity pallet!");
                    if let Some(storage) = pallet.storage() {
                        println!("Storage entries:");
                        for entry in storage.entries() {
                            println!("  - {}", entry.name());
                        }
                    }
                }
            }

            let people_client = stkopt_chain::PeopleChainClient::new(subxt_client);

            // Fetch validators from Asset Hub
            println!("\n--- Fetching validators from Asset Hub ---");
            let validators = match client.get_validators().await {
                Ok(v) => {
                    println!("Found {} validators", v.len());
                    v
                }
                Err(e) => {
                    println!("Failed to get validators: {}", e);
                    return;
                }
            };

            // Take first 5 validators for testing
            let test_addresses: Vec<_> = validators.iter().take(5).collect();
            println!(
                "Testing identity fetch for first {} validators:",
                test_addresses.len()
            );

            for v in &test_addresses {
                println!("  Validator: {}", v.address);
            }

            // Query People chain for identities
            let addresses: Vec<subxt::utils::AccountId32> =
                test_addresses.iter().map(|v| v.address.clone()).collect();

            match people_client.get_identities(&addresses).await {
                Ok(identities) => {
                    let with_names = identities
                        .iter()
                        .filter(|i| i.display_name.is_some())
                        .count();
                    println!(
                        "\nFound {} identities ({} with display names):",
                        identities.len(),
                        with_names
                    );
                    for id in identities {
                        if let Some(name) = &id.display_name {
                            let addr_short = &id.address.to_string()[..8];
                            println!("  {} => {}", addr_short, name);
                        }
                    }
                }
                Err(e) => {
                    println!("Failed to fetch identities: {}", e);
                }
            }
        }
        Err(e) => {
            println!("✗ Failed to connect to People chain: {}", e);
        }
    }
}
