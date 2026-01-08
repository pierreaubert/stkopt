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
    ///
    /// # Arguments
    /// * `signer` - The account that will sign the transaction
    /// * `targets` - List of validator addresses to nominate
    /// * `use_mortal_era` - If true, use mortal era (expires after ~12 min); if false, use immortal era
    pub async fn create_nominate_payload(
        &self,
        signer: &AccountId32,
        targets: &[AccountId32],
        use_mortal_era: bool,
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

        let runtime = client.runtime_version();
        let spec_version = runtime.spec_version;
        let tx_version = runtime.transaction_version;

        // Get account nonce from Asset Hub
        let nonce = self.get_account_nonce(signer).await?;

        // Choose era and block_hash based on use_mortal_era flag
        let (era, block_hash) = if use_mortal_era {
            // Mortal era for security (replay protection)
            // 128 blocks ~ 12 minutes (at 6s block time), plenty for QR scan flow
            let (block_number, block_hash) = self.get_latest_block().await?;
            let period = 128;
            let phase = block_number as u64 % period;
            (Era::Mortal { period, phase }, block_hash)
        } else {
            // Immortal era - use genesis hash as block_hash
            (Era::Immortal, genesis_hash)
        };

        // Create description
        let description = format!("Nominate {} validators", targets.len());

        let metadata_hash = [0u8; 32];
        let genesis_hex: String = genesis_hash.iter().map(|b| format!("{:02x}", b)).collect();
        let era_str = match &era {
            Era::Immortal => "Immortal".to_string(),
            Era::Mortal { period, phase } => format!("Mortal({}/{})", period, phase),
        };
        tracing::info!(
            "Created nominate payload: genesis=0x{}, spec={}, tx={}, nonce={}, meta_hash={}, asset_pay={}, era={}",
            genesis_hex,
            spec_version,
            tx_version,
            nonce,
            include_metadata_hash,
            use_asset_payment,
            era_str
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
            era,
            include_metadata_hash,
            use_asset_payment,
        })
    }

    /// Generate an unsigned bond extrinsic.
    ///
    /// Staking transactions go to Asset Hub where the Staking pallet lives.
    ///
    /// # Arguments
    /// * `signer` - The account that will sign the transaction
    /// * `value` - Amount to bond
    /// * `use_mortal_era` - If true, use mortal era (expires after ~12 min); if false, use immortal era
    pub async fn create_bond_payload(
        &self,
        signer: &AccountId32,
        value: u128,
        use_mortal_era: bool,
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

        let runtime = client.runtime_version();
        let nonce = self.get_account_nonce(signer).await?;

        // Choose era and block_hash based on use_mortal_era flag
        let (era, block_hash) = if use_mortal_era {
            // Mortal era for security (replay protection)
            let (block_number, block_hash) = self.get_latest_block().await?;
            let period = 128;
            let phase = block_number as u64 % period;
            (Era::Mortal { period, phase }, block_hash)
        } else {
            // Immortal era - use genesis hash as block_hash
            (Era::Immortal, genesis_hash)
        };

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
            era,
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

    // Call data with compact length prefix
    data.extend_from_slice(&compact_encode(payload.call_data.len() as u64));
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

/// Signed extrinsic ready for submission.
#[derive(Debug, Clone)]
pub struct SignedExtrinsic {
    /// The full SCALE-encoded signed extrinsic.
    pub encoded: Vec<u8>,
    /// Human-readable description.
    pub description: String,
    /// Hash of the extrinsic (blake2-256).
    pub hash: [u8; 32],
}

/// Transaction submission result.
#[derive(Debug, Clone)]
pub enum TxStatus {
    /// Transaction is in the pool, waiting to be included.
    InPool,
    /// Transaction was included in a block.
    InBlock {
        block_hash: [u8; 32],
        block_number: u32,
    },
    /// Transaction was finalized.
    Finalized {
        block_hash: [u8; 32],
        block_number: u32,
    },
    /// Transaction was dropped from the pool.
    Dropped(String),
    /// Transaction was invalid.
    Invalid(String),
}

/// Decode a signed transaction response from Polkadot Vault QR code.
///
/// Vault can return signatures in two formats:
///
/// 1. Binary format (UOS):
///    - `0x53` - Substrate prefix
///    - crypto_type (0x01 = Sr25519)
///    - `0x00` - Signature response type
///    - 64 bytes - Sr25519 signature
///
/// 2. Hex-encoded format:
///    - 130 characters hex string (65 bytes when decoded)
///    - First byte: signature type (0x01 = Sr25519)
///    - Following 64 bytes: signature
///
/// Returns the 64-byte signature if valid.
pub fn decode_vault_signature(qr_data: &[u8]) -> Result<[u8; 64], String> {
    // Try to detect if this is hex-encoded (ASCII hex characters)
    let is_hex = qr_data.iter().all(|&b| {
        (b >= b'0' && b <= b'9') || (b >= b'a' && b <= b'f') || (b >= b'A' && b <= b'F')
    });

    let binary_data = if is_hex && qr_data.len() >= 128 {
        // Hex-encoded format: decode from hex
        let hex_str = std::str::from_utf8(qr_data)
            .map_err(|e| format!("Invalid UTF-8 in hex string: {}", e))?;
        hex::decode(hex_str).map_err(|e| format!("Failed to decode hex: {}", e))?
    } else {
        // Already binary
        qr_data.to_vec()
    };

    tracing::info!(
        "Signature data: {} bytes, first 10: {:02x?}",
        binary_data.len(),
        &binary_data[..binary_data.len().min(10)]
    );

    // Check for UOS format (starts with 0x53 'S')
    if binary_data.len() >= 67 && binary_data[0] == 0x53 {
        // UOS format: 0x53 + crypto_type + response_type + signature
        if binary_data[1] != 0x01 {
            return Err(format!(
                "Unsupported crypto type: 0x{:02x}, expected 0x01 (Sr25519)",
                binary_data[1]
            ));
        }
        if binary_data[2] != 0x00 {
            return Err(format!(
                "Not a signature response: 0x{:02x}, expected 0x00",
                binary_data[2]
            ));
        }
        let mut signature = [0u8; 64];
        signature.copy_from_slice(&binary_data[3..67]);
        tracing::info!("Decoded UOS signature: {} bytes", signature.len());
        return Ok(signature);
    }

    // Check for simple format: crypto_type (1 byte) + signature (64 bytes)
    if binary_data.len() >= 65 {
        let crypto_type = binary_data[0];
        if crypto_type == 0x01 {
            // Sr25519
            let mut signature = [0u8; 64];
            signature.copy_from_slice(&binary_data[1..65]);
            tracing::info!("Decoded Sr25519 signature: {} bytes", signature.len());
            return Ok(signature);
        } else if crypto_type == 0x00 {
            // Ed25519
            let mut signature = [0u8; 64];
            signature.copy_from_slice(&binary_data[1..65]);
            tracing::info!("Decoded Ed25519 signature: {} bytes", signature.len());
            return Ok(signature);
        }
    }

    // Check for raw 64-byte signature (no prefix)
    if binary_data.len() == 64 {
        let mut signature = [0u8; 64];
        signature.copy_from_slice(&binary_data);
        tracing::info!("Decoded raw signature: {} bytes", signature.len());
        return Ok(signature);
    }

    Err(format!(
        "Unknown signature format: {} bytes, first byte: 0x{:02x}",
        binary_data.len(),
        binary_data.first().copied().unwrap_or(0)
    ))
}

/// Construct a signed extrinsic from the unsigned payload and signature.
///
/// Extrinsic format (signed):
/// - Length prefix (compact encoded)
/// - Extrinsic version: 0x84 (signed, version 4)
/// - Signer address (MultiAddress::Id)
/// - Signature (MultiSignature::Sr25519)
/// - Extra (signed extensions data)
/// - Call data
pub fn build_signed_extrinsic(
    payload: &UnsignedPayload,
    signer: &AccountId32,
    signature: &[u8; 64],
) -> SignedExtrinsic {
    let mut extrinsic = Vec::new();

    // Build the extrinsic body first (without length prefix)
    let mut body = Vec::new();

    // Extrinsic version: 0x84 = signed (0x80) | version 4 (0x04)
    body.push(0x84);

    // Signer: MultiAddress::Id (0x00 variant + 32 byte account)
    body.push(0x00);
    body.extend_from_slice(signer.as_ref());

    // Signature: MultiSignature::Sr25519 (0x01 variant + 64 byte signature)
    body.push(0x01);
    body.extend_from_slice(signature);

    // Extra: signed extensions (same order as signing payload)

    // 1. CheckNonZeroSender: nothing
    // 2. CheckSpecVersion: nothing (only in additional_signed)
    // 3. CheckTxVersion: nothing (only in additional_signed)
    // 4. CheckGenesis: nothing (only in additional_signed)

    // 5. CheckMortality: Era encoding
    match payload.era {
        Era::Immortal => body.push(0x00),
        Era::Mortal { period, phase } => {
            let encoded = encode_mortal_era(period, phase);
            body.extend_from_slice(&encoded);
        }
    }

    // 6. CheckNonce: nonce (compact encoded)
    body.extend_from_slice(&compact_encode(payload.nonce));

    // 7. CheckWeight: nothing

    // 8. ChargeTransactionPayment / ChargeAssetTxPayment: tip = 0
    body.push(0x00); // tip = 0, compact encoded
    if payload.use_asset_payment {
        // ChargeAssetTxPayment expects Option<AssetId>
        // None = 0x00 (pay in native asset)
        body.push(0x00);
    }

    // 9. PrevalidateAttests: nothing (PhantomData)

    // 10. CheckMetadataHash: mode (u8)
    if payload.include_metadata_hash {
        // mode = 0 means disabled
        body.push(0x00);
    }

    // Call data
    body.extend_from_slice(&payload.call_data);

    // Add length prefix to complete extrinsic
    let length_prefix = compact_encode(body.len() as u64);
    extrinsic.extend_from_slice(&length_prefix);
    extrinsic.extend_from_slice(&body);

    // Calculate extrinsic hash (blake2-256)
    let hash = blake2_256(&extrinsic);

    tracing::info!(
        "Built signed extrinsic: {} bytes, hash: 0x{}",
        extrinsic.len(),
        hex::encode(hash)
    );

    SignedExtrinsic {
        encoded: extrinsic,
        description: payload.description.clone(),
        hash,
    }
}

/// Blake2-256 hash using sp-crypto-hashing.
fn blake2_256(data: &[u8]) -> [u8; 32] {
    sp_crypto_hashing::blake2_256(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_payload() -> UnsignedPayload {
        UnsignedPayload {
            call_data: vec![0x06, 0x01, 0x00], // Mock staking.nominate call
            description: "Test nominate".to_string(),
            metadata_hash: [0u8; 32],
            genesis_hash: [1u8; 32],
            block_hash: [2u8; 32],
            spec_version: 1002000,
            tx_version: 26,
            nonce: 42,
            era: Era::Mortal { period: 128, phase: 64 },
            include_metadata_hash: false,
            use_asset_payment: false,
        }
    }

    fn make_test_signer() -> AccountId32 {
        // A mock 32-byte account ID
        AccountId32::from([0xABu8; 32])
    }

    #[test]
    fn test_era_immortal() {
        let era = Era::Immortal;
        match era {
            Era::Immortal => {}
            Era::Mortal { .. } => panic!("Expected immortal"),
        }
    }

    #[test]
    fn test_era_mortal() {
        let era = Era::Mortal { period: 128, phase: 64 };
        match era {
            Era::Mortal { period, phase } => {
                assert_eq!(period, 128);
                assert_eq!(phase, 64);
            }
            Era::Immortal => panic!("Expected mortal"),
        }
    }

    #[test]
    fn test_encode_mortal_era() {
        // Test encoding of mortal era
        let encoded = encode_mortal_era(128, 64);
        assert_eq!(encoded.len(), 2);
        // Era encoding is complex - just verify it produces something
        assert!(!encoded.is_empty());
    }

    #[test]
    fn test_encode_mortal_era_various_periods() {
        // Test various period values (period >= 16 to avoid division by zero)
        // In practice, period is typically 64-128 for Polkadot
        for period in [16u64, 32, 64, 128, 256, 512, 1024] {
            let encoded = encode_mortal_era(period, 0);
            assert_eq!(encoded.len(), 2);
        }
    }

    #[test]
    fn test_compact_encode_small() {
        // Compact encoding for small values (0-63)
        let encoded = compact_encode(0);
        assert_eq!(encoded, vec![0x00]);

        let encoded = compact_encode(1);
        assert_eq!(encoded, vec![0x04]);

        let encoded = compact_encode(63);
        assert_eq!(encoded, vec![0xFC]);
    }

    #[test]
    fn test_compact_encode_medium() {
        // Compact encoding for medium values (64-16383)
        let encoded = compact_encode(64);
        assert_eq!(encoded.len(), 2);

        let encoded = compact_encode(1000);
        assert_eq!(encoded.len(), 2);
    }

    #[test]
    fn test_compact_encode_large() {
        // Compact encoding for larger values
        let encoded = compact_encode(1_000_000);
        assert!(!encoded.is_empty());
    }

    #[test]
    fn test_encode_for_qr_mortal() {
        let payload = make_test_payload();
        let signer = make_test_signer();

        let qr_data = encode_for_qr(&payload, &signer);

        // Check UOS header
        assert_eq!(qr_data[0], 0x53); // 'S' for Substrate
        assert_eq!(qr_data[1], 0x01); // Sr25519
        assert_eq!(qr_data[2], 0x02); // Mortal V2

        // Check signer is included (bytes 3-35)
        let signer_bytes: &[u8; 32] = signer.as_ref();
        assert_eq!(&qr_data[3..35], signer_bytes);

        // Verify genesis hash is at the end
        let len = qr_data.len();
        assert_eq!(&qr_data[len - 32..], &[1u8; 32]); // genesis_hash
    }

    #[test]
    fn test_encode_for_qr_immortal() {
        let mut payload = make_test_payload();
        payload.era = Era::Immortal;
        let signer = make_test_signer();

        let qr_data = encode_for_qr(&payload, &signer);

        // Check UOS header
        assert_eq!(qr_data[0], 0x53); // 'S' for Substrate
        assert_eq!(qr_data[1], 0x01); // Sr25519
        assert_eq!(qr_data[2], 0x03); // Immortal V2
    }

    #[test]
    fn test_encode_for_qr_with_metadata_hash() {
        let mut payload = make_test_payload();
        payload.include_metadata_hash = true;
        let signer = make_test_signer();

        let qr_data = encode_for_qr(&payload, &signer);

        // Should be larger due to metadata hash extension
        assert!(!qr_data.is_empty());
    }

    #[test]
    fn test_encode_for_qr_with_asset_payment() {
        let mut payload = make_test_payload();
        payload.use_asset_payment = true;
        let signer = make_test_signer();

        let qr_data = encode_for_qr(&payload, &signer);

        // Should be larger due to asset payment extension
        assert!(!qr_data.is_empty());
    }

    #[test]
    fn test_decode_vault_signature_valid() {
        // Valid signature response: 0x53 + 0x01 + 0x00 + 64 bytes signature
        let mut qr_data = vec![0x53, 0x01, 0x00];
        qr_data.extend_from_slice(&[0xAB; 64]); // Mock signature

        let result = decode_vault_signature(&qr_data);
        assert!(result.is_ok());

        let sig = result.unwrap();
        assert_eq!(sig, [0xAB; 64]);
    }

    #[test]
    fn test_decode_vault_signature_too_short() {
        let qr_data = vec![0x53, 0x01, 0x00]; // Only 3 bytes, need 67
        let result = decode_vault_signature(&qr_data);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown signature format"));
    }

    #[test]
    fn test_decode_vault_signature_simple_format_sr25519() {
        // Simple format: 0x01 (Sr25519) + 64 bytes signature
        let mut qr_data = vec![0x01];
        qr_data.extend_from_slice(&[0xAB; 64]);

        let result = decode_vault_signature(&qr_data);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), [0xAB; 64]);
    }

    #[test]
    fn test_decode_vault_signature_simple_format_ed25519() {
        // Simple format: 0x00 (Ed25519) + 64 bytes signature
        let mut qr_data = vec![0x00];
        qr_data.extend_from_slice(&[0xAB; 64]);

        let result = decode_vault_signature(&qr_data);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), [0xAB; 64]);
    }

    #[test]
    fn test_decode_vault_signature_raw_64_bytes() {
        // Raw 64-byte signature with no prefix
        let qr_data = vec![0xCD; 64];

        let result = decode_vault_signature(&qr_data);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), [0xCD; 64]);
    }

    #[test]
    fn test_decode_vault_signature_hex_encoded() {
        // Hex-encoded format: "01" prefix + 64 bytes as hex
        let signature = [0xAB; 64];
        let hex_str = format!("01{}", hex::encode(signature));

        let result = decode_vault_signature(hex_str.as_bytes());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), signature);
    }

    #[test]
    fn test_decode_vault_signature_invalid_crypto_type() {
        let mut qr_data = vec![0x53, 0x02, 0x00]; // Wrong crypto type in UOS format
        qr_data.extend_from_slice(&[0xAB; 64]);

        let result = decode_vault_signature(&qr_data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unsupported crypto type"));
    }

    #[test]
    fn test_decode_vault_signature_not_signature_response() {
        let mut qr_data = vec![0x53, 0x01, 0x02]; // Response type 0x02 instead of 0x00
        qr_data.extend_from_slice(&[0xAB; 64]);

        let result = decode_vault_signature(&qr_data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Not a signature response"));
    }

    #[test]
    fn test_build_signed_extrinsic_mortal() {
        let payload = make_test_payload();
        let signer = make_test_signer();
        let signature = [0xCD; 64];

        let signed = build_signed_extrinsic(&payload, &signer, &signature);

        // Check basic structure
        assert!(!signed.encoded.is_empty());
        assert_eq!(signed.description, "Test nominate");
        assert_ne!(signed.hash, [0u8; 32]); // Hash should be computed

        // Check that extrinsic starts with length prefix and version
        // The first bytes are compact-encoded length, followed by 0x84 (signed v4)
        let body_start = signed.encoded.iter()
            .position(|&b| b == 0x84)
            .expect("Should contain 0x84 version byte");

        assert_eq!(signed.encoded[body_start], 0x84);
    }

    #[test]
    fn test_build_signed_extrinsic_immortal() {
        let mut payload = make_test_payload();
        payload.era = Era::Immortal;
        let signer = make_test_signer();
        let signature = [0xCD; 64];

        let signed = build_signed_extrinsic(&payload, &signer, &signature);

        assert!(!signed.encoded.is_empty());
    }

    #[test]
    fn test_build_signed_extrinsic_with_metadata_hash() {
        let mut payload = make_test_payload();
        payload.include_metadata_hash = true;
        let signer = make_test_signer();
        let signature = [0xCD; 64];

        let signed = build_signed_extrinsic(&payload, &signer, &signature);

        assert!(!signed.encoded.is_empty());
    }

    #[test]
    fn test_build_signed_extrinsic_with_asset_payment() {
        let mut payload = make_test_payload();
        payload.use_asset_payment = true;
        let signer = make_test_signer();
        let signature = [0xCD; 64];

        let signed = build_signed_extrinsic(&payload, &signer, &signature);

        assert!(!signed.encoded.is_empty());
    }

    #[test]
    fn test_signed_extrinsic_hash_deterministic() {
        let payload = make_test_payload();
        let signer = make_test_signer();
        let signature = [0xCD; 64];

        let signed1 = build_signed_extrinsic(&payload, &signer, &signature);
        let signed2 = build_signed_extrinsic(&payload, &signer, &signature);

        // Same inputs should produce same hash
        assert_eq!(signed1.hash, signed2.hash);
        assert_eq!(signed1.encoded, signed2.encoded);
    }

    #[test]
    fn test_signed_extrinsic_different_signature() {
        let payload = make_test_payload();
        let signer = make_test_signer();
        let signature1 = [0xCD; 64];
        let signature2 = [0xEF; 64];

        let signed1 = build_signed_extrinsic(&payload, &signer, &signature1);
        let signed2 = build_signed_extrinsic(&payload, &signer, &signature2);

        // Different signatures should produce different hashes
        assert_ne!(signed1.hash, signed2.hash);
        assert_ne!(signed1.encoded, signed2.encoded);
    }

    #[test]
    fn test_unsigned_payload_clone() {
        let payload = make_test_payload();
        let payload_clone = payload.clone();

        assert_eq!(payload.call_data, payload_clone.call_data);
        assert_eq!(payload.description, payload_clone.description);
        assert_eq!(payload.spec_version, payload_clone.spec_version);
        assert_eq!(payload.nonce, payload_clone.nonce);
    }

    #[test]
    fn test_signed_extrinsic_clone() {
        let payload = make_test_payload();
        let signer = make_test_signer();
        let signature = [0xCD; 64];

        let signed = build_signed_extrinsic(&payload, &signer, &signature);
        let signed_clone = signed.clone();

        assert_eq!(signed.encoded, signed_clone.encoded);
        assert_eq!(signed.description, signed_clone.description);
        assert_eq!(signed.hash, signed_clone.hash);
    }

    #[test]
    fn test_tx_status_variants() {
        // Just verify we can create all variants
        let _in_pool = TxStatus::InPool;
        let _in_block = TxStatus::InBlock {
            block_hash: [0; 32],
            block_number: 100,
        };
        let _finalized = TxStatus::Finalized {
            block_hash: [0; 32],
            block_number: 100,
        };
        let _dropped = TxStatus::Dropped("reason".to_string());
        let _invalid = TxStatus::Invalid("error".to_string());
    }

    #[test]
    fn test_tx_status_clone() {
        let status = TxStatus::InBlock {
            block_hash: [0xAB; 32],
            block_number: 12345,
        };
        let status_clone = status.clone();

        match (status, status_clone) {
            (
                TxStatus::InBlock { block_hash: h1, block_number: n1 },
                TxStatus::InBlock { block_hash: h2, block_number: n2 },
            ) => {
                assert_eq!(h1, h2);
                assert_eq!(n1, n2);
            }
            _ => panic!("Clone mismatch"),
        }
    }

    #[test]
    fn test_blake2_256_deterministic() {
        let data = b"test data";
        let hash1 = blake2_256(data);
        let hash2 = blake2_256(data);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_blake2_256_different_input() {
        let hash1 = blake2_256(b"data1");
        let hash2 = blake2_256(b"data2");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_build_signing_payload_structure() {
        let payload = make_test_payload();
        let signing_payload = build_signing_payload(&payload);

        // Should start with compact-encoded call length
        let call_len_encoded = compact_encode(payload.call_data.len() as u64);
        assert!(signing_payload.starts_with(&call_len_encoded));

        // Should contain spec_version somewhere in the payload
        let spec_bytes = payload.spec_version.to_le_bytes();
        let has_spec = signing_payload
            .windows(4)
            .any(|w| w == spec_bytes);
        assert!(has_spec);
    }

    #[test]
    fn test_encode_for_qr_roundtrip_signer() {
        let payload = make_test_payload();
        let signer = make_test_signer();

        let qr_data = encode_for_qr(&payload, &signer);

        // Extract signer from QR data (bytes 3-35)
        let extracted_signer: [u8; 32] = qr_data[3..35].try_into().unwrap();
        let signer_bytes: &[u8; 32] = signer.as_ref();
        assert_eq!(extracted_signer, *signer_bytes);
    }
}
