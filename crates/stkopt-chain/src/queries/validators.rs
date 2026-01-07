//! Validator-related chain queries.

use crate::ChainClient;
use crate::error::ChainError;
use stkopt_core::{Balance, EraIndex, ValidatorPreferences};
use subxt::dynamic::{At, DecodedValueThunk, Value};
use subxt::utils::AccountId32;
use std::collections::HashMap;

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
    /// Note: This uses storage iteration which may be limited with light clients.
    /// Returns partial results if iteration is interrupted (e.g., connection drop).
    /// For light client mode, prefer `get_validator_preferences_batch` with known addresses.
    pub async fn get_validators(&self) -> Result<Vec<ValidatorInfo>, ChainError> {
        let storage_query = subxt::dynamic::storage("Staking", "Validators", ());

        let mut validators = Vec::new();
        let iter_result = self
            .client()
            .storage()
            .at_latest()
            .await?
            .iter(storage_query)
            .await;

        let mut iter = match iter_result {
            Ok(iter) => iter,
            Err(e) => {
                tracing::warn!("Failed to start validator iteration: {}", e);
                return Err(e.into());
            }
        };

        loop {
            match iter.next().await {
                Some(Ok(kv)) => {
                    let key_bytes = kv.key_bytes;
                    let value: DecodedValueThunk = kv.value;

                    // Extract account ID from storage key (last 32 bytes)
                    if key_bytes.len() < 32 {
                        continue;
                    }
                    let Ok(account_bytes): Result<[u8; 32], _> = key_bytes[key_bytes.len() - 32..]
                        .try_into() else {
                        continue;
                    };
                    let address = AccountId32::from(account_bytes);

                    let Ok(decoded) = value.to_value() else {
                        continue;
                    };

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
                Some(Err(e)) => {
                    // Connection error during iteration - return what we have so far
                    tracing::warn!(
                        "Validator iteration interrupted after {} entries: {}",
                        validators.len(),
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

        if validators.is_empty() {
            Err(ChainError::InvalidData("No validators found".into()))
        } else {
            Ok(validators)
        }
    }

    /// Get validator preferences for a single validator.
    pub async fn get_validator_preferences(
        &self,
        address: &AccountId32,
    ) -> Result<Option<ValidatorPreferences>, ChainError> {
        let storage_query = subxt::dynamic::storage(
            "Staking",
            "Validators",
            vec![Value::from_bytes(address.clone())],
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

        let commission_perbill = decoded
            .at("commission")
            .and_then(|v: &Value<u32>| v.as_u128())
            .unwrap_or(0);
        let commission = commission_perbill as f64 / 1_000_000_000.0;

        let blocked = decoded
            .at("blocked")
            .and_then(|v: &Value<u32>| v.as_bool())
            .unwrap_or(false);

        Ok(Some(ValidatorPreferences { commission, blocked }))
    }

    /// Get validator preferences for a batch of validators.
    /// More reliable than `get_validators()` for light clients since it fetches
    /// specific keys instead of iterating storage.
    pub async fn get_validator_preferences_batch(
        &self,
        addresses: &[AccountId32],
    ) -> Result<Vec<ValidatorInfo>, ChainError> {
        let mut validators = Vec::with_capacity(addresses.len());

        // Fetch in smaller batches to avoid overwhelming the connection
        for chunk in addresses.chunks(50) {
            for address in chunk {
                match self.get_validator_preferences(address).await {
                    Ok(Some(preferences)) => {
                        validators.push(ValidatorInfo {
                            address: address.clone(),
                            preferences,
                        });
                    }
                    Ok(None) => {
                        // Validator not found, use defaults
                        validators.push(ValidatorInfo {
                            address: address.clone(),
                            preferences: ValidatorPreferences {
                                commission: 0.0,
                                blocked: false,
                            },
                        });
                    }
                    Err(e) => {
                        tracing::debug!("Failed to get preferences for {}: {}", address, e);
                        // Use defaults on error
                        validators.push(ValidatorInfo {
                            address: address.clone(),
                            preferences: ValidatorPreferences {
                                commission: 0.0,
                                blocked: false,
                            },
                        });
                    }
                }
            }
            // Small delay between batches
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }

        Ok(validators)
    }

    /// Get the current session validators (active validator set).
    /// This is a single storage value (Vec<AccountId>) that works well with light clients.
    /// Returns only addresses, not preferences.
    ///
    /// Note: Session.Validators lives on the relay chain, not Asset Hub.
    /// This method queries the relay chain if available.
    pub async fn get_session_validators(&self) -> Result<Vec<AccountId32>, ChainError> {
        // Session.Validators is on the relay chain, not Asset Hub
        let storage_query = subxt::dynamic::storage("Session", "Validators", ());

        let result: Option<DecodedValueThunk> = self
            .relay_client()  // Use relay chain, not Asset Hub
            .storage()
            .at_latest()
            .await?
            .fetch(&storage_query)
            .await?;

        let Some(value) = result else {
            tracing::warn!("Session.Validators storage is empty (relay chain)");
            return Ok(Vec::new());
        };

        let decoded = value.to_value()?;
        let mut validators = Vec::new();

        // Session.Validators is a Vec<AccountId32>
        for i in 0..2000 {
            if let Some(account_val) = decoded.at(i) {
                let mut bytes = Vec::with_capacity(32);
                for k in 0..32 {
                    if let Some(b_val) = account_val.at(k)
                        && let Some(b) = b_val.as_u128()
                    {
                        bytes.push(b as u8);
                    }
                }
                if bytes.len() == 32 {
                    let arr: [u8; 32] = bytes.try_into().unwrap();
                    validators.push(AccountId32::from(arr));
                }
            } else {
                break;
            }
        }

        tracing::info!("Session.Validators (relay chain) returned {} active validators", validators.len());
        Ok(validators)
    }

    /// Get validators using a light-client-friendly approach.
    ///
    /// Light clients have fundamental limitations with iterating large storage maps.
    /// This method tries multiple iteration attempts and combines results, since
    /// each attempt may return different partial data.
    ///
    /// For staking decisions, partial data from active validators is often sufficient.
    pub async fn get_validators_light_client(&self) -> Result<Vec<ValidatorInfo>, ChainError> {
        // Use HashMap with address bytes as key since AccountId32 doesn't impl Hash
        let mut all_addresses: HashMap<[u8; 32], AccountId32> = HashMap::new();

        // Try multiple iterations - light client may return different partial results each time
        // Collect the union of all results for better coverage
        let max_attempts = 3;
        for attempt in 1..=max_attempts {
            match self.get_validators().await {
                Ok(vals) => {
                    let new_count = vals.iter().filter(|v| !all_addresses.contains_key(&v.address.0)).count();
                    tracing::info!(
                        "Iteration attempt {}/{}: Got {} validators ({} new, {} total so far)",
                        attempt, max_attempts, vals.len(), new_count, all_addresses.len() + new_count
                    );
                    for v in vals {
                        all_addresses.insert(v.address.0, v.address);
                    }

                    // If we didn't get any new validators, probably have reached the limit
                    if new_count == 0 && !all_addresses.is_empty() {
                        break;
                    }
                }
                Err(e) => {
                    tracing::warn!("Iteration attempt {} failed: {}", attempt, e);
                }
            }

            // Small delay between attempts
            if attempt < max_attempts {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
        }

        // Also try era stakers as another source
        if let Ok(Some(era_info)) = self.get_active_era().await
            && let Ok(exposures) = self.get_era_stakers_overview(era_info.index).await
        {
            let new_count = exposures.iter().filter(|e| !all_addresses.contains_key(&e.address.0)).count();
            tracing::info!("Era stakers: {} ({} new)", exposures.len(), new_count);
            for exp in exposures {
                all_addresses.insert(exp.address.0, exp.address);
            }
        }

        tracing::info!("Total unique validators discovered: {}", all_addresses.len());

        if all_addresses.is_empty() {
            return Err(ChainError::InvalidData("No validators found with light client".into()));
        }

        // Batch fetch preferences for all discovered validators
        let addresses: Vec<AccountId32> = all_addresses.into_values().collect();
        self.get_validator_preferences_batch(&addresses).await
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
    /// Returns partial results if iteration is interrupted (e.g., connection drop).
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
        let iter_result = self
            .client()
            .storage()
            .at_latest()
            .await?
            .iter(storage_query)
            .await;

        let mut iter = match iter_result {
            Ok(iter) => iter,
            Err(e) => {
                tracing::warn!("Failed to start era stakers iteration: {}", e);
                return Err(e.into());
            }
        };

        loop {
            match iter.next().await {
                Some(Ok(kv)) => {
                    let key_bytes = kv.key_bytes;
                    let value: DecodedValueThunk = kv.value;

                    // Key format: prefix + era (4 bytes) + account (32 bytes)
                    if key_bytes.len() < 32 {
                        continue;
                    }
                    let Ok(account_bytes): Result<[u8; 32], _> = key_bytes[key_bytes.len() - 32..]
                        .try_into() else {
                        continue;
                    };
                    let address = AccountId32::from(account_bytes);

                    let Ok(decoded) = value.to_value() else {
                        continue;
                    };

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
                Some(Err(e)) => {
                    // Connection error during iteration - return what we have so far
                    tracing::warn!(
                        "Era stakers iteration interrupted after {} entries: {}",
                        exposures.len(),
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

        Ok(exposures)
    }

    /// Get total staked amount for an era (sum of all validator stakes).
    pub async fn get_era_total_staked(&self, era: EraIndex) -> Result<Balance, ChainError> {
        let exposures = self.get_era_stakers_overview(era).await?;
        Ok(exposures.iter().map(|e| e.total).sum())
    }
}
