use subxt::{OnlineClient, PolkadotConfig};

#[tokio::main]

async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = "wss://westend-asset-hub-rpc.polkadot.io";

    println!("Connecting to {}...", url);

    let client = OnlineClient::<PolkadotConfig>::from_url(url).await?;

    println!("Connected!");

    let block = client.at_current_block().await?;
    let metadata = block.metadata();

    println!("\nTransaction Extensions:");

    let extensions: Vec<_> = (0..=5)
        .find_map(|v| metadata.extrinsic().transaction_extensions_by_version(v))
        .map(|iter| iter.collect())
        .unwrap_or_default();

    for ext in extensions {
        println!(" - {}", ext.identifier());
    }

    Ok(())
}
