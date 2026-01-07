//! Account-related chain queries.

use crate::ChainClient;
use crate::error::ChainError;
use stkopt_core::{Balance, EraIndex};
use subxt::dynamic::{At, DecodedValueThunk, Value};
use subxt::utils::AccountId32;

/// Account balance information.
#[derive(Debug, Clone)]
pub struct AccountBalance {
    pub free: Balance,
    pub reserved: Balance,
    pub frozen: Balance,
}

/// Staking ledger information.
#[derive(Debug, Clone)]
pub struct StakingLedger {
    pub stash: AccountId32,
    pub total: Balance,
    pub active: Balance,
    pub unlocking: Vec<UnlockChunk>,
}

/// An unlocking chunk.
#[derive(Debug, Clone)]
pub struct UnlockChunk {
    pub value: Balance,
    pub era: EraIndex,
}

/// Nomination information for a nominator.
#[derive(Debug, Clone)]
pub struct NominatorInfo {
    pub targets: Vec<AccountId32>,
    pub submitted_in: EraIndex,
}

/// Pool membership information.
#[derive(Debug, Clone)]
pub struct PoolMembership {
    pub pool_id: u32,
    pub points: Balance,
    pub unbonding_eras: Vec<(EraIndex, Balance)>,
}

impl ChainClient {
    /// Get account balance information.
    pub async fn get_account_balance(
        &self,
        account: &AccountId32,
    ) -> Result<AccountBalance, ChainError> {
        let storage_query = subxt::dynamic::storage(
            "System",
            "Account",
            vec![Value::from_bytes(account.clone())],
        );

        let result: Option<DecodedValueThunk> = self
            .client()
            .storage()
            .at_latest()
            .await?
            .fetch(&storage_query)
            .await?;

        let Some(value) = result else {
            return Ok(AccountBalance {
                free: 0,
                reserved: 0,
                frozen: 0,
            });
        };

        let decoded = value.to_value()?;

        // AccountInfo = { nonce, consumers, providers, sufficients, data: AccountData }
        // AccountData = { free, reserved, frozen, flags }
        let data = decoded.at("data");

        let free = data
            .and_then(|d| d.at("free"))
            .and_then(|v: &Value<u32>| v.as_u128())
            .unwrap_or(0);

        let reserved = data
            .and_then(|d| d.at("reserved"))
            .and_then(|v: &Value<u32>| v.as_u128())
            .unwrap_or(0);

        let frozen = data
            .and_then(|d| d.at("frozen"))
            .and_then(|v: &Value<u32>| v.as_u128())
            .unwrap_or(0);

        Ok(AccountBalance {
            free,
            reserved,
            frozen,
        })
    }

    /// Get staking ledger for a controller account.
    pub async fn get_staking_ledger(
        &self,
        stash: &AccountId32,
    ) -> Result<Option<StakingLedger>, ChainError> {
        // In newer runtimes, the stash is used directly (no separate controller)
        let storage_query =
            subxt::dynamic::storage("Staking", "Ledger", vec![Value::from_bytes(stash.clone())]);

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

        // StakingLedger = { stash, total, active, unlocking, legacy_claimed_rewards }
        let total = decoded
            .at("total")
            .and_then(|v: &Value<u32>| v.as_u128())
            .unwrap_or(0);

        let active = decoded
            .at("active")
            .and_then(|v: &Value<u32>| v.as_u128())
            .unwrap_or(0);

        // Parse unlocking chunks
        let mut unlocking = Vec::new();
        if let Some(unlocking_val) = decoded.at("unlocking") {
            for i in 0..32 {
                // Max unlocking chunks
                if let Some(chunk) = unlocking_val.at(i) {
                    let chunk_value = chunk
                        .at("value")
                        .and_then(|v: &Value<u32>| v.as_u128())
                        .unwrap_or(0);
                    let chunk_era = chunk
                        .at("era")
                        .and_then(|v: &Value<u32>| v.as_u128())
                        .unwrap_or(0) as u32;

                    if chunk_value > 0 {
                        unlocking.push(UnlockChunk {
                            value: chunk_value,
                            era: chunk_era,
                        });
                    }
                } else {
                    break;
                }
            }
        }

        Ok(Some(StakingLedger {
            stash: stash.clone(),
            total,
            active,
            unlocking,
        }))
    }

    /// Get nominations for an account.
    pub async fn get_nominations(
        &self,
        stash: &AccountId32,
    ) -> Result<Option<NominatorInfo>, ChainError> {
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

        // Nominations = { targets, submitted_in, suppressed }
        let submitted_in = decoded
            .at("submitted_in")
            .and_then(|v: &Value<u32>| v.as_u128())
            .unwrap_or(0) as u32;

        let mut targets = Vec::new();
        if let Some(targets_val) = decoded.at("targets") {
            for i in 0..16 {
                // Max nominations
                if let Some(target) = targets_val.at(i) {
                    if let Some(account) = extract_account_id(target) {
                        targets.push(account);
                    }
                } else {
                    break;
                }
            }
        }

        Ok(Some(NominatorInfo {
            targets,
            submitted_in,
        }))
    }

    /// Get pool membership for an account.
    pub async fn get_pool_membership(
        &self,
        account: &AccountId32,
    ) -> Result<Option<PoolMembership>, ChainError> {
        let storage_query = subxt::dynamic::storage(
            "NominationPools",
            "PoolMembers",
            vec![Value::from_bytes(account.clone())],
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

        // PoolMember = { pool_id, points, last_recorded_reward_counter, unbonding_eras }
        let pool_id = decoded
            .at("pool_id")
            .and_then(|v: &Value<u32>| v.as_u128())
            .unwrap_or(0) as u32;

        let points = decoded
            .at("points")
            .and_then(|v: &Value<u32>| v.as_u128())
            .unwrap_or(0);

        // Parse unbonding eras (BTreeMap<EraIndex, Balance>)
        let mut unbonding_eras = Vec::new();
        if let Some(unbonding_val) = decoded.at("unbonding_eras") {
            // BTreeMap is serialized as array of [key, value] pairs
            for i in 0..32 {
                if let Some(pair) = unbonding_val.at(i) {
                    let era = pair
                        .at(0)
                        .and_then(|v: &Value<u32>| v.as_u128())
                        .unwrap_or(0) as u32;
                    let amount = pair
                        .at(1)
                        .and_then(|v: &Value<u32>| v.as_u128())
                        .unwrap_or(0);
                    if amount > 0 {
                        unbonding_eras.push((era, amount));
                    }
                } else {
                    break;
                }
            }
        }

        Ok(Some(PoolMembership {
            pool_id,
            points,
            unbonding_eras,
        }))
    }
}

/// Extract an AccountId from a dynamic Value.
fn extract_account_id(value: &Value<u32>) -> Option<AccountId32> {
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
