//! Validator-related chain queries.

use super::decode_helpers::extract_account_id;
use crate::ChainClient;
use crate::error::ChainError;
use std::collections::HashMap;
use stkopt_core::{Balance, EraIndex, ValidatorPreferences};
use subxt::dynamic::{At, Value};
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

fn default_validator_preferences() -> ValidatorPreferences {
    ValidatorPreferences {
        commission: 0.0,
        blocked: false,
    }
}

fn parse_validator_preferences(decoded: &Value) -> ValidatorPreferences {
    let commission_perbill = decoded
        .at("commission")
        .and_then(|v: &Value| v.as_u128())
        .unwrap_or(0);
    // Perbill is parts per billion (1_000_000_000 = 100%).
    let commission = commission_perbill as f64 / 1_000_000_000.0;

    let blocked = decoded
        .at("blocked")
        .and_then(|v: &Value| v.as_bool())
        .unwrap_or(false);

    ValidatorPreferences {
        commission,
        blocked,
    }
}

impl ChainClient {
    /// Get all registered validators with their preferences.
    /// Note: This uses storage iteration which may be limited with light clients.
    /// Returns partial results if iteration is interrupted (e.g., connection drop).
    /// Deduplicates by address (light clients may return duplicates during iteration).
    /// For light client mode, prefer `get_validator_preferences_batch` with known addresses.
    pub async fn get_validators(&self) -> Result<Vec<ValidatorInfo>, ChainError> {
        let storage_query = subxt::dynamic::storage::<Vec<Value>, Value>("Staking", "Validators");

        // Use HashMap to deduplicate by address (light clients may return duplicates)
        let mut validators_map: HashMap<[u8; 32], ValidatorInfo> = HashMap::new();
        let block = self.client().at_current_block().await?;
        let iter_result = block.storage().iter(&storage_query, vec![]).await;

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
                    let key_bytes = kv.key_bytes();
                    let value = kv.value();

                    // Extract account ID from storage key (last 32 bytes)
                    if key_bytes.len() < 32 {
                        continue;
                    }
                    let Ok(account_bytes): Result<[u8; 32], _> =
                        key_bytes[key_bytes.len() - 32..].try_into()
                    else {
                        continue;
                    };

                    // Skip if we already have this validator
                    if validators_map.contains_key(&account_bytes) {
                        continue;
                    }

                    let address = AccountId32::from(account_bytes);

                    let Ok(decoded) = value.decode() else {
                        continue;
                    };

                    let preferences = parse_validator_preferences(&decoded);

                    validators_map.insert(
                        account_bytes,
                        ValidatorInfo {
                            address,
                            preferences,
                        },
                    );
                }
                Some(Err(e)) => {
                    // Connection error during iteration - return what we have so far
                    tracing::warn!(
                        "Validator iteration interrupted after {} entries: {}",
                        validators_map.len(),
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

        let validators: Vec<ValidatorInfo> = validators_map.into_values().collect();

        Ok(validators)
    }

    /// Get validator preferences for a single validator.
    pub async fn get_validator_preferences(
        &self,
        address: &AccountId32,
    ) -> Result<Option<ValidatorPreferences>, ChainError> {
        let storage_query = subxt::dynamic::storage::<Vec<Value>, Value>("Staking", "Validators");

        let block = self.client().at_current_block().await?;
        let result = block
            .storage()
            .try_fetch(&storage_query, vec![Value::from_bytes(address.clone())])
            .await?;

        let Some(value) = result else {
            return Ok(None);
        };

        let decoded: Value = value.decode()?;

        Ok(Some(parse_validator_preferences(&decoded)))
    }

    /// Get validator preferences for a batch of validators.
    /// More reliable than `get_validators()` for light clients since it fetches
    /// specific keys instead of iterating storage.
    pub async fn get_validator_preferences_batch(
        &self,
        addresses: &[AccountId32],
    ) -> Result<Vec<ValidatorInfo>, ChainError> {
        let mut validators = Vec::with_capacity(addresses.len());

        // Fetch in smaller batches to avoid overwhelming the connection.
        for chunk in addresses.chunks(50) {
            match self
                .batch_fetch_account_storage_values("Staking", "Validators", chunk)
                .await
            {
                Ok(values) => {
                    for address in chunk {
                        let preferences = values
                            .get(&crate::batch_storage::account_key(address))
                            .map(parse_validator_preferences)
                            .unwrap_or_else(default_validator_preferences);
                        validators.push(ValidatorInfo {
                            address: address.clone(),
                            preferences,
                        });
                    }
                }
                Err(error) => {
                    tracing::debug!(
                        "Batch validator preference fetch failed for {} addresses: {}",
                        chunk.len(),
                        error
                    );
                    for address in chunk {
                        match self.get_validator_preferences(address).await {
                            Ok(Some(preferences)) => {
                                validators.push(ValidatorInfo {
                                    address: address.clone(),
                                    preferences,
                                });
                            }
                            Ok(None) => {
                                validators.push(ValidatorInfo {
                                    address: address.clone(),
                                    preferences: default_validator_preferences(),
                                });
                            }
                            Err(e) => {
                                tracing::debug!(
                                    "Failed to get preferences for {} after batch fallback: {}",
                                    address,
                                    e
                                );
                                validators.push(ValidatorInfo {
                                    address: address.clone(),
                                    preferences: default_validator_preferences(),
                                });
                            }
                        }
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
        if !self.has_relay_connection() {
            return Err(ChainError::Connection(
                "Relay chain not connected. Session validators require a relay chain connection."
                    .into(),
            ));
        }

        // Session.Validators is on the relay chain, not Asset Hub
        let storage_query = subxt::dynamic::storage::<Vec<Value>, Value>("Session", "Validators");

        let block = self.relay_client().at_current_block().await?;
        let result = block.storage().try_fetch(&storage_query, vec![]).await?;

        let Some(value) = result else {
            tracing::warn!("Session.Validators storage is empty (relay chain)");
            return Ok(Vec::new());
        };

        let decoded: Value = value.decode()?;
        let mut validators = Vec::new();

        let mut index = 0;
        while let Some(account_val) = decoded.at(index) {
            if let Some(account) = extract_account_id(account_val) {
                validators.push(account);
            } else {
                tracing::debug!("Could not decode Session.Validators[{}]", index);
            }
            index += 1;
        }

        tracing::info!(
            "Session.Validators (relay chain) returned {} active validators",
            validators.len()
        );
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
            let map_size_before = all_addresses.len();
            match self.get_validators().await {
                Ok(vals) => {
                    let new_count = vals
                        .iter()
                        .filter(|v| !all_addresses.contains_key(&v.address.0))
                        .count();
                    for v in vals.iter() {
                        all_addresses.insert(v.address.0, v.address.clone());
                    }
                    let map_size_after = all_addresses.len();
                    tracing::info!(
                        "Iteration attempt {}/{}: Got {} validators, {} were new. Map: {} -> {} entries",
                        attempt,
                        max_attempts,
                        vals.len(),
                        new_count,
                        map_size_before,
                        map_size_after
                    );

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
            let new_count = exposures
                .iter()
                .filter(|e| !all_addresses.contains_key(&e.address.0))
                .count();
            tracing::info!("Era stakers: {} ({} new)", exposures.len(), new_count);
            for exp in exposures {
                all_addresses.insert(exp.address.0, exp.address);
            }
        }

        tracing::info!(
            "Total unique validators discovered: {}",
            all_addresses.len()
        );

        if all_addresses.is_empty() {
            return Err(ChainError::InvalidData(
                "No validators found with light client".into(),
            ));
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
        let storage_query =
            subxt::dynamic::storage::<Vec<Value>, Value>("Staking", "ErasRewardPoints");

        let block = self.client().at_current_block().await?;
        let result = block
            .storage()
            .try_fetch(&storage_query, vec![Value::u128(era as u128)])
            .await?;

        let Some(value) = result else {
            return Ok((0, Vec::new()));
        };

        let decoded: Value = value.decode()?;

        // EraRewardPoints = { total: u32, individual: BTreeMap<AccountId, u32> }
        let total = decoded
            .at("total")
            .and_then(|v: &Value| v.as_u128())
            .unwrap_or(0) as u32;

        tracing::debug!("EraRewardPoints total: {}", total);

        let mut validators = Vec::new();

        // individual is a BTreeMap<AccountId32, u32>, which decodes as a
        // Composite sequence of (key, value) tuple entries.
        if let Some(individual) = decoded.at("individual") {
            let mut i = 0;
            while let Some(entry) = individual.at(i) {
                i += 1;
                // Each entry is a tuple (AccountId32, u32)
                let account_id = entry.at(0).and_then(extract_account_id);
                let points = entry
                    .at(1)
                    .and_then(|v: &Value| v.as_u128())
                    .map(|n| n as u32);

                if let (Some(address), Some(points)) = (account_id, points) {
                    validators.push(ValidatorPoints { address, points });
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
        let storage_query =
            subxt::dynamic::storage::<Vec<Value>, Value>("Staking", "ErasValidatorReward");

        let block = self.client().at_current_block().await?;
        let result = block
            .storage()
            .try_fetch(&storage_query, vec![Value::u128(era as u128)])
            .await?;

        Ok(result
            .and_then(|v| v.decode().ok())
            .and_then(|v| v.as_u128()))
    }

    /// Get total staked amount for an era from the dedicated `ErasTotalStake` storage item.
    ///
    /// This is a direct point query (no iteration), so it works reliably with light clients.
    /// Prefer this over summing from `get_era_stakers_overview`, which may return partial results.
    pub async fn get_era_total_stake_direct(&self, era: EraIndex) -> Result<Balance, ChainError> {
        let storage_query =
            subxt::dynamic::storage::<Vec<Value>, Value>("Staking", "ErasTotalStake");

        let block = self.client().at_current_block().await?;
        let result = block
            .storage()
            .try_fetch(&storage_query, vec![Value::u128(era as u128)])
            .await?;

        Ok(result
            .and_then(|v| v.decode().ok())
            .and_then(|v| v.as_u128())
            .unwrap_or(0))
    }

    /// Get staking exposure for validators in an era (using ErasStakersOverview).
    /// Returns partial results if iteration is interrupted (e.g., connection drop).
    /// Deduplicates by address (light clients may return duplicates during iteration).
    /// Retries up to 3 times on light client storage query failures.
    pub async fn get_era_stakers_overview(
        &self,
        era: EraIndex,
    ) -> Result<Vec<ValidatorExposure>, ChainError> {
        // Use HashMap to deduplicate by address (light clients may return duplicates)
        let mut exposures_map: HashMap<[u8; 32], ValidatorExposure> = HashMap::new();

        // Retry loop for light client failures
        for attempt in 0..3 {
            // For iterating with a partial key, we need to use the era as the first key
            let storage_query =
                subxt::dynamic::storage::<Vec<Value>, Value>("Staking", "ErasStakersOverview");

            let block = self.client().at_current_block().await?;
            let iter_result = block
                .storage()
                .iter(&storage_query, vec![Value::u128(era as u128)])
                .await;

            let mut iter = match iter_result {
                Ok(iter) => iter,
                Err(e) => {
                    if attempt < 2 {
                        tracing::debug!(
                            "Failed to start era stakers iteration (attempt {}), retrying: {}",
                            attempt + 1,
                            e
                        );
                        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
                        continue;
                    }
                    tracing::warn!(
                        "Failed to start era stakers iteration after 3 attempts: {}",
                        e
                    );
                    return Err(e.into());
                }
            };

            let mut iteration_failed = false;
            loop {
                match iter.next().await {
                    Some(Ok(kv)) => {
                        let key_bytes = kv.key_bytes();
                        let value = kv.value();

                        // Key format: prefix + era (4 bytes) + account (32 bytes)
                        if key_bytes.len() < 32 {
                            continue;
                        }
                        let Ok(account_bytes): Result<[u8; 32], _> =
                            key_bytes[key_bytes.len() - 32..].try_into()
                        else {
                            continue;
                        };

                        // Skip if we already have this validator
                        if exposures_map.contains_key(&account_bytes) {
                            continue;
                        }

                        let address = AccountId32::from(account_bytes);

                        let Ok(decoded) = value.decode() else {
                            continue;
                        };

                        // PagedExposureMetadata = { total: Balance, own: Balance, nominator_count: u32, page_count: u32 }
                        let total = decoded
                            .at("total")
                            .and_then(|v: &Value| v.as_u128())
                            .unwrap_or(0);
                        let own = decoded
                            .at("own")
                            .and_then(|v: &Value| v.as_u128())
                            .unwrap_or(0);
                        let nominator_count = decoded
                            .at("nominator_count")
                            .and_then(|v: &Value| v.as_u128())
                            .unwrap_or(0) as u32;

                        exposures_map.insert(
                            account_bytes,
                            ValidatorExposure {
                                address,
                                own,
                                total,
                                nominator_count,
                            },
                        );
                    }
                    Some(Err(e)) => {
                        // Connection error during iteration
                        if attempt < 2 {
                            tracing::debug!(
                                "Era stakers iteration interrupted after {} entries (attempt {}), retrying: {}",
                                exposures_map.len(),
                                attempt + 1,
                                e
                            );
                            iteration_failed = true;
                            break;
                        }
                        tracing::warn!(
                            "Era stakers iteration interrupted after {} entries (final attempt): {}",
                            exposures_map.len(),
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

            if iteration_failed {
                tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
                continue;
            }

            // Iteration completed (successfully or with partial results on final attempt)
            break;
        }

        let exposures: Vec<ValidatorExposure> = exposures_map.into_values().collect();
        Ok(exposures)
    }

    /// Get total staked amount for an era (sum of all validator stakes).
    pub async fn get_era_total_staked(&self, era: EraIndex) -> Result<Balance, ChainError> {
        let exposures = self.get_era_stakers_overview(era).await?;
        Ok(exposures.iter().map(|e| e.total).sum())
    }
}
