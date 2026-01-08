use subxt::{OnlineClient, PolkadotConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to Polkadot Asset Hub
    let url = "wss://polkadot-asset-hub-rpc.polkadot.io";
    println!("Connecting to {}...", url);
    let client = OnlineClient::<PolkadotConfig>::from_url(url).await?;

    println!("Connected!");
    let metadata = client.metadata();
    let extrinsic = metadata.extrinsic();

    println!(
        "Transaction Version: {}",
        client.runtime_version().transaction_version
    );

    for version in 0..=5 {
        if let Some(exts) = extrinsic.transaction_extensions_by_version(version) {
            println!("\nExtensions for version {}:", version);
            for (i, ext) in exts.enumerate() {
                println!("  {}: {}", i + 1, ext.identifier());
            }
        }
    }

    Ok(())
}
