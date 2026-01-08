use subxt::{OnlineClient, PolkadotConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = "wss://polkadot-asset-hub-rpc.polkadot.io";
    println!("Connecting to {}...", url);
    let client = OnlineClient::<PolkadotConfig>::from_url(url).await?;

    println!("Connected!");
    let metadata = client.metadata();

    println!("Pallets:");
    for pallet in metadata.pallets() {
        println!(" - {}", pallet.name());
    }

    // Check specifically for Staking
    if metadata.pallet_by_name("Staking").is_some() {
        println!("\n✅ Staking pallet FOUND on Asset Hub");
    } else {
        println!("\n❌ Staking pallet NOT FOUND on Asset Hub");
    }

    Ok(())
}
