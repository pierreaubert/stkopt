//! Identity queries from the People chain.

use crate::batch_storage::{account_key, fetch_account_storage_values};
use crate::error::ChainError;
use std::sync::Arc;
use subxt::backend::CombinedBackend;
use subxt::dynamic::{At, Value};
use subxt::ext::scale_value::ValueDef;
use subxt::utils::AccountId32;
use subxt::{OnlineClient, PolkadotConfig};

const IDENTITY_BATCH_SIZE: usize = 20;
const IDENTITY_QUERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const PEOPLE_READY_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);
const PEOPLE_READY_ATTEMPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

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
    backend: Option<Arc<CombinedBackend<PolkadotConfig>>>,
}

impl PeopleChainClient {
    /// Create a new People chain client from an existing subxt client.
    pub fn new(client: OnlineClient<PolkadotConfig>) -> Self {
        Self {
            client,
            backend: None,
        }
    }

    /// Create a new People chain client with access to the underlying Subxt backend.
    pub fn with_backend(
        client: OnlineClient<PolkadotConfig>,
        backend: Arc<CombinedBackend<PolkadotConfig>>,
    ) -> Self {
        Self {
            client,
            backend: Some(backend),
        }
    }

    /// Access the underlying Subxt client.
    pub fn online_client(&self) -> &OnlineClient<PolkadotConfig> {
        &self.client
    }

    /// Wait until the People chain client can answer block queries.
    pub async fn wait_until_ready(&self, timeout: std::time::Duration) -> Result<u32, ChainError> {
        let start = std::time::Instant::now();
        let mut last_error = None;

        loop {
            let Some(remaining) = timeout.checked_sub(start.elapsed()) else {
                break;
            };
            if remaining.is_zero() {
                break;
            }

            let attempt_timeout = std::cmp::min(remaining, PEOPLE_READY_ATTEMPT_TIMEOUT);
            match tokio::time::timeout(attempt_timeout, self.client.at_current_block()).await {
                Ok(Ok(block)) => return Ok(block.block_number() as u32),
                Ok(Err(error)) => {
                    last_error = Some(error.to_string());
                }
                Err(_) => {
                    last_error = Some(format!(
                        "attempt timed out after {}s",
                        attempt_timeout.as_secs()
                    ));
                }
            }

            tokio::time::sleep(std::cmp::min(
                PEOPLE_READY_POLL_INTERVAL,
                timeout.saturating_sub(start.elapsed()),
            ))
            .await;
        }

        Err(ChainError::Connection(format!(
            "People chain did not become ready within {}s{}",
            timeout.as_secs(),
            last_error
                .map(|error| format!(" (last error: {error})"))
                .unwrap_or_default()
        )))
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
        let storage_query = subxt::dynamic::storage("Identity", "IdentityOf");

        let block = self.client.at_current_block().await?;
        let result = block
            .storage()
            .try_fetch(&storage_query, vec![Value::from_bytes(address.clone())])
            .await?;

        let Some(value) = result else {
            tracing::trace!("No identity found for address {}", address);
            return Ok(None);
        };

        let decoded: Value = value.decode()?;

        tracing::debug!("Raw identity data for {}: {:?}", address, decoded);

        Ok(Some(parse_direct_identity(address, &decoded)))
    }

    /// Check if this is a sub-identity and get parent's name.
    async fn get_sub_identity(
        &self,
        address: &AccountId32,
    ) -> Result<Option<ValidatorIdentity>, ChainError> {
        // Query Identity.SuperOf storage
        let storage_query = subxt::dynamic::storage("Identity", "SuperOf");

        let block = self.client.at_current_block().await?;
        let result = block
            .storage()
            .try_fetch(&storage_query, vec![Value::from_bytes(address.clone())])
            .await?;

        let Some(value) = result else {
            return Ok(None);
        };

        let decoded: Value = value.decode()?;

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
        if self.backend.is_none() {
            return self.get_identities_fallback(addresses).await;
        }

        let mut identities = Vec::new();

        let total_batches = addresses.len().div_ceil(IDENTITY_BATCH_SIZE);
        let mut batch_num = 0;

        for chunk in addresses.chunks(IDENTITY_BATCH_SIZE) {
            batch_num += 1;
            tracing::info!(
                "Fetching identity batch {}/{} ({} addresses)...",
                batch_num,
                total_batches,
                chunk.len()
            );

            let results =
                tokio::time::timeout(IDENTITY_QUERY_TIMEOUT, self.get_identity_batch(chunk)).await;

            let mut batch_found = 0;
            match results {
                Ok(Ok(batch_identities)) => {
                    batch_found = batch_identities.len();
                    identities.extend(batch_identities);
                }
                Ok(Err(error)) => {
                    tracing::debug!("Failed to fetch identity batch {}: {}", batch_num, error);
                }
                Err(_) => {
                    tracing::trace!("Timeout fetching identity batch {}", batch_num);
                }
            }
            tracing::debug!("Batch {}: found {} identities", batch_num, batch_found);
        }

        Ok(identities)
    }

    async fn get_identities_fallback(
        &self,
        addresses: &[AccountId32],
    ) -> Result<Vec<ValidatorIdentity>, ChainError> {
        let mut identities = Vec::new();
        let total_batches = addresses.len().div_ceil(IDENTITY_BATCH_SIZE);

        for (idx, chunk) in addresses.chunks(IDENTITY_BATCH_SIZE).enumerate() {
            let batch_num = idx + 1;
            tracing::info!(
                "Fetching identity batch {}/{} ({} addresses, single-key fallback)...",
                batch_num,
                total_batches,
                chunk.len()
            );

            let futures: Vec<_> = chunk
                .iter()
                .map(|address| self.get_identity_with_timeout(address))
                .collect();
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

    async fn get_identity_batch(
        &self,
        addresses: &[AccountId32],
    ) -> Result<Vec<ValidatorIdentity>, ChainError> {
        let direct_values = self.fetch_storage_values("IdentityOf", addresses).await?;

        let mut identities = Vec::new();
        let mut misses = Vec::new();
        for address in addresses {
            if let Some(decoded) = direct_values.get(&account_key(address)) {
                identities.push(parse_direct_identity(address, decoded));
            } else {
                misses.push(address.clone());
            }
        }

        if misses.is_empty() {
            return Ok(identities);
        }

        let super_values = self.fetch_storage_values("SuperOf", &misses).await?;
        if super_values.is_empty() {
            return Ok(identities);
        }

        let mut sub_accounts = Vec::new();
        let mut parent_accounts = Vec::new();
        for address in misses {
            let Some(decoded) = super_values.get(&account_key(&address)) else {
                continue;
            };
            if let Some(parent_bytes) = extract_account_bytes(decoded.at(0)) {
                let parent = AccountId32::from(parent_bytes);
                let sub_name = decoded.at(1).and_then(extract_data_field);
                sub_accounts.push((address, parent.clone(), sub_name));
                parent_accounts.push(parent);
            }
        }

        if parent_accounts.is_empty() {
            return Ok(identities);
        }

        let parent_values = self
            .fetch_storage_values("IdentityOf", &parent_accounts)
            .await?;
        for (address, parent, sub_name) in sub_accounts {
            let Some(parent_decoded) = parent_values.get(&account_key(&parent)) else {
                continue;
            };
            let parent_identity = parse_direct_identity(&parent, parent_decoded);
            let full_name = match (&parent_identity.display_name, &sub_name) {
                (Some(parent), Some(sub)) => Some(format!("{}/{}", parent, sub)),
                (Some(parent), None) => Some(parent.clone()),
                (None, Some(sub)) => Some(sub.clone()),
                (None, None) => None,
            };

            identities.push(ValidatorIdentity {
                address,
                display_name: full_name,
                verified: parent_identity.verified,
                sub_identity: sub_name,
            });
        }

        Ok(identities)
    }

    async fn fetch_storage_values(
        &self,
        entry_name: &str,
        addresses: &[AccountId32],
    ) -> Result<std::collections::HashMap<[u8; 32], Value>, ChainError> {
        let backend = self.backend.as_ref().ok_or_else(|| {
            ChainError::InvalidData("People chain batch backend unavailable".to_string())
        })?;
        fetch_account_storage_values(&self.client, backend, "Identity", entry_name, addresses).await
    }

    /// Get identity with a timeout to prevent hanging.
    async fn get_identity_with_timeout(&self, address: &AccountId32) -> Option<ValidatorIdentity> {
        match tokio::time::timeout(IDENTITY_QUERY_TIMEOUT, self.get_identity(address)).await {
            Ok(Ok(identity)) => identity,
            Ok(Err(e)) => {
                tracing::debug!("Failed to fetch identity for {}: {}", address, e);
                None
            }
            Err(_) => {
                tracing::trace!("Timeout fetching identity for {}", address);
                None
            }
        }
    }
}

fn parse_direct_identity(address: &AccountId32, decoded: &Value) -> ValidatorIdentity {
    // IdentityOf returns (Registration, Option<Username>)
    // Registration contains: { judgements, deposit, info }
    // info is IdentityInfo with: { display, legal, web, riot, email, ... }
    let registration = decoded.at(0).unwrap_or(decoded);
    tracing::debug!("Registration for {}: {:?}", address, registration);

    let info = registration
        .at("info")
        .or_else(|| registration.at(2))
        .or_else(|| decoded.at("info"));

    tracing::debug!("Info field for {}: {:?}", address, info);

    let display_field = info.and_then(|i| {
        let field = i.at("display").or_else(|| i.at(0));
        tracing::debug!("Display field for {}: {:?}", address, field);
        field
    });

    let display_name = display_field.and_then(extract_data_field);

    if display_name.is_some() {
        tracing::info!("Found identity for {}: {:?}", address, display_name);
    } else {
        tracing::debug!("No display name found for {}", address);
    }

    let verified = registration
        .at("judgements")
        .or_else(|| registration.at(0))
        .map(has_positive_judgement)
        .unwrap_or(false);

    ValidatorIdentity {
        address: address.clone(),
        display_name,
        verified,
        sub_identity: None,
    }
}

/// Extract string from a Data field (Raw, BlakeTwo256, etc).
fn extract_data_field(value: &Value) -> Option<String> {
    // Data enum variants: None, Raw0-32, BlakeTwo256, Keccak256, ShaThree256
    // Most common is Raw which contains bytes

    tracing::trace!("Extracting data field from: {:?}", value);

    // Check for None variant first
    if value.at("None").is_some() {
        tracing::trace!("Data field is None");
        return None;
    }

    // Try Raw variants first (most common) - these are Raw0 through Raw32
    for variant_name in [
        "Raw0", "Raw1", "Raw2", "Raw3", "Raw4", "Raw5", "Raw6", "Raw7", "Raw8", "Raw9", "Raw10",
        "Raw11", "Raw12", "Raw13", "Raw14", "Raw15", "Raw16", "Raw17", "Raw18", "Raw19", "Raw20",
        "Raw21", "Raw22", "Raw23", "Raw24", "Raw25", "Raw26", "Raw27", "Raw28", "Raw29", "Raw30",
        "Raw31", "Raw32",
    ] {
        if let Some(inner) = value.at(variant_name)
            && let Some(s) = extract_bytes_as_string(inner)
            && !s.is_empty()
        {
            tracing::trace!("Found {}, extracted: {}", variant_name, s);
            return Some(s);
        }
    }

    // Try generic Raw variant
    if let Some(inner) = value.at("Raw")
        && let Some(s) = extract_bytes_as_string(inner)
        && !s.is_empty()
    {
        tracing::trace!("Found generic Raw variant");
        return Some(s);
    }

    // Try to extract using index 0 (unnamed variant tuple)
    if let Some(inner) = value.at(0)
        && let Some(s) = extract_bytes_as_string(inner)
        && !s.is_empty()
    {
        tracing::trace!("Extracted from index 0");
        return Some(s);
    }

    // Last resort: try to extract bytes directly from value itself
    if let Some(s) = extract_bytes_as_string(value)
        && !s.is_empty()
    {
        return Some(s);
    }

    None
}

/// Extract bytes from a value and convert to UTF-8 string.
fn extract_bytes_as_string(value: &Value) -> Option<String> {
    // The value might be a Composite containing primitives, or directly a primitive
    // We need to handle both cases

    fn extract_bytes_recursive(val: &Value, bytes: &mut Vec<u8>) {
        // Check if this is a composite (tuple/struct)
        // scale_value::Value doesn't expose is_composite directly, but we can try accessing elements
        for i in 0..256 {
            if let Some(elem) = val.at(i) {
                // Try to get as u128
                if let Some(b) = elem.as_u128() {
                    bytes.push(b as u8);
                } else {
                    // Recursively extract from nested composite
                    extract_bytes_recursive(elem, bytes);
                }
            } else {
                break;
            }
        }
    }

    let mut bytes = Vec::new();
    extract_bytes_recursive(value, &mut bytes);

    if bytes.is_empty() {
        return None;
    }

    // Filter out null bytes and convert to string
    let filtered: Vec<u8> = bytes.into_iter().filter(|&b| b != 0).collect();
    if filtered.is_empty() {
        return None;
    }

    let result = String::from_utf8_lossy(&filtered).to_string();
    // Only log if we found a non-empty string
    if !result.trim().is_empty() {
        tracing::trace!("Extracted string: '{}'", result);
    }
    Some(result)
}

/// Extract account ID bytes from a Value.
fn extract_account_bytes(value: Option<&Value>) -> Option<[u8; 32]> {
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
///
/// Handles both unit variants (`Reasonable`, `KnownGood`) and struct variants
/// with the same names.
fn has_positive_judgement(judgements: &Value) -> bool {
    // Judgements is a BoundedVec of (RegistrarIndex, Judgement)
    let mut i = 0;
    while let Some(judgement_tuple) = judgements.at(i) {
        // Second element is the Judgement enum
        if let Some(judgement) = judgement_tuple.at(1) {
            if let ValueDef::Variant(variant) = &judgement.value {
                let name = variant.name.as_str();
                if name == "Reasonable" || name == "KnownGood" {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_address() -> AccountId32 {
        AccountId32::from([1u8; 32])
    }

    // ----- extract_bytes_as_string -----

    #[test]
    fn test_extract_bytes_as_string_empty() {
        let value = Value::unnamed_composite(Vec::<Value>::new());
        assert_eq!(extract_bytes_as_string(&value), None);
    }

    #[test]
    fn test_extract_bytes_as_string_single_byte() {
        let value = Value::unnamed_composite(vec![Value::u128(65)]);
        assert_eq!(extract_bytes_as_string(&value), Some("A".to_string()));
    }

    #[test]
    fn test_extract_bytes_as_string_multiple_bytes() {
        let value = Value::unnamed_composite(vec![
            Value::u128(65),
            Value::u128(108),
            Value::u128(105),
            Value::u128(99),
            Value::u128(101),
        ]);
        assert_eq!(extract_bytes_as_string(&value), Some("Alice".to_string()));
    }

    #[test]
    fn test_extract_bytes_as_string_filters_null_bytes() {
        let value = Value::unnamed_composite(vec![
            Value::u128(65),
            Value::u128(0),
            Value::u128(0),
            Value::u128(0),
        ]);
        assert_eq!(extract_bytes_as_string(&value), Some("A".to_string()));
    }

    #[test]
    fn test_extract_bytes_as_string_all_null_bytes() {
        let value = Value::unnamed_composite(vec![Value::u128(0), Value::u128(0)]);
        assert_eq!(extract_bytes_as_string(&value), None);
    }

    #[test]
    fn test_extract_bytes_as_string_nested_composite() {
        let inner =
            Value::unnamed_composite(vec![Value::u128(66), Value::u128(111), Value::u128(98)]);
        let value = Value::unnamed_composite(vec![inner]);
        assert_eq!(extract_bytes_as_string(&value), Some("Bob".to_string()));
    }

    #[test]
    fn test_extract_bytes_as_string_non_utf8() {
        let value = Value::unnamed_composite(vec![Value::u128(0xff), Value::u128(0xfe)]);
        let result = extract_bytes_as_string(&value).unwrap();
        // from_utf8_lossy replaces invalid sequences with the replacement character
        assert_eq!(result, "��");
    }

    // ----- extract_account_bytes -----

    #[test]
    fn test_extract_account_bytes_none() {
        assert_eq!(extract_account_bytes(None), None);
    }

    #[test]
    fn test_extract_account_bytes_empty() {
        let value = Value::unnamed_composite(Vec::<Value>::new());
        assert_eq!(extract_account_bytes(Some(&value)), None);
    }

    #[test]
    fn test_extract_account_bytes_exact_32() {
        let bytes: Vec<Value> = (0..32).map(Value::u128).collect();
        let value = Value::unnamed_composite(bytes);
        let expected: [u8; 32] = std::array::from_fn(|i| i as u8);
        assert_eq!(extract_account_bytes(Some(&value)), Some(expected));
    }

    #[test]
    fn test_extract_account_bytes_less_than_32() {
        let bytes: Vec<Value> = (0..31).map(Value::u128).collect();
        let value = Value::unnamed_composite(bytes);
        assert_eq!(extract_account_bytes(Some(&value)), None);
    }

    #[test]
    fn test_extract_account_bytes_more_than_32() {
        let bytes: Vec<Value> = (0..33).map(Value::u128).collect();
        let value = Value::unnamed_composite(bytes);
        let expected: [u8; 32] = std::array::from_fn(|i| i as u8);
        assert_eq!(extract_account_bytes(Some(&value)), Some(expected));
    }

    // ----- has_positive_judgement -----

    #[test]
    fn test_has_positive_judgement_empty() {
        let judgements = Value::unnamed_composite(Vec::<Value>::new());
        assert!(!has_positive_judgement(&judgements));
    }

    #[test]
    fn test_has_positive_judgement_reasonable_unit_variant() {
        let judgement_tuple = Value::unnamed_composite(vec![
            Value::u128(0),
            Value::unnamed_variant("Reasonable", Vec::<Value>::new()),
        ]);
        let judgements = Value::unnamed_composite(vec![judgement_tuple]);
        // Unit variants Reasonable and KnownGood are positive judgements.
        assert!(has_positive_judgement(&judgements));
    }

    #[test]
    fn test_has_positive_judgement_known_good_unit_variant() {
        let judgement_tuple = Value::unnamed_composite(vec![
            Value::u128(0),
            Value::unnamed_variant("KnownGood", Vec::<Value>::new()),
        ]);
        let judgements = Value::unnamed_composite(vec![judgement_tuple]);
        assert!(has_positive_judgement(&judgements));
    }

    #[test]
    fn test_has_positive_judgement_unknown_variant() {
        let judgement_tuple = Value::unnamed_composite(vec![
            Value::u128(0),
            Value::unnamed_variant("Unknown", Vec::<Value>::new()),
        ]);
        let judgements = Value::unnamed_composite(vec![judgement_tuple]);
        assert!(!has_positive_judgement(&judgements));
    }

    #[test]
    fn test_has_positive_judgement_multiple_variants() {
        let judgements = Value::unnamed_composite(vec![
            Value::unnamed_composite(vec![
                Value::u128(0),
                Value::unnamed_variant("Unknown", Vec::<Value>::new()),
            ]),
            Value::unnamed_composite(vec![
                Value::u128(1),
                Value::unnamed_variant("Reasonable", Vec::<Value>::new()),
            ]),
        ]);
        assert!(has_positive_judgement(&judgements));
    }

    // ----- extract_data_field -----

    #[test]
    fn test_extract_data_field_none_variant() {
        let value = Value::unnamed_variant("None", Vec::<Value>::new());
        assert_eq!(extract_data_field(&value), None);
    }

    #[test]
    fn test_extract_data_field_raw5() {
        let value = Value::unnamed_variant("Raw5", vec![Value::from_bytes(b"Alice")]);
        assert_eq!(extract_data_field(&value), Some("Alice".to_string()));
    }

    #[test]
    fn test_extract_data_field_raw() {
        let value = Value::unnamed_variant("Raw", vec![Value::from_bytes(b"Bob")]);
        assert_eq!(extract_data_field(&value), Some("Bob".to_string()));
    }

    #[test]
    fn test_extract_data_field_index_zero() {
        let value = Value::unnamed_composite(vec![Value::from_bytes(b"Charlie")]);
        assert_eq!(extract_data_field(&value), Some("Charlie".to_string()));
    }

    #[test]
    fn test_extract_data_field_direct_bytes() {
        let value = Value::from_bytes(b"Dave");
        assert_eq!(extract_data_field(&value), Some("Dave".to_string()));
    }

    #[test]
    fn test_extract_data_field_empty_raw() {
        let value = Value::unnamed_variant("Raw5", vec![Value::from_bytes(b"")]);
        assert_eq!(extract_data_field(&value), None);
    }

    // ----- parse_direct_identity -----

    #[test]
    fn test_parse_direct_identity_named_registration() {
        let decoded = Value::unnamed_composite(vec![
            Value::named_composite(vec![
                ("judgements", Value::unnamed_composite(Vec::new())),
                ("deposit", Value::u128(100)),
                (
                    "info",
                    Value::named_composite(vec![(
                        "display",
                        Value::unnamed_variant("Raw5", vec![Value::from_bytes(b"Alice")]),
                    )]),
                ),
            ]),
            Value::unnamed_variant("None", Vec::new()),
        ]);

        let identity = parse_direct_identity(&test_address(), &decoded);
        assert_eq!(identity.address, test_address());
        assert_eq!(identity.display_name, Some("Alice".to_string()));
        assert!(!identity.verified);
        assert_eq!(identity.sub_identity, None);
    }

    #[test]
    fn test_parse_direct_identity_unnamed_registration() {
        let decoded = Value::unnamed_composite(vec![Value::unnamed_composite(vec![
            Value::unnamed_composite(Vec::new()), // judgements at 0
            Value::u128(100),                     // deposit at 1
            Value::unnamed_composite(vec![
                Value::unnamed_variant("Raw5", vec![Value::from_bytes(b"Bob")]), // display at 0
            ]), // info at 2
        ])]);

        let identity = parse_direct_identity(&test_address(), &decoded);
        assert_eq!(identity.display_name, Some("Bob".to_string()));
        assert!(!identity.verified);
    }

    #[test]
    fn test_parse_direct_identity_direct_registration() {
        // No outer tuple wrapper; Registration is returned directly.
        let decoded = Value::named_composite(vec![
            ("judgements", Value::unnamed_composite(Vec::new())),
            ("deposit", Value::u128(100)),
            (
                "info",
                Value::named_composite(vec![(
                    "display",
                    Value::unnamed_variant("Raw5", vec![Value::from_bytes(b"Charlie")]),
                )]),
            ),
        ]);

        let identity = parse_direct_identity(&test_address(), &decoded);
        assert_eq!(identity.display_name, Some("Charlie".to_string()));
        assert!(!identity.verified);
    }

    #[test]
    fn test_parse_direct_identity_no_display() {
        let decoded = Value::unnamed_composite(vec![
            Value::named_composite(vec![
                ("judgements", Value::unnamed_composite(Vec::new())),
                ("deposit", Value::u128(100)),
                (
                    "info",
                    Value::named_composite(vec![(
                        "legal",
                        Value::unnamed_variant("None", Vec::new()),
                    )]),
                ),
            ]),
            Value::unnamed_variant("None", Vec::new()),
        ]);

        let identity = parse_direct_identity(&test_address(), &decoded);
        assert_eq!(identity.display_name, None);
        assert!(!identity.verified);
    }

    #[test]
    fn test_parse_direct_identity_with_judgement() {
        let decoded = Value::unnamed_composite(vec![
            Value::named_composite(vec![
                (
                    "judgements",
                    Value::unnamed_composite(vec![Value::unnamed_composite(vec![
                        Value::u128(0),
                        Value::unnamed_variant("Reasonable", Vec::new()),
                    ])]),
                ),
                ("deposit", Value::u128(100)),
                (
                    "info",
                    Value::named_composite(vec![(
                        "display",
                        Value::unnamed_variant("Raw5", vec![Value::from_bytes(b"Dave")]),
                    )]),
                ),
            ]),
            Value::unnamed_variant("None", Vec::new()),
        ]);

        let identity = parse_direct_identity(&test_address(), &decoded);
        assert_eq!(identity.display_name, Some("Dave".to_string()));
        // Unit-variant Reasonable is a positive judgement.
        assert!(identity.verified);
    }

    #[test]
    fn test_has_positive_judgement_unit_variants() {
        let judgements = Value::unnamed_composite(vec![Value::unnamed_composite(vec![
            Value::u128(0),
            Value::unnamed_variant("Reasonable", Vec::new()),
        ])]);
        assert!(has_positive_judgement(&judgements));

        let judgements = Value::unnamed_composite(vec![Value::unnamed_composite(vec![
            Value::u128(0),
            Value::unnamed_variant("KnownGood", Vec::new()),
        ])]);
        assert!(has_positive_judgement(&judgements));
    }

    #[test]
    fn test_has_positive_judgement_negative_and_unknown() {
        let judgements = Value::unnamed_composite(vec![Value::unnamed_composite(vec![
            Value::u128(0),
            Value::unnamed_variant("Erroneous", Vec::new()),
        ])]);
        assert!(!has_positive_judgement(&judgements));

        let judgements = Value::unnamed_composite(vec![Value::unnamed_composite(vec![
            Value::u128(0),
            Value::unnamed_variant("Unknown", Vec::new()),
        ])]);
        assert!(!has_positive_judgement(&judgements));

        let judgements = Value::unnamed_composite(Vec::new());
        assert!(!has_positive_judgement(&judgements));
    }
}
