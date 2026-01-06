//! Identity queries from the People chain.

use crate::error::ChainError;
use subxt::dynamic::{At, DecodedValueThunk, Value};
use subxt::utils::AccountId32;
use subxt::{OnlineClient, PolkadotConfig};

/// Validator identity information.
#[derive(Debug, Clone)]
pub struct ValidatorIdentity {
    pub address: AccountId32,
    /// Display name from identity.
    pub display_name: Option<String>,
    /// Whether identity is verified (has judgement).
    pub verified: bool,
    /// Sub-identity name if this is a sub-account.
    pub sub_identity: Option<String>,
}

/// People chain client for identity queries.
pub struct PeopleChainClient {
    client: OnlineClient<PolkadotConfig>,
}

impl PeopleChainClient {
    /// Create a new People chain client from an existing subxt client.
    pub fn new(client: OnlineClient<PolkadotConfig>) -> Self {
        Self { client }
    }

    /// Get identity for a single address.
    pub async fn get_identity(
        &self,
        address: &AccountId32,
    ) -> Result<Option<ValidatorIdentity>, ChainError> {
        // First check for direct identity
        if let Some(identity) = self.get_direct_identity(address).await? {
            return Ok(Some(identity));
        }

        // Check for super identity (this might be a sub-account)
        self.get_sub_identity(address).await
    }

    /// Get direct identity (not sub-identity).
    async fn get_direct_identity(
        &self,
        address: &AccountId32,
    ) -> Result<Option<ValidatorIdentity>, ChainError> {
        // Query Identity.IdentityOf storage
        let storage_query = subxt::dynamic::storage(
            "Identity",
            "IdentityOf",
            vec![Value::from_bytes(address.clone())],
        );

        let result: Option<DecodedValueThunk> = self
            .client
            .storage()
            .at_latest()
            .await?
            .fetch(&storage_query)
            .await?;

        let Some(value) = result else {
            return Ok(None);
        };

        let decoded = value.to_value()?;

        // IdentityOf returns (Registration, Option<Username>)
        // Registration contains: { judgements, deposit, info }
        // info is IdentityInfo with: { display, legal, web, riot, email, ... }

        // Try to get the registration (first element of tuple or direct struct)
        let registration = decoded.at(0).unwrap_or(&decoded);
        let info = registration.at("info");

        let display_name = info
            .and_then(|i| i.at("display"))
            .and_then(extract_data_field);

        // Check judgements for verification
        let verified = registration
            .at("judgements")
            .map(has_positive_judgement)
            .unwrap_or(false);

        Ok(Some(ValidatorIdentity {
            address: address.clone(),
            display_name,
            verified,
            sub_identity: None,
        }))
    }

    /// Check if this is a sub-identity and get parent's name.
    async fn get_sub_identity(
        &self,
        address: &AccountId32,
    ) -> Result<Option<ValidatorIdentity>, ChainError> {
        // Query Identity.SuperOf storage
        let storage_query = subxt::dynamic::storage(
            "Identity",
            "SuperOf",
            vec![Value::from_bytes(address.clone())],
        );

        let result: Option<DecodedValueThunk> = self
            .client
            .storage()
            .at_latest()
            .await?
            .fetch(&storage_query)
            .await?;

        let Some(value) = result else {
            return Ok(None);
        };

        let decoded = value.to_value()?;

        // SuperOf returns (parent_account, sub_name)
        let parent_bytes = extract_account_bytes(decoded.at(0));
        let sub_name = decoded.at(1).and_then(extract_data_field);

        if let Some(parent_bytes) = parent_bytes {
            let parent_account = AccountId32::from(parent_bytes);

            // Get parent's direct identity (not recursive - only one level up)
            if let Ok(Some(parent_identity)) = self.get_direct_identity(&parent_account).await {
                let full_name = match (&parent_identity.display_name, &sub_name) {
                    (Some(parent), Some(sub)) => Some(format!("{}/{}", parent, sub)),
                    (Some(parent), None) => Some(parent.clone()),
                    (None, Some(sub)) => Some(sub.clone()),
                    (None, None) => None,
                };

                return Ok(Some(ValidatorIdentity {
                    address: address.clone(),
                    display_name: full_name,
                    verified: parent_identity.verified,
                    sub_identity: sub_name,
                }));
            }
        }

        Ok(None)
    }

    /// Get identities for multiple addresses in batches.
    pub async fn get_identities(
        &self,
        addresses: &[AccountId32],
    ) -> Result<Vec<ValidatorIdentity>, ChainError> {
        let mut identities = Vec::new();

        // Process in batches to speed up fetching while avoiding overwhelming the RPC
        const BATCH_SIZE: usize = 50;

        let total_batches = addresses.len().div_ceil(BATCH_SIZE);
        let mut batch_num = 0;

        for chunk in addresses.chunks(BATCH_SIZE) {
            batch_num += 1;
            tracing::info!(
                "Fetching identity batch {}/{} ({} addresses)...",
                batch_num,
                total_batches,
                chunk.len()
            );

            // Create futures for all addresses in this batch
            let futures: Vec<_> = chunk
                .iter()
                .map(|address| self.get_identity_with_timeout(address))
                .collect();

            // Execute batch concurrently
            let results = futures::future::join_all(futures).await;

            let mut batch_found = 0;
            for identity in results.into_iter().flatten() {
                identities.push(identity);
                batch_found += 1;
            }
            tracing::debug!("Batch {}: found {} identities", batch_num, batch_found);
        }

        Ok(identities)
    }

    /// Get identity with a timeout to prevent hanging.
    async fn get_identity_with_timeout(
        &self,
        address: &AccountId32,
    ) -> Option<ValidatorIdentity> {
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.get_identity(address),
        )
        .await
        {
            Ok(Ok(identity)) => identity,
            Ok(Err(e)) => {
                tracing::debug!("Failed to fetch identity for {}: {}", address, e);
                None
            }
            Err(_) => {
                tracing::debug!("Timeout fetching identity for {}", address);
                None
            }
        }
    }
}

/// Extract string from a Data field (Raw, BlakeTwo256, etc).
fn extract_data_field(value: &Value<u32>) -> Option<String> {
    // Data enum variants: None, Raw0-32, BlakeTwo256, Sha256, Keccak256, ShaThree256
    // Most common is Raw which contains bytes

    // Try Raw variants first (most common)
    for variant_name in [
        "Raw0", "Raw1", "Raw2", "Raw3", "Raw4", "Raw5", "Raw6", "Raw7", "Raw8", "Raw9", "Raw10",
        "Raw11", "Raw12", "Raw13", "Raw14", "Raw15", "Raw16", "Raw17", "Raw18", "Raw19", "Raw20",
        "Raw21", "Raw22", "Raw23", "Raw24", "Raw25", "Raw26", "Raw27", "Raw28", "Raw29", "Raw30",
        "Raw31", "Raw32",
    ] {
        if let Some(inner) = value.at(variant_name) {
            return extract_bytes_as_string(inner);
        }
    }

    // Try generic Raw variant
    if let Some(inner) = value.at("Raw") {
        return extract_bytes_as_string(inner);
    }

    // Check for None variant
    if value.at("None").is_some() {
        return None;
    }

    // Try to extract directly if it's already bytes
    extract_bytes_as_string(value)
}

/// Extract bytes from a value and convert to UTF-8 string.
fn extract_bytes_as_string(value: &Value<u32>) -> Option<String> {
    let mut bytes = Vec::new();

    // Try iterating indices
    for i in 0..256 {
        if let Some(byte_val) = value.at(i) {
            if let Some(byte) = byte_val.as_u128() {
                bytes.push(byte as u8);
            }
        } else {
            break;
        }
    }

    if bytes.is_empty() {
        return None;
    }

    // Filter out null bytes and convert to string
    let filtered: Vec<u8> = bytes.into_iter().filter(|&b| b != 0).collect();
    if filtered.is_empty() {
        return None;
    }

    Some(String::from_utf8_lossy(&filtered).to_string())
}

/// Extract account ID bytes from a Value.
fn extract_account_bytes(value: Option<&Value<u32>>) -> Option<[u8; 32]> {
    let v = value?;
    let mut bytes = Vec::with_capacity(32);

    for i in 0..32 {
        if let Some(byte_val) = v.at(i)
            && let Some(byte) = byte_val.as_u128()
        {
            bytes.push(byte as u8);
        }
    }

    if bytes.len() == 32 {
        let arr: [u8; 32] = bytes.try_into().ok()?;
        Some(arr)
    } else {
        None
    }
}

/// Check if judgements contain a positive judgement (Reasonable, KnownGood).
fn has_positive_judgement(judgements: &Value<u32>) -> bool {
    // Judgements is a BoundedVec of (RegistrarIndex, Judgement)
    for i in 0..100 {
        if let Some(judgement_tuple) = judgements.at(i) {
            // Second element is the Judgement enum
            if let Some(judgement) = judgement_tuple.at(1) {
                // Positive judgements
                if judgement.at("Reasonable").is_some()
                    || judgement.at("KnownGood").is_some()
                    || judgement.at("LowQuality").is_some()
                {
                    return true;
                }
            }
        } else {
            break;
        }
    }
    false
}
