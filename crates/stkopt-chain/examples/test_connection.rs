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
            let storage_query = subxt::dynamic::storage("Staking", "ActiveEra", ());
            let storage = client.client().storage().at_latest().await.unwrap();
            let metadata = client.client().metadata();

            // First, let's check if we can get runtime constants (to verify metadata works)
            println!("Checking runtime constants...");
            let sessions_per_era = subxt::dynamic::constant("Staking", "SessionsPerEra");
            match client.client().constants().at(&sessions_per_era) {
                Ok(val) => {
                    let decoded = val.to_value().unwrap();
                    println!("SessionsPerEra constant: {:?}", decoded);
                }
                Err(e) => {
                    println!("Failed to get SessionsPerEra: {}", e);
                }
            }

            // Try to get validators count (a simple value)
            println!("\nTrying Staking.ValidatorCount with () key...");
            let validator_count_query = subxt::dynamic::storage("Staking", "ValidatorCount", ());
            match storage.fetch(&validator_count_query).await {
                Ok(Some(value)) => {
                    let decoded = value.to_value().unwrap();
                    println!("ValidatorCount: {:?}", decoded);
                }
                Ok(None) => println!("ValidatorCount: None"),
                Err(e) => println!("ValidatorCount error: {}", e),
            }

            // Try with Vec<Value> empty key
            println!("\nTrying Staking.ValidatorCount with vec![] key...");
            let validator_count_query2 = subxt::dynamic::storage(
                "Staking",
                "ValidatorCount",
                Vec::<subxt::dynamic::Value<()>>::new(),
            );
            match storage.fetch(&validator_count_query2).await {
                Ok(Some(value)) => {
                    let decoded = value.to_value().unwrap();
                    println!("ValidatorCount (vec): {:?}", decoded);
                }
                Ok(None) => println!("ValidatorCount (vec): None"),
                Err(e) => println!("ValidatorCount (vec) error: {}", e),
            }

            // Try using fetch_or_default
            println!("\nTrying Staking.ValidatorCount with fetch_or_default...");
            match storage.fetch_or_default(&validator_count_query).await {
                Ok(value) => {
                    let decoded = value.to_value().unwrap();
                    println!("ValidatorCount (default): {:?}", decoded);
                }
                Err(e) => println!("ValidatorCount (default) error: {}", e),
            }

            // Try to get raw bytes
            println!("\nTrying raw storage fetch for ValidatorCount...");
            let storage_key = validator_count_query.to_root_bytes();
            println!("Storage key: 0x{}", hex::encode(&storage_key));
            match storage.fetch_raw(storage_key).await {
                Ok(Some(bytes)) => {
                    println!("Raw bytes: 0x{}", hex::encode(&bytes));
                }
                Ok(None) => println!("Raw bytes: None"),
                Err(e) => println!("Raw fetch error: {}", e),
            }

            // Try ActiveEra raw
            println!("\nTrying raw storage fetch for ActiveEra...");
            let active_era_key = storage_query.to_root_bytes();
            println!("ActiveEra key: 0x{}", hex::encode(&active_era_key));
            match storage.fetch_raw(active_era_key).await {
                Ok(Some(bytes)) => {
                    println!("Raw bytes: 0x{}", hex::encode(&bytes));
                }
                Ok(None) => println!("Raw bytes: None"),
                Err(e) => println!("Raw fetch error: {}", e),
            }

            // Try ActiveEra
            println!("\nTrying Staking.ActiveEra...");
            match storage.fetch(&storage_query).await {
                Ok(Some(value)) => {
                    println!("ActiveEra raw value found!");
                    let decoded = value.to_value().unwrap();
                    println!("Decoded value: {:?}", decoded);
                }
                Ok(None) => {
                    println!("ActiveEra returned None from fetch()");

                    // Try to list available storage entries
                    println!("\nTrying to fetch CurrentEra instead...");
                    let current_era_query = subxt::dynamic::storage("Staking", "CurrentEra", ());
                    match storage.fetch(&current_era_query).await {
                        Ok(Some(value)) => {
                            let decoded = value.to_value().unwrap();
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
            let number_query = subxt::dynamic::storage("System", "Number", ());
            match storage.fetch(&number_query).await {
                Ok(Some(value)) => {
                    let decoded = value.to_value().unwrap();
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
            let bonded_iter = subxt::dynamic::storage("Staking", "Bonded", ());
            let iter = storage.iter(bonded_iter).await;
            match iter {
                Ok(mut stream) => {
                    println!("Bonded iterator created");
                    use futures::StreamExt;
                    let mut count = 0;
                    while let Some(item) = stream.next().await {
                        if count >= 3 {
                            break;
                        }
                        match item {
                            Ok(kv) => {
                                println!("  Bonded entry key: 0x{}", hex::encode(&kv.key_bytes));
                                let val = kv.value.to_value().unwrap();
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
            let mode_query = subxt::dynamic::storage("StakingAhClient", "Mode", ());
            match storage.fetch(&mode_query).await {
                Ok(Some(value)) => {
                    let decoded = value.to_value().unwrap();
                    println!("StakingAhClient.Mode: {:?}", decoded);
                }
                Ok(None) => println!("StakingAhClient.Mode: None"),
                Err(e) => println!("StakingAhClient.Mode error: {}", e),
            }

            // Check Session.CurrentIndex
            println!("\n--- Checking Session pallet ---");
            let session_idx = subxt::dynamic::storage("Session", "CurrentIndex", ());
            match storage.fetch(&session_idx).await {
                Ok(Some(value)) => {
                    let decoded = value.to_value().unwrap();
                    println!("Session.CurrentIndex: {:?}", decoded);
                }
                Ok(None) => println!("Session.CurrentIndex: None"),
                Err(e) => println!("Session.CurrentIndex error: {}", e),
            }

            // Check StakingAhClient.ValidatorSet
            println!("\n--- Checking StakingAhClient.ValidatorSet ---");
            let validator_set_query =
                subxt::dynamic::storage("StakingAhClient", "ValidatorSet", ());
            let iter = storage.iter(validator_set_query).await;
            match iter {
                Ok(mut stream) => {
                    use futures::StreamExt;
                    let mut count = 0;
                    while let Some(item) = stream.next().await {
                        if count >= 5 {
                            println!("  ... (showing first 5)");
                            break;
                        }
                        match item {
                            Ok(kv) => {
                                let val = kv.value.to_value().unwrap();
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
            let session_validators = subxt::dynamic::storage("Session", "Validators", ());
            match storage.fetch(&session_validators).await {
                Ok(Some(value)) => {
                    let decoded = value.to_value().unwrap();
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
            let aura_slot = subxt::dynamic::constant("Aura", "SlotDuration");
            match client.client().constants().at(&aura_slot) {
                Ok(val) => {
                    let decoded = val.to_value().unwrap();
                    println!("Aura.SlotDuration: {:?}", decoded);
                }
                Err(e) => println!("Aura.SlotDuration error: {}", e),
            }

            // Check ParachainSystem for block time
            let system_block_time = subxt::dynamic::constant("System", "BlockTime");
            match client.client().constants().at(&system_block_time) {
                Ok(val) => {
                    let decoded = val.to_value().unwrap();
                    println!("System.BlockTime: {:?}", decoded);
                }
                Err(e) => println!("System.BlockTime error: {}", e),
            }

            // Check Staking.SessionsPerEra
            let sessions = subxt::dynamic::constant("Staking", "SessionsPerEra");
            match client.client().constants().at(&sessions) {
                Ok(val) => {
                    let decoded = val.to_value().unwrap();
                    println!("Staking.SessionsPerEra: {:?}", decoded);
                }
                Err(e) => println!("Staking.SessionsPerEra error: {}", e),
            }

            // Check Staking.MaxEraDuration
            let max_era = subxt::dynamic::constant("Staking", "MaxEraDuration");
            match client.client().constants().at(&max_era) {
                Ok(val) => {
                    let decoded = val.to_value().unwrap();
                    println!("Staking.MaxEraDuration: {:?}", decoded);
                }
                Err(e) => println!("Staking.MaxEraDuration error: {}", e),
            }

            // Check Session.Period if it exists
            let session_period = subxt::dynamic::constant("Session", "Period");
            match client.client().constants().at(&session_period) {
                Ok(val) => {
                    let decoded = val.to_value().unwrap();
                    println!("Session.Period: {:?}", decoded);
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

async fn test_people_chain(client: &stkopt_chain::ChainClient, network: Network) {
    println!("Connecting to {} People chain...", network);

    match stkopt_chain::connect_people_chain(network, None).await {
        Ok(subxt_client) => {
            println!("✓ Connected to People chain");

            // List available pallets to verify Identity pallet exists
            println!("\n--- Checking People chain pallets ---");
            let metadata = subxt_client.metadata();
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
