//! Nomination pool queries.

use super::decode_helpers::extract_account_id;
use crate::ChainClient;
use crate::batch_storage::account_key;
use crate::error::ChainError;
use std::collections::HashMap;
use stkopt_core::Balance;
use subxt::dynamic::{At, Value};
use subxt::utils::AccountId32;

/// Nomination pool state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolState {
    Open,
    Blocked,
    Destroying,
}

impl From<PoolState> for stkopt_core::PoolState {
    fn from(state: PoolState) -> Self {
        match state {
            PoolState::Open => stkopt_core::PoolState::Open,
            PoolState::Blocked => stkopt_core::PoolState::Blocked,
            PoolState::Destroying => stkopt_core::PoolState::Destroying,
        }
    }
}

/// Nomination pool information.
#[derive(Debug, Clone)]
pub struct PoolInfo {
    pub id: u32,
    pub state: PoolState,
    pub points: Balance,
    pub member_count: u32,
    pub roles: PoolRoles,
    /// Pool commission rate (0.0 - 1.0), if configured.
    pub commission: Option<f64>,
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
    /// Returns partial results if iteration is interrupted (e.g., connection drop).
    /// Deduplicates by pool ID (light clients may return duplicates during iteration).
    pub async fn get_nomination_pools(&self) -> Result<Vec<PoolInfo>, ChainError> {
        use std::collections::HashMap;

        let storage_query =
            subxt::dynamic::storage::<Vec<Value>, Value>("NominationPools", "BondedPools");

        // Use HashMap to deduplicate by pool ID (light clients may return duplicates)
        let mut pools_map: HashMap<u32, PoolInfo> = HashMap::new();
        let block = self.client().at_current_block().await?;
        let iter_result = block.storage().iter(&storage_query, vec![]).await;

        let mut iter = match iter_result {
            Ok(iter) => iter,
            Err(e) => {
                tracing::warn!("Failed to start pool iteration: {}", e);
                return Err(e.into());
            }
        };

        loop {
            match iter.next().await {
                Some(Ok(kv)) => {
                    let key_bytes = kv.key_bytes();
                    let value = kv.value();

                    // Extract pool ID from key (last 4 bytes as u32)
                    if key_bytes.len() < 4 {
                        continue;
                    }
                    let Ok(id_bytes): Result<[u8; 4], _> =
                        key_bytes[key_bytes.len() - 4..].try_into()
                    else {
                        continue;
                    };
                    let id = u32::from_le_bytes(id_bytes);

                    // Skip if we already have this pool
                    if pools_map.contains_key(&id) {
                        continue;
                    }

                    let Ok(decoded) = value.decode() else {
                        continue;
                    };

                    // BondedPoolInner structure
                    let points = decoded
                        .at("points")
                        .and_then(|v: &Value| v.as_u128())
                        .unwrap_or(0);

                    let state = match decoded.at("state").map(|v: &Value| &v.value) {
                        Some(subxt::ext::scale_value::ValueDef::Variant(variant)) => {
                            match variant.name.as_str() {
                                "Open" => PoolState::Open,
                                "Blocked" => PoolState::Blocked,
                                "Destroying" => PoolState::Destroying,
                                other => {
                                    tracing::warn!(
                                        "Skipping pool {}: unknown state '{}'",
                                        id,
                                        other
                                    );
                                    continue;
                                }
                            }
                        }
                        _ => {
                            tracing::warn!("Skipping pool {}: state field is not a variant", id);
                            continue;
                        }
                    };

                    let member_count = decoded
                        .at("member_counter")
                        .and_then(|v: &Value| v.as_u128())
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

                    // Parse commission (stored as Option<CommissionClaimPermission> in older runtimes
                    // or Commission struct in newer runtimes)
                    let commission = decoded.at("commission").and_then(|c| {
                        // Try to get current commission rate
                        // Commission struct has: { current: Option<(Perbill, AccountId)>, ... }
                        c.at("current")
                            .and_then(|current| {
                                // current is Option<(Perbill, AccountId)>
                                current.at(0).and_then(|perbill| perbill.as_u128())
                            })
                            .map(|perbill| perbill as f64 / 1_000_000_000.0)
                    });

                    pools_map.insert(
                        id,
                        PoolInfo {
                            id,
                            state,
                            points,
                            member_count,
                            roles,
                            commission,
                        },
                    );
                }
                Some(Err(e)) => {
                    // Connection error during iteration - return what we have so far
                    tracing::warn!(
                        "Pool iteration interrupted after {} entries: {}",
                        pools_map.len(),
                        e
                    );
                    break;
                }
                None => {
                    // Iteration complete
                    break;
                }
            }
        }

        // Convert to Vec and sort by pool ID
        let mut pools: Vec<PoolInfo> = pools_map.into_values().collect();
        pools.sort_by_key(|p| p.id);

        Ok(pools)
    }

    /// Get metadata (names) for all pools.
    /// Returns partial results if iteration is interrupted (e.g., connection drop).
    /// Deduplicates by pool ID (light clients may return duplicates during iteration).
    pub async fn get_pool_metadata(&self) -> Result<Vec<PoolMetadata>, ChainError> {
        use std::collections::HashMap;

        let storage_query =
            subxt::dynamic::storage::<Vec<Value>, Value>("NominationPools", "Metadata");

        // Use HashMap to deduplicate by pool ID
        let mut metadata_map: HashMap<u32, String> = HashMap::new();
        let block = self.client().at_current_block().await?;
        let iter_result = block.storage().iter(&storage_query, vec![]).await;

        let mut iter = match iter_result {
            Ok(iter) => iter,
            Err(e) => {
                tracing::warn!("Failed to start pool metadata iteration: {}", e);
                return Err(e.into());
            }
        };

        let mut count = 0;
        loop {
            match iter.next().await {
                Some(Ok(kv)) => {
                    count += 1;
                    let key_bytes = kv.key_bytes();
                    let value = kv.value();

                    // Extract pool ID from key
                    if key_bytes.len() < 4 {
                        tracing::debug!("Pool metadata key too short: {} bytes", key_bytes.len());
                        continue;
                    }
                    let Ok(id_bytes): Result<[u8; 4], _> =
                        key_bytes[key_bytes.len() - 4..].try_into()
                    else {
                        continue;
                    };
                    let id = u32::from_le_bytes(id_bytes);

                    // Skip if we already have this pool's metadata
                    if metadata_map.contains_key(&id) {
                        continue;
                    }

                    let Ok(decoded) = value.decode() else {
                        continue;
                    };
                    tracing::debug!("Pool {} raw metadata: {:?}", id, decoded);

                    // Metadata is stored as a sequence of bytes
                    let name = extract_bytes_as_string(&decoded);
                    tracing::debug!("Pool {} extracted name: '{}'", id, name);

                    if !name.is_empty() {
                        metadata_map.insert(id, name);
                    }
                }
                Some(Err(e)) => {
                    // Connection error during iteration - return what we have so far
                    tracing::warn!(
                        "Pool metadata iteration interrupted after {} entries: {}",
                        count,
                        e
                    );
                    break;
                }
                None => {
                    // Iteration complete
                    break;
                }
            }
        }

        // Convert to Vec
        let metadata: Vec<PoolMetadata> = metadata_map
            .into_iter()
            .map(|(id, name)| PoolMetadata { id, name })
            .collect();

        tracing::info!(
            "Fetched pool metadata: {} entries iterated, {} unique with names",
            count,
            metadata.len()
        );

        Ok(metadata)
    }

    /// Get metadata (name) for a specific pool.
    pub async fn get_pool_name(&self, pool_id: u32) -> Result<Option<String>, ChainError> {
        let storage_query =
            subxt::dynamic::storage::<Vec<Value>, Value>("NominationPools", "Metadata");

        let block = self.client().at_current_block().await?;
        let result = block
            .storage()
            .try_fetch(&storage_query, vec![Value::u128(pool_id as u128)])
            .await?;

        let Some(value) = result else {
            return Ok(None);
        };

        let decoded: Value = value.decode()?;
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
        let storage_query = subxt::dynamic::storage::<Vec<Value>, Value>("Staking", "Nominators");

        let block = self.client().at_current_block().await?;

        // Light clients can fail with "Storage query errors" when they can't retrieve
        // the storage proof. Treat this as "no nominations" rather than a fatal error.
        let result = match block
            .storage()
            .try_fetch(&storage_query, vec![Value::from_bytes(stash.clone())])
            .await
        {
            Ok(r) => r,
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("Storage query errors") {
                    tracing::debug!(
                        "Light client storage query failed for pool {} nominations: {}",
                        pool_id,
                        err_str
                    );
                    return Ok(None);
                }
                return Err(e.into());
            }
        };

        let Some(value) = result else {
            return Ok(None);
        };

        let decoded: Value = value.decode()?;
        Ok(Some(parse_pool_nominations(pool_id, stash, &decoded)))
    }

    /// Get nominations for multiple pools.
    ///
    /// This fetches the derived bonded pool accounts from `Staking.Nominators` in chunks.
    /// Batch failures fall back to single-key reads and return whatever pool nominations
    /// were available, which keeps UI pool APY enrichment best-effort.
    pub async fn get_pool_nominations_batch(
        &self,
        pool_ids: &[u32],
    ) -> Result<Vec<PoolNominations>, ChainError> {
        let pool_accounts: Vec<(u32, AccountId32)> = pool_ids
            .iter()
            .map(|pool_id| {
                (
                    *pool_id,
                    derive_pool_account(*pool_id, PoolAccountType::Bonded),
                )
            })
            .collect();
        let pool_id_by_stash: HashMap<[u8; 32], u32> = pool_accounts
            .iter()
            .map(|(pool_id, stash)| (account_key(stash), *pool_id))
            .collect();

        let mut nominations = Vec::new();
        for chunk in pool_accounts.chunks(50) {
            let stashes: Vec<AccountId32> = chunk.iter().map(|(_, stash)| stash.clone()).collect();
            match self
                .batch_fetch_account_storage_values("Staking", "Nominators", &stashes)
                .await
            {
                Ok(values) => {
                    for (stash_key, decoded) in values {
                        let Some(pool_id) = pool_id_by_stash.get(&stash_key).copied() else {
                            tracing::debug!(
                                "Ignoring unexpected pool nominations response for unknown stash"
                            );
                            continue;
                        };
                        let stash = AccountId32::from(stash_key);
                        nominations.push(parse_pool_nominations(pool_id, stash, &decoded));
                    }
                }
                Err(error) => {
                    tracing::debug!(
                        "Batch pool nominations fetch failed for {} pools: {}",
                        chunk.len(),
                        error
                    );
                    for (pool_id, _) in chunk {
                        match self.get_pool_nominations(*pool_id).await {
                            Ok(Some(pool_nominations)) => nominations.push(pool_nominations),
                            Ok(None) => {}
                            Err(e) => tracing::debug!(
                                "Failed to get nominations for pool {} after batch fallback: {}",
                                pool_id,
                                e
                            ),
                        }
                    }
                }
            }
        }

        nominations.sort_by_key(|n| n.pool_id);
        Ok(nominations)
    }

    /// Get the reward pool state for a pool.
    /// Returns the current reward counter needed to calculate pending rewards.
    pub async fn get_reward_pool(&self, pool_id: u32) -> Result<Option<RewardPool>, ChainError> {
        let storage_query =
            subxt::dynamic::storage::<Vec<Value>, Value>("NominationPools", "RewardPools");

        let block = self.client().at_current_block().await?;
        let result = block
            .storage()
            .try_fetch(&storage_query, vec![Value::u128(pool_id as u128)])
            .await?;

        let Some(value) = result else {
            return Ok(None);
        };

        let decoded: Value = value.decode()?;

        // RewardPool = { last_recorded_reward_counter, last_recorded_total_payouts, total_rewards_claimed, total_commission_pending, total_commission_claimed }
        let last_recorded_reward_counter = decoded
            .at("last_recorded_reward_counter")
            .and_then(|v: &Value| v.as_u128())
            .unwrap_or(0);

        let last_recorded_total_payouts = decoded
            .at("last_recorded_total_payouts")
            .and_then(|v: &Value| v.as_u128())
            .unwrap_or(0);

        let total_rewards_claimed = decoded
            .at("total_rewards_claimed")
            .and_then(|v: &Value| v.as_u128())
            .unwrap_or(0);

        Ok(Some(RewardPool {
            pool_id,
            last_recorded_reward_counter,
            last_recorded_total_payouts,
            total_rewards_claimed,
        }))
    }

    /// Calculate pending rewards for a pool member.
    /// Formula: pending = member_points * (pool_counter - member_counter) / points_to_balance_ratio
    /// Note: This is a simplified calculation; the actual on-chain calculation may differ slightly.
    pub async fn get_pool_pending_rewards(
        &self,
        pool_id: u32,
        member_points: u128,
        member_last_recorded_counter: u128,
    ) -> Result<Balance, ChainError> {
        let reward_pool = self.get_reward_pool(pool_id).await?;

        let Some(reward_pool) = reward_pool else {
            return Ok(0);
        };

        // If member's counter is already at or ahead of pool's counter, no pending rewards
        if member_last_recorded_counter >= reward_pool.last_recorded_reward_counter {
            return Ok(0);
        }

        // Calculate pending rewards
        // The counter difference represents rewards per point since last claim
        let counter_diff = reward_pool.last_recorded_reward_counter - member_last_recorded_counter;

        // pending = member_points * counter_diff / 10^18 (counter is scaled by 10^18)
        // Use u128 arithmetic carefully to avoid overflow
        let pending = if counter_diff > 0 && member_points > 0 {
            // Scale down: counter is in 10^18 precision
            member_points
                .saturating_mul(counter_diff)
                .checked_div(1_000_000_000_000_000_000) // 10^18
                .unwrap_or(0)
        } else {
            0
        };

        tracing::debug!(
            "Pool {} pending rewards: member_points={}, counter_diff={}, pending={}",
            pool_id,
            member_points,
            counter_diff,
            pending
        );

        Ok(pending)
    }
}

fn parse_pool_nominations(pool_id: u32, stash: AccountId32, decoded: &Value) -> PoolNominations {
    // Nominations = { targets: Vec<AccountId>, submitted_in: EraIndex, suppressed: bool }
    let mut targets = Vec::new();
    if let Some(targets_val) = decoded.at("targets") {
        let mut i = 0;
        while let Some(target) = targets_val.at(i) {
            if let Some(account) = extract_account_id(target) {
                targets.push(account);
            }
            i += 1;
        }
    }

    PoolNominations {
        pool_id,
        stash,
        targets,
    }
}

/// Reward pool state (for calculating pending rewards).
#[derive(Debug, Clone)]
pub struct RewardPool {
    pub pool_id: u32,
    /// Current reward counter (scaled by 10^18).
    pub last_recorded_reward_counter: u128,
    /// Total payouts recorded.
    pub last_recorded_total_payouts: Balance,
    /// Total rewards claimed by members.
    pub total_rewards_claimed: Balance,
}

/// Parse an AccountId from a dynamic Value.
/// AccountId is stored as a 32-byte array in Option/Some variant.
fn parse_account_id(value: Option<&Value>) -> Option<AccountId32> {
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

/// Extract bytes from a Value and convert to String.
/// Metadata is stored as a BoundedVec<u8>, which decodes as a sequence.
fn extract_bytes_as_string(value: &Value) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_account_value(bytes: [u8; 32]) -> Value {
        Value::unnamed_composite(bytes.iter().map(|&b| Value::u128(b as u128)))
    }

    // ── derive_pool_account ──

    #[test]
    fn test_derive_pool_account_bonded_consistency() {
        let a = derive_pool_account(42, PoolAccountType::Bonded);
        let b = derive_pool_account(42, PoolAccountType::Bonded);
        assert_eq!(a, b);
    }

    #[test]
    fn test_derive_pool_account_reward_consistency() {
        let a = derive_pool_account(7, PoolAccountType::Reward);
        let b = derive_pool_account(7, PoolAccountType::Reward);
        assert_eq!(a, b);
    }

    #[test]
    fn test_derive_pool_account_different_pool_ids() {
        let a = derive_pool_account(1, PoolAccountType::Bonded);
        let b = derive_pool_account(2, PoolAccountType::Bonded);
        assert_ne!(a, b);
    }

    #[test]
    fn test_derive_pool_account_different_types_same_pool() {
        let bonded = derive_pool_account(5, PoolAccountType::Bonded);
        let reward = derive_pool_account(5, PoolAccountType::Reward);
        assert_ne!(bonded, reward);
    }

    #[test]
    fn test_derive_pool_account_zero_pool_id() {
        let account = derive_pool_account(0, PoolAccountType::Bonded);
        assert_ne!(account, AccountId32::from([0u8; 32]));
    }

    // ── parse_account_id ──

    #[test]
    fn test_parse_account_id_some_variant_unnamed() {
        let bytes = [
            1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30, 31, 32,
        ];
        let some = Value::unnamed_variant("Some", vec![make_account_value(bytes)]);
        assert_eq!(
            parse_account_id(Some(&some)),
            Some(AccountId32::from(bytes))
        );
    }

    #[test]
    fn test_parse_account_id_none_variant() {
        let none = Value::unnamed_variant("None", vec![]);
        assert_eq!(parse_account_id(Some(&none)), None);
    }

    #[test]
    fn test_parse_account_id_none_input() {
        assert_eq!(parse_account_id(None), None);
    }

    #[test]
    fn test_parse_account_id_some_variant_named() {
        let bytes = [
            10u8, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30,
            31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41,
        ];
        let some = Value::named_variant("Some", [("0", make_account_value(bytes))]);
        assert_eq!(
            parse_account_id(Some(&some)),
            Some(AccountId32::from(bytes))
        );
    }

    #[test]
    fn test_parse_account_id_too_short() {
        let short = Value::unnamed_composite(vec![Value::u128(1); 16]);
        assert_eq!(parse_account_id(Some(&short)), None);
    }

    #[test]
    fn test_parse_account_id_empty() {
        let empty = Value::unnamed_composite(vec![]);
        assert_eq!(parse_account_id(Some(&empty)), None);
    }

    // ── extract_bytes_as_string ──

    #[test]
    fn test_extract_bytes_as_string_empty() {
        let value = Value::unnamed_composite(vec![]);
        assert_eq!(extract_bytes_as_string(&value), "");
    }

    #[test]
    fn test_extract_bytes_as_string_simple() {
        let value = Value::unnamed_composite(vec![
            Value::u128(104), // h
            Value::u128(101), // e
            Value::u128(108), // l
            Value::u128(108), // l
            Value::u128(111), // o
        ]);
        assert_eq!(extract_bytes_as_string(&value), "hello");
    }

    #[test]
    fn test_extract_bytes_as_string_nested() {
        let value = Value::unnamed_composite(vec![
            Value::unnamed_composite(vec![Value::u128(119)]), // w
            Value::unnamed_composite(vec![Value::u128(111)]), // o
            Value::unnamed_composite(vec![Value::u128(114)]), // r
            Value::unnamed_composite(vec![Value::u128(108)]), // l
            Value::unnamed_composite(vec![Value::u128(100)]), // d
        ]);
        assert_eq!(extract_bytes_as_string(&value), "world");
    }

    #[test]
    fn test_extract_bytes_as_string_filters_null_bytes() {
        let value = Value::unnamed_composite(vec![
            Value::u128(97), // a
            Value::u128(0),  // null
            Value::u128(98), // b
        ]);
        assert_eq!(extract_bytes_as_string(&value), "ab");
    }

    #[test]
    fn test_extract_bytes_as_string_non_utf8() {
        let value = Value::unnamed_composite(vec![Value::u128(0xC0), Value::u128(0x80)]);
        let result = extract_bytes_as_string(&value);
        assert!(result.contains('�'));
    }

    // ── parse_pool_nominations ──

    #[test]
    fn test_parse_pool_nominations_empty_targets() {
        let decoded = Value::named_composite([("targets", Value::unnamed_composite(vec![]))]);
        let stash = AccountId32::from([9u8; 32]);
        let result = parse_pool_nominations(1, stash.clone(), &decoded);
        assert_eq!(result.pool_id, 1);
        assert_eq!(result.stash, stash);
        assert!(result.targets.is_empty());
    }

    #[test]
    fn test_parse_pool_nominations_single_target() {
        let target = [1u8; 32];
        let decoded = Value::named_composite([(
            "targets",
            Value::unnamed_composite(vec![make_account_value(target)]),
        )]);
        let stash = AccountId32::from([2u8; 32]);
        let result = parse_pool_nominations(3, stash.clone(), &decoded);
        assert_eq!(result.pool_id, 3);
        assert_eq!(result.stash, stash);
        assert_eq!(result.targets, vec![AccountId32::from(target)]);
    }

    #[test]
    fn test_parse_pool_nominations_multiple_targets() {
        let t1 = [1u8; 32];
        let t2 = [2u8; 32];
        let decoded = Value::named_composite([(
            "targets",
            Value::unnamed_composite(vec![make_account_value(t1), make_account_value(t2)]),
        )]);
        let stash = AccountId32::from([3u8; 32]);
        let result = parse_pool_nominations(5, stash.clone(), &decoded);
        assert_eq!(result.targets.len(), 2);
        assert_eq!(result.targets[0], AccountId32::from(t1));
        assert_eq!(result.targets[1], AccountId32::from(t2));
    }

    #[test]
    fn test_parse_pool_nominations_missing_targets() {
        let decoded = Value::named_composite::<&str, [(&str, Value); 0]>([]);
        let stash = AccountId32::from([4u8; 32]);
        let result = parse_pool_nominations(6, stash.clone(), &decoded);
        assert!(result.targets.is_empty());
    }

    #[test]
    fn test_parse_pool_nominations_skips_invalid_accounts() {
        let valid = [5u8; 32];
        let invalid = Value::unnamed_composite(vec![Value::u128(1); 16]);
        let decoded = Value::named_composite([(
            "targets",
            Value::unnamed_composite(vec![invalid, make_account_value(valid)]),
        )]);
        let stash = AccountId32::from([6u8; 32]);
        let result = parse_pool_nominations(7, stash.clone(), &decoded);
        assert_eq!(result.targets.len(), 1);
        assert_eq!(result.targets[0], AccountId32::from(valid));
    }
}
