//! Validator-related chain queries.

use crate::ChainClient;
use crate::error::ChainError;
use stkopt_core::{Balance, EraIndex, ValidatorPreferences};
use subxt::dynamic::{At, DecodedValueThunk, Value};
use subxt::utils::AccountId32;

/// Raw validator data from chain.
#[derive(Debug, Clone)]
pub struct ValidatorInfo {
    pub address: AccountId32,
    pub preferences: ValidatorPreferences,
}

/// Era reward points for a validator.
#[derive(Debug, Clone)]
pub struct ValidatorPoints {
    pub address: AccountId32,
    pub points: u32,
}

/// Era staking exposure for a validator.
#[derive(Debug, Clone)]
pub struct ValidatorExposure {
    pub address: AccountId32,
    pub own: Balance,
    pub total: Balance,
    pub nominator_count: u32,
}

impl ChainClient {
    /// Get all registered validators with their preferences.
    pub async fn get_validators(&self) -> Result<Vec<ValidatorInfo>, ChainError> {
        let storage_query = subxt::dynamic::storage("Staking", "Validators", ());

        let mut validators = Vec::new();
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

            // Extract account ID from storage key (last 32 bytes)
            if key_bytes.len() < 32 {
                continue;
            }
            let account_bytes: [u8; 32] = key_bytes[key_bytes.len() - 32..]
                .try_into()
                .map_err(|_| ChainError::InvalidData("Invalid account key".into()))?;
            let address = AccountId32::from(account_bytes);

            let decoded = value.to_value()?;

            // ValidatorPrefs = { commission: Perbill, blocked: bool }
            let commission_perbill = decoded
                .at("commission")
                .and_then(|v: &Value<u32>| v.as_u128())
                .unwrap_or(0);
            // Perbill is parts per billion (1_000_000_000 = 100%)
            let commission = commission_perbill as f64 / 1_000_000_000.0;

            let blocked = decoded
                .at("blocked")
                .and_then(|v: &Value<u32>| v.as_bool())
                .unwrap_or(false);

            validators.push(ValidatorInfo {
                address,
                preferences: ValidatorPreferences {
                    commission,
                    blocked,
                },
            });
        }

        Ok(validators)
    }

    /// Get reward points for all validators in a specific era.
    pub async fn get_era_reward_points(
        &self,
        era: EraIndex,
    ) -> Result<(u32, Vec<ValidatorPoints>), ChainError> {
        let storage_query = subxt::dynamic::storage(
            "Staking",
            "ErasRewardPoints",
            vec![Value::u128(era as u128)],
        );

        let result: Option<DecodedValueThunk> = self
            .client()
            .storage()
            .at_latest()
            .await?
            .fetch(&storage_query)
            .await?;

        let Some(value) = result else {
            return Ok((0, Vec::new()));
        };

        let decoded = value.to_value()?;

        // EraRewardPoints = { total: u32, individual: BTreeMap<AccountId, u32> }
        let total = decoded
            .at("total")
            .and_then(|v: &Value<u32>| v.as_u128())
            .unwrap_or(0) as u32;

        let mut validators = Vec::new();

        // individual is a BTreeMap, which encodes as a sequence of (Key, Value) tuples
        if let Some(individual) = decoded.at("individual") {
            // Iterate over the map entries
            // Depending on subxt/scale-value version, this might be represented as a sequence of tuples
            // or a map. We'll try to iterate assuming it's a sequence/composite.

            // We can iterate by index or use values() iterator if available
            // Let's iterate up to a reasonable limit or until we find no more items
            for i in 0..10000 {
                if let Some(entry) = individual.at(i) {
                    // Each entry should be a tuple (AccountId, points)
                    if let Some(account_val) = entry.at(0)
                        && let Some(points_val) = entry.at(1)
                    {
                        // Extract account ID
                        let mut account_bytes = [0u8; 32];
                        let mut bytes_found = false;

                        // AccountId might be wrapped or direct bytes
                        // Try to get as slice of bytes if it's a Primitive(U128/U256 etc) or a Composite
                        // subxt Value doesn't have as_bytes(), so we have to try different ways

                        let mut extracted_bytes = Vec::new();

                        // Case 1: Sequence of u8
                        for k in 0..32 {
                            if let Some(b_val) = account_val.at(k) {
                                if let Some(b) = b_val.as_u128() {
                                    extracted_bytes.push(b as u8);
                                }
                            } else {
                                break;
                            }
                        }

                        if extracted_bytes.len() == 32 {
                            account_bytes.copy_from_slice(&extracted_bytes);
                            bytes_found = true;
                        }
                        // Note: AccountId32 is [u8; 32], handled above via byte iteration

                        if bytes_found {
                            let points = points_val.as_u128().unwrap_or(0) as u32;
                            validators.push(ValidatorPoints {
                                address: AccountId32::from(account_bytes),
                                points,
                            });
                        }
                    }
                } else {
                    break;
                }
            }
        }

        Ok((total, validators))
    }

    /// Get the total validator reward for an era.
    pub async fn get_era_validator_reward(
        &self,
        era: EraIndex,
    ) -> Result<Option<Balance>, ChainError> {
        let storage_query = subxt::dynamic::storage(
            "Staking",
            "ErasValidatorReward",
            vec![Value::u128(era as u128)],
        );

        let result: Option<DecodedValueThunk> = self
            .client()
            .storage()
            .at_latest()
            .await?
            .fetch(&storage_query)
            .await?;

        Ok(result
            .and_then(|v| v.to_value().ok())
            .and_then(|v| v.as_u128()))
    }

    /// Get staking exposure for validators in an era (using ErasStakersOverview).
    pub async fn get_era_stakers_overview(
        &self,
        era: EraIndex,
    ) -> Result<Vec<ValidatorExposure>, ChainError> {
        // For iterating with a partial key, we need to use the era as the first key
        let storage_query = subxt::dynamic::storage(
            "Staking",
            "ErasStakersOverview",
            vec![Value::u128(era as u128)],
        );

        let mut exposures = Vec::new();
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

            // Key format: prefix + era (4 bytes) + account (32 bytes)
            if key_bytes.len() < 32 {
                continue;
            }
            let account_bytes: [u8; 32] = key_bytes[key_bytes.len() - 32..]
                .try_into()
                .map_err(|_| ChainError::InvalidData("Invalid account key".into()))?;
            let address = AccountId32::from(account_bytes);

            let decoded = value.to_value()?;

            // PagedExposureMetadata = { total: Balance, own: Balance, nominator_count: u32, page_count: u32 }
            let total = decoded
                .at("total")
                .and_then(|v: &Value<u32>| v.as_u128())
                .unwrap_or(0);
            let own = decoded
                .at("own")
                .and_then(|v: &Value<u32>| v.as_u128())
                .unwrap_or(0);
            let nominator_count = decoded
                .at("nominator_count")
                .and_then(|v: &Value<u32>| v.as_u128())
                .unwrap_or(0) as u32;

            exposures.push(ValidatorExposure {
                address,
                own,
                total,
                nominator_count,
            });
        }

        Ok(exposures)
    }

    /// Get total staked amount for an era (sum of all validator stakes).
    pub async fn get_era_total_staked(&self, era: EraIndex) -> Result<Balance, ChainError> {
        let exposures = self.get_era_stakers_overview(era).await?;
        Ok(exposures.iter().map(|e| e.total).sum())
    }
}
