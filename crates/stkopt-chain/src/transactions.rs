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
    /// Since Polkadot 2.0 (Nov 2025), the Staking pallet lives on Asset Hub,
    /// so staking transactions go to Asset Hub, not the relay chain.
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

        // Use Asset Hub client for transaction data (Staking pallet is on Asset Hub since Polkadot 2.0)
        let asset_hub = self.client();

        // Get the call data (using Asset Hub's metadata for call encoding)
        let call_data = asset_hub.tx().call_data(&call)?;

        // Get Asset Hub info (transactions go to Asset Hub where Staking pallet lives)
        let genesis_hash: [u8; 32] = self.genesis_hash();

        let block = asset_hub.blocks().at_latest().await?;
        let block_hash: [u8; 32] = block.hash().0;

        let runtime = asset_hub.runtime_version();
        let spec_version = runtime.spec_version;
        let tx_version = runtime.transaction_version;

        // Get account nonce from Asset Hub
        let nonce = self.get_account_nonce(signer).await?;

        // Create description
        let description = format!("Nominate {} validators", targets.len());

        // Calculate metadata hash (simplified - real implementation would hash the metadata)
        // NOTE: This is a placeholder. For full security, the metadata hash should be computed
        // from the chain's metadata to enable runtime version verification.
        // Polkadot Vault will still validate genesis hash, spec version, and transaction version.
        let metadata_hash = [0u8; 32];

        tracing::info!(
            "Created nominate payload for Asset Hub: genesis={:?}, spec_version={}, tx_version={}, nonce={}",
            &genesis_hash[..8], // First 8 bytes for logging
            spec_version,
            tx_version,
            nonce
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
            era: Era::Mortal {
                period: 64, // ~6.4 minutes on Polkadot
                phase: 0,
            },
        })
    }

    /// Generate an unsigned bond extrinsic.
    ///
    /// Since Polkadot 2.0 (Nov 2025), staking transactions go to Asset Hub.
    pub async fn create_bond_payload(
        &self,
        signer: &AccountId32,
        value: u128,
    ) -> Result<UnsignedPayload, ChainError> {
        // Build the bond call (bond to self with Staked payee)
        let payee = Value::unnamed_variant("Staked", std::iter::empty::<Value<()>>());
        let call = subxt::dynamic::tx("Staking", "bond", vec![Value::u128(value), payee]);

        // Use Asset Hub client for transaction data
        let asset_hub = self.client();

        let call_data = asset_hub.tx().call_data(&call)?;
        let genesis_hash: [u8; 32] = self.genesis_hash();
        let block = asset_hub.blocks().at_latest().await?;
        let block_hash: [u8; 32] = block.hash().0;
        let runtime = asset_hub.runtime_version();
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
            era: Era::Mortal {
                period: 64,
                phase: 0,
            },
        })
    }

    /// Get account nonce from Asset Hub (for transactions).
    async fn get_account_nonce(&self, account: &AccountId32) -> Result<u64, ChainError> {
        Self::fetch_nonce(self.client(), account).await
    }

    /// Get account nonce from relay chain.
    #[allow(dead_code)]
    async fn get_relay_account_nonce(&self, account: &AccountId32) -> Result<u64, ChainError> {
        Self::fetch_nonce(self.relay_client(), account).await
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
/// UOS (Universal Offline Signature) format:
/// - `0x53` (ASCII 'S') - Substrate prefix
/// - Crypto type: `0x00` = Ed25519, `0x01` = Sr25519
/// - Action type: `0x00` = Standard, `0x01` = Hash, `0x02` = Immortal, `0x03` = Message
/// - 32 bytes - Public key of signer
/// - Signing payload bytes
///
/// Format: `[S][crypto][action][pubkey(32)][payload]`
pub fn encode_for_qr(payload: &UnsignedPayload, signer: &AccountId32) -> Vec<u8> {
    let mut qr_payload = Vec::new();

    // Substrate prefix
    qr_payload.push(0x53); // 'S' for Substrate

    // Crypto type: 0x01 = Sr25519 (default for Polkadot)
    qr_payload.push(0x01);

    // Action type: 0x00 = Standard mortal transaction
    // (0x02 is for immortal, not mortal as previously thought)
    qr_payload.push(0x00);

    // Signer's public key (32 bytes)
    qr_payload.extend_from_slice(signer.as_ref());

    // Build the signing payload
    let signing_payload = build_signing_payload(payload);
    qr_payload.extend_from_slice(&signing_payload);

    qr_payload
}

/// Build the signing payload (the data that gets signed).
fn build_signing_payload(payload: &UnsignedPayload) -> Vec<u8> {
    let mut data = Vec::new();

    // Call data
    data.extend_from_slice(&payload.call_data);

    // Extensions (era, nonce, tip, spec_version, tx_version, genesis_hash, block_hash)
    // Era
    match payload.era {
        Era::Immortal => data.push(0x00),
        Era::Mortal { period, phase } => {
            let encoded = encode_mortal_era(period, phase);
            data.extend_from_slice(&encoded);
        }
    }

    // Nonce (compact encoded)
    data.extend_from_slice(&compact_encode(payload.nonce));

    // Tip (0, compact encoded)
    data.push(0x00);

    // Spec version
    data.extend_from_slice(&payload.spec_version.to_le_bytes());

    // Transaction version
    data.extend_from_slice(&payload.tx_version.to_le_bytes());

    // Genesis hash
    data.extend_from_slice(&payload.genesis_hash);

    // Block hash (for mortality check)
    data.extend_from_slice(&payload.block_hash);

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
