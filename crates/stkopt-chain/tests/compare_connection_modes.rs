//! Integration test comparing RPC vs Light Client data fetching.
//!
//! Run with: cargo test --test compare_connection_modes -- --nocapture

use stkopt_chain::{ChainClient, ConnectionConfig, ConnectionMode, RpcEndpoints};
use stkopt_core::{ConnectionStatus, Network};
use std::collections::HashSet;
use tokio::sync::mpsc;

const TEST_NETWORK: Network = Network::Polkadot;

/// Create a dummy status channel for testing
fn create_status_channel() -> mpsc::Sender<ConnectionStatus> {
    let (tx, mut rx) = mpsc::channel(10);
    // Drain the channel in background
    tokio::spawn(async move {
        while rx.recv().await.is_some() {}
    });
    tx
}

#[tokio::test]
async fn compare_validators_rpc_vs_lightclient() {
    // Initialize logging
    let _ = tracing_subscriber::fmt()
        .with_env_filter("stkopt_chain=debug,info")
        .try_init();

    println!("\n========================================");
    println!("Comparing RPC vs Light Client validators");
    println!("========================================\n");

    // Connect via RPC first (faster, more reliable for comparison baseline)
    println!("--- Connecting via RPC ---");
    let rpc_config = ConnectionConfig {
        mode: ConnectionMode::Rpc,
        rpc_endpoints: RpcEndpoints::default(),
    };

    let rpc_client = ChainClient::connect(TEST_NETWORK, &rpc_config, create_status_channel())
        .await
        .expect("Failed to connect via RPC");

    println!("RPC connected to {}", TEST_NETWORK);

    // Fetch validators via RPC
    println!("\n--- Fetching validators via RPC ---");
    let rpc_validators = rpc_client
        .get_validators()
        .await
        .expect("Failed to fetch validators via RPC");
    println!("RPC: Found {} validators", rpc_validators.len());

    // Fetch era stakers via RPC
    println!("\n--- Fetching era stakers via RPC ---");
    let era_info = rpc_client
        .get_active_era()
        .await
        .expect("Failed to get active era via RPC")
        .expect("No active era");
    let query_era = era_info.index.saturating_sub(1);
    println!("Query era: {}", query_era);

    let rpc_exposures = rpc_client
        .get_era_stakers_overview(query_era)
        .await
        .expect("Failed to fetch era stakers via RPC");
    println!("RPC: Found {} era stakers for era {}", rpc_exposures.len(), query_era);

    // Now connect via Light Client
    println!("\n--- Connecting via Light Client ---");
    println!("(This may take 30-60 seconds for initial sync...)");

    let lc_config = ConnectionConfig {
        mode: ConnectionMode::LightClient,
        rpc_endpoints: RpcEndpoints::default(),
    };

    let lc_client = match ChainClient::connect(TEST_NETWORK, &lc_config, create_status_channel()).await {
        Ok(client) => client,
        Err(e) => {
            println!("Light client connection failed: {}", e);
            println!("This test requires light client support.");
            return;
        }
    };

    println!("Light client connected to {}", TEST_NETWORK);

    // Wait for light client to stabilize
    println!("\n--- Waiting for light client to stabilize (10s) ---");
    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

    // Fetch validators via Light Client (basic iteration)
    println!("\n--- Fetching validators via Light Client (iteration) ---");
    let lc_validators_iter = match lc_client.get_validators().await {
        Ok(v) => {
            println!("Light Client (iteration): Found {} validators", v.len());
            v
        }
        Err(e) => {
            println!("Light Client (iteration): Failed: {}", e);
            Vec::new()
        }
    };

    // Fetch validators via Light Client (multi-source approach)
    println!("\n--- Fetching validators via Light Client (multi-source) ---");
    let lc_validators = match lc_client.get_validators_light_client().await {
        Ok(v) => {
            println!("Light Client (multi-source): Found {} validators", v.len());
            v
        }
        Err(e) => {
            println!("Light Client (multi-source): Failed: {}", e);
            Vec::new()
        }
    };

    // Fetch era stakers via Light Client
    println!("\n--- Fetching era stakers via Light Client ---");
    let lc_exposures = match lc_client.get_era_stakers_overview(query_era).await {
        Ok(e) => {
            println!("Light Client: Found {} era stakers for era {}", e.len(), query_era);
            e
        }
        Err(e) => {
            println!("Light Client: Failed to fetch era stakers: {}", e);
            Vec::new()
        }
    };

    // Compare results
    println!("\n========================================");
    println!("COMPARISON RESULTS");
    println!("========================================\n");

    // Validators comparison
    let rpc_validator_addrs: HashSet<_> = rpc_validators.iter().map(|v| v.address.to_string()).collect();
    let lc_iter_addrs: HashSet<_> = lc_validators_iter.iter().map(|v| v.address.to_string()).collect();
    let lc_validator_addrs: HashSet<_> = lc_validators.iter().map(|v| v.address.to_string()).collect();

    println!("VALIDATORS:");
    println!("  RPC count:             {}", rpc_validators.len());
    println!("  LC iteration count:    {}", lc_validators_iter.len());
    println!("  LC multi-source count: {}", lc_validators.len());
    println!("  Diff (RPC vs iter):    {}", rpc_validators.len() as i64 - lc_validators_iter.len() as i64);
    println!("  Diff (RPC vs multi):   {}", rpc_validators.len() as i64 - lc_validators.len() as i64);

    let missing_in_lc: Vec<_> = rpc_validator_addrs.difference(&lc_validator_addrs).collect();
    let extra_in_lc: Vec<_> = lc_validator_addrs.difference(&rpc_validator_addrs).collect();

    if !missing_in_lc.is_empty() {
        println!("  Missing in LC:      {} validators", missing_in_lc.len());
        if missing_in_lc.len() <= 10 {
            for addr in &missing_in_lc {
                println!("    - {}", addr);
            }
        }
    }
    if !extra_in_lc.is_empty() {
        println!("  Extra in LC:        {} validators", extra_in_lc.len());
    }

    // Era stakers comparison
    let rpc_staker_addrs: HashSet<_> = rpc_exposures.iter().map(|e| e.address.to_string()).collect();
    let lc_staker_addrs: HashSet<_> = lc_exposures.iter().map(|e| e.address.to_string()).collect();

    println!("\nERA STAKERS (era {}):", query_era);
    println!("  RPC count:          {}", rpc_exposures.len());
    println!("  Light Client count: {}", lc_exposures.len());
    println!("  Difference:         {}", rpc_exposures.len() as i64 - lc_exposures.len() as i64);

    let missing_stakers_in_lc: Vec<_> = rpc_staker_addrs.difference(&lc_staker_addrs).collect();
    let extra_stakers_in_lc: Vec<_> = lc_staker_addrs.difference(&rpc_staker_addrs).collect();

    if !missing_stakers_in_lc.is_empty() {
        println!("  Missing in LC:      {} stakers", missing_stakers_in_lc.len());
    }
    if !extra_stakers_in_lc.is_empty() {
        println!("  Extra in LC:        {} stakers", extra_stakers_in_lc.len());
    }

    // Commission comparison for validators present in both
    println!("\nCOMMISSION CHECK (validators in both sets):");
    let common_validators: HashSet<_> = rpc_validator_addrs.intersection(&lc_validator_addrs).collect();
    println!("  Common validators: {}", common_validators.len());

    let mut commission_mismatches = 0;
    for addr in common_validators.iter().take(10) {
        let rpc_v = rpc_validators.iter().find(|v| &v.address.to_string() == *addr);
        let lc_v = lc_validators.iter().find(|v| &v.address.to_string() == *addr);

        if let (Some(rpc_v), Some(lc_v)) = (rpc_v, lc_v) {
            if (rpc_v.preferences.commission - lc_v.preferences.commission).abs() > 0.0001 {
                commission_mismatches += 1;
                println!(
                    "  Commission mismatch for {}: RPC={:.2}%, LC={:.2}%",
                    addr,
                    rpc_v.preferences.commission * 100.0,
                    lc_v.preferences.commission * 100.0
                );
            }
        }
    }
    if commission_mismatches == 0 {
        println!("  No commission mismatches in sampled validators");
    }

    // Final verdict
    println!("\n========================================");
    println!("VERDICT");
    println!("========================================\n");

    let validators_match = rpc_validators.len() == lc_validators.len() && missing_in_lc.is_empty();
    let stakers_match = rpc_exposures.len() == lc_exposures.len() && missing_stakers_in_lc.is_empty();

    if validators_match && stakers_match {
        println!("✓ SUCCESS: RPC and Light Client return identical data");
    } else {
        println!("✗ MISMATCH DETECTED:");
        if !validators_match {
            println!("  - Validators: RPC has {}, LC has {} ({} missing)",
                rpc_validators.len(), lc_validators.len(), missing_in_lc.len());
        }
        if !stakers_match {
            println!("  - Era stakers: RPC has {}, LC has {} ({} missing)",
                rpc_exposures.len(), lc_exposures.len(), missing_stakers_in_lc.len());
        }
    }

    // Assert for CI (optional - comment out if you just want diagnostic output)
    // assert!(validators_match, "Validator counts should match");
    // assert!(stakers_match, "Era staker counts should match");
}

#[tokio::test]
async fn test_light_client_iteration_completeness() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("stkopt_chain=info")
        .try_init();

    println!("\n========================================");
    println!("Testing Light Client iteration completeness");
    println!("========================================\n");

    let config = ConnectionConfig {
        mode: ConnectionMode::LightClient,
        rpc_endpoints: RpcEndpoints::default(),
    };

    println!("Connecting via Light Client...");
    let client = match ChainClient::connect(TEST_NETWORK, &config, create_status_channel()).await {
        Ok(c) => c,
        Err(e) => {
            println!("Light client connection failed: {}", e);
            return;
        }
    };

    println!("Waiting 15s for light client to stabilize...");
    tokio::time::sleep(tokio::time::Duration::from_secs(15)).await;

    // Try fetching validators multiple times to see if results are consistent
    println!("\n--- Testing iteration consistency ---");
    let mut counts = Vec::new();

    for i in 1..=5 {
        println!("Attempt {}/5...", i);
        match client.get_validators().await {
            Ok(v) => {
                println!("  Got {} validators", v.len());
                counts.push(v.len());
            }
            Err(e) => {
                println!("  Failed: {}", e);
                counts.push(0);
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }

    println!("\nResults: {:?}", counts);

    let max_count = *counts.iter().max().unwrap_or(&0);
    let min_count = *counts.iter().filter(|&&c| c > 0).min().unwrap_or(&0);

    println!("Max: {}, Min (non-zero): {}", max_count, min_count);

    if min_count > 0 && max_count > min_count {
        println!("\n⚠ WARNING: Inconsistent results - iteration appears unreliable");
        println!("  The light client is returning different numbers of validators each time");
    } else if min_count == 0 {
        println!("\n⚠ WARNING: Some iterations failed completely");
    } else {
        println!("\n✓ Results appear consistent");
    }
}
