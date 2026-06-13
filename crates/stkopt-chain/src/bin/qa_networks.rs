//! Live QA smoke test for every supported network and connection mode.
//!
//! By default this connects to every configured network with both RPC and the
//! light client. Narrow a run while debugging with:
//!
//! cargo run -p stkopt-chain --bin qa_networks -- --network polkadot --mode rpc

use std::fmt;
use std::time::Duration;
use stkopt_chain::{ChainClient, ConnectionConfig, ConnectionMode};
use stkopt_core::{ConnectionStatus, Network};
use subxt::utils::AccountId32;
use subxt::{OnlineClient, PolkadotConfig};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(90);
const QUERY_TIMEOUT: Duration = Duration::from_secs(90);
const PEOPLE_TIMEOUT: Duration = Duration::from_secs(60);
const VALIDATOR_SAMPLE_SIZE: usize = 16;
const PEOPLE_SAMPLE_SIZE: usize = 5;

type QaResult<T> = Result<T, String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModeSelection {
    Both,
    Rpc,
    LightClient,
}

#[derive(Debug)]
struct Args {
    networks: Vec<Network>,
    modes: Vec<ConnectionMode>,
}

#[derive(Debug)]
struct CaseReport {
    network: Network,
    mode: ConnectionMode,
    asset_hub_block: u32,
    relay_block: u32,
    active_era: u32,
    total_stake: u128,
    validators: usize,
    sampled_validator_preferences: usize,
    people_block: u32,
    people_identity_rows: usize,
    validator_identities: usize,
}

#[derive(Debug)]
struct ValidatorSample {
    total_count: usize,
    validators: Vec<stkopt_chain::ValidatorInfo>,
}

impl fmt::Display for CaseReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} / {}: asset hub #{}, relay #{}, era {}, total stake {}, validators {}, sampled prefs {}, people #{}, people rows {}, validator identities {}",
            self.network,
            self.mode,
            self.asset_hub_block,
            self.relay_block,
            self.active_era,
            self.total_stake,
            self.validators,
            self.sampled_validator_preferences,
            self.people_block,
            self.people_identity_rows,
            self.validator_identities
        )
    }
}

#[tokio::main]
async fn main() {
    init_logging();

    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            print_usage();
            std::process::exit(2);
        }
    };

    println!(
        "Running stkopt-chain live QA for {} network(s) and {} mode(s)",
        args.networks.len(),
        args.modes.len()
    );

    let mut failures = Vec::new();

    for network in args.networks {
        for mode in &args.modes {
            println!("\n== {network} / {mode} ==");
            match run_case(network, *mode).await {
                Ok(report) => println!("PASS {report}"),
                Err(error) => {
                    println!("FAIL {network} / {mode}: {error}");
                    failures.push(format!("{network} / {mode}: {error}"));
                }
            }
        }
    }

    if failures.is_empty() {
        println!("\nAll QA checks passed.");
    } else {
        eprintln!("\n{} QA check(s) failed:", failures.len());
        for failure in failures {
            eprintln!("  - {failure}");
        }
        std::process::exit(1);
    }
}

fn init_logging() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    let env_filter = suppress_light_client_chatter(env_filter);

    let _ = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(std::io::stderr)
        .try_init();
}

fn suppress_light_client_chatter(mut filter: EnvFilter) -> EnvFilter {
    for directive in [
        "json-rpc=warn",
        "network=info",
        "runtime=info",
        "sync-service=info",
        "bitswap-service=info",
        "tx-service=info",
        "subxt-light-client-background-task=error",
        "stkopt_chain::lightclient=info",
        "stkopt_chain::queries::identity=info",
    ] {
        filter = filter.add_directive(directive.parse().expect("log directive is valid"));
    }
    filter
}

async fn run_case(network: Network, mode: ConnectionMode) -> QaResult<CaseReport> {
    let (status_tx, mut status_rx) = mpsc::channel::<ConnectionStatus>(32);
    tokio::spawn(async move { while status_rx.recv().await.is_some() {} });

    let config = ConnectionConfig {
        mode,
        ..ConnectionConfig::default()
    };

    let client = with_timeout(
        CONNECT_TIMEOUT,
        format!("connect {network} / {mode}"),
        ChainClient::connect(network, &config, status_tx),
    )
    .await?;

    if mode == ConnectionMode::Rpc && client.connection_mode() != ConnectionMode::Rpc {
        return Err(format!(
            "requested RPC but connected with {}",
            client.connection_mode()
        ));
    }

    if !client.has_relay_connection() {
        return Err("relay chain did not connect".to_string());
    }

    let relay_block = with_timeout(
        QUERY_TIMEOUT,
        "read latest relay block",
        latest_block(client.relay_client()),
    )
    .await?;

    let (asset_hub_block, _) = with_timeout(
        QUERY_TIMEOUT,
        "read latest Asset Hub block",
        client.get_latest_block(),
    )
    .await?;

    let people_client = with_timeout(
        PEOPLE_TIMEOUT,
        "connect People chain",
        client.connect_people_chain_client(),
    )
    .await?;
    let people_block = with_timeout(
        QUERY_TIMEOUT,
        "read latest People chain block",
        latest_block(people_client.online_client()),
    )
    .await?;
    let people_identity_rows = with_timeout(
        PEOPLE_TIMEOUT,
        "sample People.IdentityOf",
        sample_people_identity_rows(people_client.online_client(), PEOPLE_SAMPLE_SIZE),
    )
    .await?;
    if people_identity_rows == 0 {
        return Err("People.IdentityOf returned no rows".to_string());
    }

    let active_era = with_timeout(
        QUERY_TIMEOUT,
        "read Staking.ActiveEra",
        client.get_active_era(),
    )
    .await?
    .ok_or_else(|| "Staking.ActiveEra was empty".to_string())?;

    let total_stake = with_timeout(
        QUERY_TIMEOUT,
        "read Staking.ErasTotalStake",
        client.get_era_total_stake_direct(active_era.index),
    )
    .await?;
    if total_stake == 0 {
        return Err(format!(
            "Staking.ErasTotalStake({}) was zero",
            active_era.index
        ));
    }

    let validator_sample = with_timeout(
        QUERY_TIMEOUT,
        "read validator sample",
        validator_sample(&client),
    )
    .await?;
    if validator_sample.validators.is_empty() {
        return Err("validator sample returned no validators".to_string());
    }

    let sample_addresses: Vec<AccountId32> = validator_sample
        .validators
        .iter()
        .take(VALIDATOR_SAMPLE_SIZE)
        .map(|validator| validator.address.clone())
        .collect();
    let validator_preferences = with_timeout(
        QUERY_TIMEOUT,
        "read validator preferences from Asset Hub",
        client.get_validator_preferences_batch(&sample_addresses),
    )
    .await?;
    if validator_preferences.len() != sample_addresses.len() {
        return Err(format!(
            "validator preference sample returned {} of {} entries",
            validator_preferences.len(),
            sample_addresses.len()
        ));
    }

    let validator_identities = with_timeout(
        PEOPLE_TIMEOUT,
        "lookup identities for sampled validators",
        people_client.get_identities(&sample_addresses),
    )
    .await?
    .len();

    Ok(CaseReport {
        network,
        mode: client.connection_mode(),
        asset_hub_block,
        relay_block,
        active_era: active_era.index,
        total_stake,
        validators: validator_sample.total_count,
        sampled_validator_preferences: validator_preferences.len(),
        people_block,
        people_identity_rows,
        validator_identities,
    })
}

async fn validator_sample(client: &ChainClient) -> QaResult<ValidatorSample> {
    if client.is_light_client() {
        let session_validators = client
            .get_session_validators()
            .await
            .map_err(|error| error.to_string())?;
        let sample_addresses: Vec<AccountId32> = session_validators
            .iter()
            .take(VALIDATOR_SAMPLE_SIZE)
            .cloned()
            .collect();
        let validators = client
            .get_validator_preferences_batch(&sample_addresses)
            .await
            .map_err(|error| error.to_string())?;
        Ok(ValidatorSample {
            total_count: session_validators.len(),
            validators,
        })
    } else {
        let validators = client
            .get_validators()
            .await
            .map_err(|error| error.to_string())?;
        Ok(ValidatorSample {
            total_count: validators.len(),
            validators,
        })
    }
}

async fn latest_block(client: &OnlineClient<PolkadotConfig>) -> QaResult<u32> {
    client
        .at_current_block()
        .await
        .map(|block| block.block_number() as u32)
        .map_err(|error| error.to_string())
}

async fn sample_people_identity_rows(
    client: &OnlineClient<PolkadotConfig>,
    max_rows: usize,
) -> QaResult<usize> {
    let storage_query = subxt::dynamic::storage::<Vec<subxt::dynamic::Value>, subxt::dynamic::Value>(
        "Identity",
        "IdentityOf",
    );
    let block = client.at_current_block().await.map_err(|e| e.to_string())?;
    let mut iter = block
        .storage()
        .iter(&storage_query, Vec::new())
        .await
        .map_err(|e| e.to_string())?;

    let mut rows = 0;
    while rows < max_rows {
        match iter.next().await {
            Some(Ok(kv)) => {
                let _ = kv.value().decode().map_err(|e| e.to_string())?;
                rows += 1;
            }
            Some(Err(error)) => return Err(error.to_string()),
            None => break,
        }
    }

    Ok(rows)
}

async fn with_timeout<T, E: fmt::Display>(
    duration: Duration,
    label: impl Into<String>,
    future: impl std::future::Future<Output = Result<T, E>>,
) -> QaResult<T> {
    let label = label.into();
    tokio::time::timeout(duration, future)
        .await
        .map_err(|_| format!("{label} timed out after {duration:?}"))?
        .map_err(|error| error.to_string())
}

fn parse_args(args: impl IntoIterator<Item = String>) -> QaResult<Args> {
    let mut selected_network: Option<Network> = None;
    let mut selected_mode = ModeSelection::Both;

    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        let (flag, value) = if let Some((flag, value)) = arg.split_once('=') {
            (flag.to_string(), Some(value.to_string()))
        } else {
            (arg, None)
        };

        match flag.as_str() {
            "--network" | "-n" => {
                let value = value
                    .or_else(|| iter.next())
                    .ok_or_else(|| "--network requires a value".to_string())?;
                selected_network = Some(parse_network(&value)?);
            }
            "--mode" | "-m" => {
                let value = value
                    .or_else(|| iter.next())
                    .ok_or_else(|| "--mode requires a value".to_string())?;
                selected_mode = parse_mode_selection(&value)?;
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            unknown => return Err(format!("unknown argument: {unknown}")),
        }
    }

    let networks = selected_network
        .map(|network| vec![network])
        .unwrap_or_else(|| Network::all().to_vec());
    let modes = match selected_mode {
        ModeSelection::Both => vec![ConnectionMode::Rpc, ConnectionMode::LightClient],
        ModeSelection::Rpc => vec![ConnectionMode::Rpc],
        ModeSelection::LightClient => vec![ConnectionMode::LightClient],
    };

    Ok(Args { networks, modes })
}

fn parse_network(value: &str) -> QaResult<Network> {
    match value.to_ascii_lowercase().as_str() {
        "polkadot" | "dot" => Ok(Network::Polkadot),
        "kusama" | "ksm" => Ok(Network::Kusama),
        "westend" | "wnd" => Ok(Network::Westend),
        "paseo" | "pas" => Ok(Network::Paseo),
        _ => Err(format!("unknown network: {value}")),
    }
}

fn parse_mode_selection(value: &str) -> QaResult<ModeSelection> {
    match value.to_ascii_lowercase().as_str() {
        "both" | "all" => Ok(ModeSelection::Both),
        "rpc" => Ok(ModeSelection::Rpc),
        "light" | "light-client" | "lightclient" | "lc" => Ok(ModeSelection::LightClient),
        _ => Err(format!("unknown mode: {value}")),
    }
}

fn print_usage() {
    eprintln!(
        "Usage: cargo run -p stkopt-chain --bin qa_networks -- [--network polkadot|kusama|westend|paseo] [--mode rpc|light|both]"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defaults_to_all_networks_and_modes() {
        let args = parse_args(Vec::<String>::new()).unwrap();

        assert_eq!(args.networks, Network::all());
        assert_eq!(
            args.modes,
            vec![ConnectionMode::Rpc, ConnectionMode::LightClient]
        );
    }

    #[test]
    fn parses_network_and_mode_filters() {
        let args = parse_args(["--network=kusama", "--mode", "light"].map(String::from)).unwrap();

        assert_eq!(args.networks, vec![Network::Kusama]);
        assert_eq!(args.modes, vec![ConnectionMode::LightClient]);
    }

    #[test]
    fn rejects_unknown_arguments() {
        let err = parse_args(["--nope"].map(String::from)).unwrap_err();

        assert!(err.contains("unknown argument"));
    }

    #[test]
    fn light_client_chatter_filter_directives_are_valid() {
        let _ = suppress_light_client_chatter(EnvFilter::new("debug"));
    }
}
