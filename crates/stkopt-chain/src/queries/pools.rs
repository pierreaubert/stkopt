//! Nomination pool queries.

use crate::ChainClient;
use crate::error::ChainError;
use stkopt_core::Balance;
use subxt::dynamic::{At, DecodedValueThunk, Value};
use subxt::utils::AccountId32;

/// Nomination pool state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolState {
    Open,
    Blocked,
    Destroying,
}

/// Nomination pool information.
#[derive(Debug, Clone)]
pub struct PoolInfo {
    pub id: u32,
    pub state: PoolState,
    pub points: Balance,
    pub member_count: u32,
    pub roles: PoolRoles,
}

/// Pool roles (depositor, root, nominator, bouncer).
#[derive(Debug, Clone)]
pub struct PoolRoles {
    pub depositor: AccountId32,
    pub root: Option<AccountId32>,
    pub nominator: Option<AccountId32>,
    pub bouncer: Option<AccountId32>,
}

/// Pool metadata (name).
#[derive(Debug, Clone)]
pub struct PoolMetadata {
    pub id: u32,
    pub name: String,
}

/// Pool nominations (validators the pool nominates).
#[derive(Debug, Clone)]
pub struct PoolNominations {
    pub pool_id: u32,
    pub stash: AccountId32,
    pub targets: Vec<AccountId32>,
}

/// Derive the pool's bonded account (stash).
/// Uses the same derivation as the Substrate NominationPools pallet.
pub fn derive_pool_account(pool_id: u32, account_type: PoolAccountType) -> AccountId32 {
    use sp_crypto_hashing::blake2_256;

    // Pool accounts are derived using: blake2_256(b"modl" ++ pallet_id ++ account_type ++ pool_id)
    // pallet_id for NominationPools is typically "py/nopls"
    let pallet_id = b"py/nopls";
    let mut data = Vec::with_capacity(4 + 8 + 1 + 4);
    data.extend_from_slice(b"modl");
    data.extend_from_slice(pallet_id);
    data.push(account_type as u8);
    data.extend_from_slice(&pool_id.to_le_bytes());

    // Pad to 32 bytes if needed (standard derivation uses trailing zeros)
    while data.len() < 32 {
        data.push(0);
    }

    // Hash to get the account
    let hash = blake2_256(&data);
    AccountId32::from(hash)
}

/// Pool account types for derivation.
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum PoolAccountType {
    /// Bonded account (stash) - holds staked funds
    Bonded = 0,
    /// Reward account - receives staking rewards
    Reward = 1,
}

impl ChainClient {
    /// Get all bonded nomination pools.
    pub async fn get_nomination_pools(&self) -> Result<Vec<PoolInfo>, ChainError> {
        let storage_query = subxt::dynamic::storage("NominationPools", "BondedPools", ());

        let mut pools = Vec::new();
        let mut iter = self
            .client()
            .storage()
            .at_latest()
            .await?
            .iter(storage_query)
            .await?;

        while let Some(result) = iter.next().await {
            let kv = result?;
            let key_bytes = kv.key_bytes;
            let value: DecodedValueThunk = kv.value;

            // Extract pool ID from key (last 4 bytes as u32)
            if key_bytes.len() < 4 {
                continue;
            }
            let id_bytes: [u8; 4] = key_bytes[key_bytes.len() - 4..]
                .try_into()
                .map_err(|_| ChainError::InvalidData("Invalid pool ID".into()))?;
            let id = u32::from_le_bytes(id_bytes);

            let decoded = value.to_value()?;

            // BondedPoolInner structure
            let points = decoded
                .at("points")
                .and_then(|v: &Value<u32>| v.as_u128())
                .unwrap_or(0);

            let state = match decoded.at("state").and_then(|v: &Value<u32>| v.as_str()) {
                Some("Open") => PoolState::Open,
                Some("Blocked") => PoolState::Blocked,
                Some("Destroying") => PoolState::Destroying,
                _ => PoolState::Open,
            };

            let member_count = decoded
                .at("member_counter")
                .and_then(|v: &Value<u32>| v.as_u128())
                .unwrap_or(0) as u32;

            // Parse roles
            let roles = if let Some(roles_val) = decoded.at("roles") {
                PoolRoles {
                    depositor: parse_account_id(roles_val.at("depositor"))
                        .unwrap_or_else(|| AccountId32::from([0u8; 32])),
                    root: parse_account_id(roles_val.at("root")),
                    nominator: parse_account_id(roles_val.at("nominator")),
                    bouncer: parse_account_id(roles_val.at("bouncer")),
                }
            } else {
                PoolRoles {
                    depositor: AccountId32::from([0u8; 32]),
                    root: None,
                    nominator: None,
                    bouncer: None,
                }
            };

            pools.push(PoolInfo {
                id,
                state,
                points,
                member_count,
                roles,
            });
        }

        // Sort by pool ID
        pools.sort_by_key(|p| p.id);

        Ok(pools)
    }

    /// Get metadata (names) for all pools.
    pub async fn get_pool_metadata(&self) -> Result<Vec<PoolMetadata>, ChainError> {
        let storage_query = subxt::dynamic::storage("NominationPools", "Metadata", ());

        let mut metadata = Vec::new();
        let mut iter = self
            .client()
            .storage()
            .at_latest()
            .await?
            .iter(storage_query)
            .await?;

        let mut count = 0;
        while let Some(result) = iter.next().await {
            count += 1;
            let kv = result?;
            let key_bytes = kv.key_bytes;
            let value: DecodedValueThunk = kv.value;

            // Extract pool ID from key
            if key_bytes.len() < 4 {
                tracing::debug!("Pool metadata key too short: {} bytes", key_bytes.len());
                continue;
            }
            let id_bytes: [u8; 4] = key_bytes[key_bytes.len() - 4..]
                .try_into()
                .map_err(|_| ChainError::InvalidData("Invalid pool ID".into()))?;
            let id = u32::from_le_bytes(id_bytes);

            let decoded = value.to_value()?;
            tracing::debug!("Pool {} raw metadata: {:?}", id, decoded);

            // Metadata is stored as a sequence of bytes
            let name = extract_bytes_as_string(&decoded);
            tracing::debug!("Pool {} extracted name: '{}'", id, name);

            if !name.is_empty() {
                metadata.push(PoolMetadata { id, name });
            }
        }

        tracing::info!(
            "Fetched pool metadata: {} entries iterated, {} with names",
            count,
            metadata.len()
        );

        Ok(metadata)
    }

    /// Get metadata (name) for a specific pool.
    pub async fn get_pool_name(&self, pool_id: u32) -> Result<Option<String>, ChainError> {
        let storage_query = subxt::dynamic::storage(
            "NominationPools",
            "Metadata",
            vec![Value::u128(pool_id as u128)],
        );

        let result: Option<DecodedValueThunk> = self
            .client()
            .storage()
            .at_latest()
            .await?
            .fetch(&storage_query)
            .await?;

        let Some(value) = result else {
            return Ok(None);
        };

        let decoded = value.to_value()?;
        let name = extract_bytes_as_string(&decoded);

        if name.is_empty() {
            Ok(None)
        } else {
            tracing::debug!("Pool {} name: '{}'", pool_id, name);
            Ok(Some(name))
        }
    }

    /// Get the nominations (targets) for a pool by its ID.
    pub async fn get_pool_nominations(
        &self,
        pool_id: u32,
    ) -> Result<Option<PoolNominations>, ChainError> {
        // Derive the pool's bonded (stash) account
        let stash = derive_pool_account(pool_id, PoolAccountType::Bonded);

        // Query Staking.Nominators for this stash account
        let storage_query = subxt::dynamic::storage(
            "Staking",
            "Nominators",
            vec![Value::from_bytes(stash.clone())],
        );

        let result: Option<DecodedValueThunk> = self
            .client()
            .storage()
            .at_latest()
            .await?
            .fetch(&storage_query)
            .await?;

        let Some(value) = result else {
            return Ok(None);
        };

        let decoded = value.to_value()?;

        // Nominations = { targets: Vec<AccountId>, submitted_in: EraIndex, suppressed: bool }
        let mut targets = Vec::new();
        if let Some(targets_val) = decoded.at("targets") {
            // Iterate over targets array
            for i in 0..512 {
                // Max nominators limit
                if let Some(target) = targets_val.at(i) {
                    if let Some(account) = extract_account_id(target) {
                        targets.push(account);
                    }
                } else {
                    break;
                }
            }
        }

        Ok(Some(PoolNominations {
            pool_id,
            stash,
            targets,
        }))
    }
}

/// Parse an AccountId from a dynamic Value.
/// AccountId is stored as a 32-byte array in Option/Some variant.
fn parse_account_id(value: Option<&Value<u32>>) -> Option<AccountId32> {
    let v = value?;

    // For Option<AccountId32>, we need to check if it's Some variant
    // Try to get the inner value if it's a Some variant
    let inner = if let Some(inner_val) = v.at("Some").or_else(|| v.at(0)) {
        inner_val
    } else {
        v
    };

    // Try to extract 32 bytes by iterating indices
    let mut bytes = Vec::with_capacity(32);
    for i in 0..32 {
        if let Some(byte_val) = inner.at(i)
            && let Some(byte) = byte_val.as_u128()
        {
            bytes.push(byte as u8);
        }
    }

    if bytes.len() == 32 {
        let arr: [u8; 32] = bytes.try_into().ok()?;
        Some(AccountId32::from(arr))
    } else {
        None
    }
}

/// Extract an AccountId from a dynamic Value directly.
fn extract_account_id(value: &Value<u32>) -> Option<AccountId32> {
    // Try to extract 32 bytes by iterating indices
    let mut bytes = Vec::with_capacity(32);
    for i in 0..32 {
        if let Some(byte_val) = value.at(i)
            && let Some(byte) = byte_val.as_u128()
        {
            bytes.push(byte as u8);
        }
    }

    if bytes.len() == 32 {
        let arr: [u8; 32] = bytes.try_into().ok()?;
        Some(AccountId32::from(arr))
    } else {
        None
    }
}

/// Extract bytes from a Value and convert to String.
/// Metadata is stored as a BoundedVec<u8>, which decodes as a sequence.
fn extract_bytes_as_string(value: &Value<u32>) -> String {
    // Try to iterate over indices to extract bytes
    let mut bytes = Vec::new();
    for i in 0..1024 {
        // Reasonable max for pool name
        if let Some(byte_val) = value.at(i) {
            if let Some(byte) = byte_val.as_u128() {
                bytes.push(byte as u8);
            } else {
                // Might be a nested structure, try recursively
                for j in 0..32 {
                    if let Some(inner) = byte_val.at(j)
                        && let Some(b) = inner.as_u128()
                    {
                        bytes.push(b as u8);
                    } else {
                        break;
                    }
                }
            }
        } else {
            break;
        }
    }

    // Filter out any null bytes
    let filtered: Vec<u8> = bytes.into_iter().filter(|&b| b != 0).collect();
    String::from_utf8_lossy(&filtered).to_string()
}
