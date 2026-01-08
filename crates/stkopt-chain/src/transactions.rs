//! Transaction generation for QR code signing.

use crate::ChainClient;
use crate::error::ChainError;
use subxt::dynamic::{At, Value};
pub use subxt::utils::AccountId32;

/// Unsigned extrinsic payload for QR code signing.
#[derive(Debug, Clone)]
pub struct UnsignedPayload {
    /// The call data (SCALE encoded).
    pub call_data: Vec<u8>,
    /// Human-readable description of the call.
    pub description: String,
    /// Metadata hash for verification.
    pub metadata_hash: [u8; 32],
    /// Genesis hash for the chain.
    pub genesis_hash: [u8; 32],
    /// Block hash for mortality.
    pub block_hash: [u8; 32],
    /// Runtime spec version.
    pub spec_version: u32,
    /// Transaction version.
    pub tx_version: u32,
    /// Nonce for the signing account.
    pub nonce: u64,
    /// Era for mortality (mortal or immortal).
    pub era: Era,
    /// Whether to include CheckMetadataHash extension.
    pub include_metadata_hash: bool,
    /// Whether to use ChargeAssetTxPayment (Asset Hub) instead of ChargeTransactionPayment.
    pub use_asset_payment: bool,
}

/// Transaction era (mortality).
#[derive(Debug, Clone, Copy)]
pub enum Era {
    /// Immortal transaction (never expires).
    Immortal,
    /// Mortal transaction with period and phase.
    Mortal { period: u64, phase: u64 },
}

impl ChainClient {
    /// Generate an unsigned nomination extrinsic.
    ///
    /// Staking transactions go to Asset Hub where the Staking pallet lives (Polkadot 2.0).
    pub async fn create_nominate_payload(
        &self,
        signer: &AccountId32,
        targets: &[AccountId32],
    ) -> Result<UnsignedPayload, ChainError> {
        // Build the nominate call
        let target_values: Vec<Value<()>> = targets
            .iter()
            .map(|t| {
                // MultiAddress::Id variant
                Value::named_variant("Id", [("0", Value::from_bytes(t.clone()))])
            })
            .collect();

        let call = subxt::dynamic::tx(
            "Staking",
            "nominate",
            vec![Value::unnamed_composite(target_values)],
        );

        // Use Asset Hub client
        let client = self.client();

        // Get the call data (using Asset Hub's metadata)
        let call_data = client.tx().call_data(&call)?;

        // Check extensions in metadata
        let metadata = client.metadata();
        // Try to find extensions for common versions (usually 4, but subxt might index at 0)
        let extensions: Vec<_> = (0..=5)
            .find_map(|v| metadata.extrinsic().transaction_extensions_by_version(v))
            .map(|iter| iter.collect())
            .unwrap_or_default();
            
        let include_metadata_hash = extensions
            .iter()
            .any(|e| e.identifier() == "CheckMetadataHash");
        let use_asset_payment = extensions
            .iter()
            .any(|e| e.identifier() == "ChargeAssetTxPayment");

        // Use Asset Hub genesis hash
        let genesis_hash: [u8; 32] = self.genesis_hash();

        // For Immortal transactions, the mortality checkpoint is the genesis hash
        let block_hash = genesis_hash;

        let runtime = client.runtime_version();
        let spec_version = runtime.spec_version;
        let tx_version = runtime.transaction_version;

        // Get account nonce from Asset Hub
        let nonce = self.get_account_nonce(signer).await?;

        // Create description
        let description = format!("Nominate {} validators", targets.len());

        let metadata_hash = [0u8; 32];
        let genesis_hex: String = genesis_hash.iter().map(|b| format!("{:02x}", b)).collect();
        tracing::info!(
            "Created nominate payload for Asset Hub: genesis=0x{}, spec_version={}, tx_version={}, nonce={}, meta_hash_ext={}, asset_payment={}, era=Immortal",
            genesis_hex,
            spec_version,
            tx_version,
            nonce,
            include_metadata_hash,
            use_asset_payment
        );

        Ok(UnsignedPayload {
            call_data,
            description,
            metadata_hash,
            genesis_hash,
            block_hash,
            spec_version,
            tx_version,
            nonce,
            era: Era::Immortal,
            include_metadata_hash,
            use_asset_payment,
        })
    }

    /// Generate an unsigned bond extrinsic.
    ///
    /// Staking transactions go to Asset Hub where the Staking pallet lives.
    pub async fn create_bond_payload(
        &self,
        signer: &AccountId32,
        value: u128,
    ) -> Result<UnsignedPayload, ChainError> {
        // Build the bond call (bond to self with Staked payee)
        let payee = Value::unnamed_variant("Staked", std::iter::empty::<Value<()>>());
        let call = subxt::dynamic::tx("Staking", "bond", vec![Value::u128(value), payee]);

        // Use Asset Hub client
        let client = self.client();

        let call_data = client.tx().call_data(&call)?;
        
        // Check extensions in metadata
        let metadata = client.metadata();
        let extensions: Vec<_> = (0..=5)
            .find_map(|v| metadata.extrinsic().transaction_extensions_by_version(v))
            .map(|iter| iter.collect())
            .unwrap_or_default();

        let include_metadata_hash = extensions
            .iter()
            .any(|e| e.identifier() == "CheckMetadataHash");
        let use_asset_payment = extensions
            .iter()
            .any(|e| e.identifier() == "ChargeAssetTxPayment");

        let genesis_hash: [u8; 32] = self.genesis_hash();
        // For Immortal transactions, the mortality checkpoint is the genesis hash
        let block_hash = genesis_hash;
        
        let runtime = client.runtime_version();
        let nonce = self.get_account_nonce(signer).await?;

        Ok(UnsignedPayload {
            call_data,
            description: format!("Bond {} tokens", value),
            // NOTE: Placeholder for metadata hash. See nominate payload for details.
            metadata_hash: [0u8; 32],
            genesis_hash,
            block_hash,
            spec_version: runtime.spec_version,
            tx_version: runtime.transaction_version,
            nonce,
            era: Era::Immortal,
            include_metadata_hash,
            use_asset_payment,
        })
    }

    /// Get account nonce from Asset Hub (for transactions).
    async fn get_account_nonce(&self, account: &AccountId32) -> Result<u64, ChainError> {
        Self::fetch_nonce(self.client(), account).await
    }

    /// Fetch nonce from a client.
    async fn fetch_nonce(
        client: &subxt::OnlineClient<subxt::PolkadotConfig>,
        account: &AccountId32,
    ) -> Result<u64, ChainError> {
        let storage_query = subxt::dynamic::storage(
            "System",
            "Account",
            vec![Value::from_bytes(account.clone())],
        );

        let result = client
            .storage()
            .at_latest()
            .await?
            .fetch(&storage_query)
            .await?;

        if let Some(value) = result {
            let decoded = value.to_value()?;
            let nonce = decoded
                .at("nonce")
                .and_then(|v: &subxt::dynamic::Value<u32>| v.as_u128())
                .unwrap_or(0);
            Ok(nonce as u64)
        } else {
            Ok(0)
        }
    }
}

/// Encode payload for Polkadot Vault QR code.
///
/// Polkadot Vault expects **raw binary data** in the QR code (UOS format).
/// Format:
/// - `0x53` - Substrate prefix (ASCII 'S')
/// - `0x01` - Crypto type: Sr25519
/// - `0x00`/`0x02` - Command: sign mortal tx / sign immortal tx
/// - 32 bytes - Public key of signer
/// - Signing payload (full payload, signer handles hashing if > 256 bytes)
/// - 32 bytes - Genesis hash (appended at end for chain verification)
///
/// Note: When payload > 256 bytes, the Substrate runtime automatically hashes
/// it before signing. We always include the full payload in the QR.
///
/// Reference: https://github.com/maciejhirsz/uos
pub fn encode_for_qr(payload: &UnsignedPayload, signer: &AccountId32) -> Vec<u8> {
    let mut qr_payload = Vec::new();

    // Substrate prefix
    qr_payload.push(0x53); // 'S' for Substrate

    // Crypto type: 0x01 = Sr25519 (default for Polkadot)
    qr_payload.push(0x01);

    // Build the signing payload
    let signing_payload = build_signing_payload(payload);

    // Command byte: UOS V2 (includes genesis hash)
    // 0x02 = Mortal V2
    // 0x03 = Immortal V2
    let is_immortal = matches!(payload.era, Era::Immortal);

    if is_immortal {
        qr_payload.push(0x03); // Immortal V2
    } else {
        qr_payload.push(0x02); // Mortal V2
    }

    // Signer's public key (32 bytes)
    qr_payload.extend_from_slice(signer.as_ref());

    // Full signing payload (signer will hash if > 256 bytes)
    qr_payload.extend_from_slice(&signing_payload);

    // Append genesis hash at the end (for chain verification)
    qr_payload.extend_from_slice(&payload.genesis_hash);

    tracing::info!(
        "QR payload: {} bytes total (signing payload: {} bytes, {})",
        qr_payload.len(),
        signing_payload.len(),
        if is_immortal { "immortal" } else { "mortal" }
    );

    // Return raw binary data - Polkadot Vault expects UOS binary format
    qr_payload
}

/// Build the signing payload (the data that gets signed).
///
/// Polkadot relay chain uses these signed extensions in order:
/// 1. CheckNonZeroSender - Encode: (), AdditionalSigned: ()
/// 2. CheckSpecVersion - Encode: (), AdditionalSigned: spec_version
/// 3. CheckTxVersion - Encode: (), AdditionalSigned: tx_version
/// 4. CheckGenesis - Encode: (), AdditionalSigned: genesis_hash
/// 5. CheckMortality - Encode: Era, AdditionalSigned: block_hash
/// 6. CheckNonce - Encode: nonce (compact), AdditionalSigned: ()
/// 7. CheckWeight - Encode: (), AdditionalSigned: ()
/// 8. ChargeTransactionPayment - Encode: tip (compact), AdditionalSigned: ()
/// 9. PrevalidateAttests - Encode: (), AdditionalSigned: ()
/// 10. CheckMetadataHash - Encode: mode (u8), AdditionalSigned: Option<hash>
///
/// Signing payload = call ++ extras ++ additional_signed
fn build_signing_payload(payload: &UnsignedPayload) -> Vec<u8> {
    let mut data = Vec::new();

    // Wrap in <Bytes> tags - required by some signers for raw data
    data.extend_from_slice(b"<Bytes>");

    // Call data
    data.extend_from_slice(&payload.call_data);

    // === Encoded Extras (in extension order) ===

    // 1. CheckNonZeroSender: nothing
    // 2. CheckSpecVersion: nothing
    // 3. CheckTxVersion: nothing
    // 4. CheckGenesis: nothing

    // 5. CheckMortality: Era encoding
    match payload.era {
        Era::Immortal => data.push(0x00),
        Era::Mortal { period, phase } => {
            let encoded = encode_mortal_era(period, phase);
            data.extend_from_slice(&encoded);
        }
    }

    // 6. CheckNonce: nonce (compact encoded)
    data.extend_from_slice(&compact_encode(payload.nonce));

    // 7. CheckWeight: nothing

    // 8. ChargeTransactionPayment / ChargeAssetTxPayment
    data.push(0x00); // tip = 0, compact encoded
    if payload.use_asset_payment {
        // ChargeAssetTxPayment expects Option<AssetId>
        // None = 0x00 (pay in native asset)
        data.push(0x00);
    }

    // 9. PrevalidateAttests: nothing (PhantomData)

    // 10. CheckMetadataHash: mode (u8)
    if payload.include_metadata_hash {
        // mode = 0 means disabled (no metadata hash verification)
        data.push(0x00);
    }

    // === Additional Signed Data (in extension order) ===

    // 1. CheckNonZeroSender: nothing
    // 2. CheckSpecVersion: spec_version (u32 le)
    data.extend_from_slice(&payload.spec_version.to_le_bytes());

    // 3. CheckTxVersion: tx_version (u32 le)
    data.extend_from_slice(&payload.tx_version.to_le_bytes());

    // 4. CheckGenesis: genesis_hash (32 bytes)
    data.extend_from_slice(&payload.genesis_hash);

    // 5. CheckMortality: block_hash (32 bytes) - mortality checkpoint
    data.extend_from_slice(&payload.block_hash);

    // 6. CheckNonce: nothing
    // 7. CheckWeight: nothing
    // 8. ChargeTransactionPayment: nothing
    // 9. PrevalidateAttests: nothing

    // 10. CheckMetadataHash: Option<[u8;32]>
    if payload.include_metadata_hash {
        // When mode = 0, this is None
        data.push(0x00); // None encoding
    }

    // Close <Bytes> tag
    data.extend_from_slice(b"</Bytes>");

    data
}

/// Encode mortal era (simplified).
fn encode_mortal_era(period: u64, phase: u64) -> Vec<u8> {
    let period = period.next_power_of_two().clamp(4, 65536);
    let quantize_factor = (period >> 12).max(1);
    let quantized_phase = (phase / quantize_factor) * quantize_factor;

    let encoded = {
        let period_log = period.trailing_zeros() - 1;
        let low = period_log.min(15) as u16;
        let high = ((quantized_phase / (period >> 4)) as u16).min(15) << 4;
        low | high
    };

    encoded.to_le_bytes().to_vec()
}

/// Compact encode a u64 value.
fn compact_encode(value: u64) -> Vec<u8> {
    if value < 0x40 {
        vec![(value as u8) << 2]
    } else if value < 0x4000 {
        let v = ((value as u16) << 2) | 0x01;
        v.to_le_bytes().to_vec()
    } else if value < 0x4000_0000 {
        let v = ((value as u32) << 2) | 0x02;
        v.to_le_bytes().to_vec()
    } else {
        let mut result = vec![0x03];
        result.extend_from_slice(&value.to_le_bytes());
        result
    }
}
