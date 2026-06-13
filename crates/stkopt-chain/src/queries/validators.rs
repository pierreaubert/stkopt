//! Validator-related chain queries.

use super::decode_helpers::extract_account_id;
use crate::ChainClient;
use crate::error::ChainError;
use std::collections::HashMap;
use stkopt_core::{Balance, EraIndex, ValidatorPreferences};
use subxt::dynamic::{At, Value};
use subxt::ext::scale_value::{Composite, ValueDef};
use subxt::utils::AccountId32;

const MIN_APY_EXPOSURE_COVERAGE_NUMERATOR: usize = 1;
const MIN_APY_EXPOSURE_COVERAGE_DENOMINATOR: usize = 2;

/// Raw validator data from chain.
#[derive(Debug, Clone)]
pub struct ValidatorInfo {
    pub address: AccountId32,
    pub preferences: ValidatorPreferences,
}

/// Validator fetch result with completeness metadata.
#[derive(Debug, Clone)]
pub struct ValidatorFetch {
    pub validators: Vec<ValidatorInfo>,
    pub complete: bool,
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

/// Chain data needed to calculate per-validator APY for one complete era.
#[derive(Debug, Clone)]
pub struct ValidatorApyData {
    pub era: EraIndex,
    pub era_reward: Balance,
    pub total_points: u32,
    pub points: Vec<ValidatorPoints>,
    pub exposures: Vec<ValidatorExposure>,
}

fn default_validator_preferences() -> ValidatorPreferences {
    ValidatorPreferences {
        commission: 0.0,
        blocked: false,
    }
}

fn extract_validator_points_account_id(value: &Value) -> Option<AccountId32> {
    if let Some(address) = extract_account_id(value) {
        return Some(address);
    }

    let mut bytes = Vec::with_capacity(34);
    collect_u8_primitives(value, &mut bytes);
    let account_bytes: [u8; 32] = bytes
        .get(bytes.len().saturating_sub(32)..)?
        .try_into()
        .ok()?;
    Some(AccountId32::from(account_bytes))
}

fn collect_u8_primitives(value: &Value, bytes: &mut Vec<u8>) {
    match &value.value {
        ValueDef::Primitive(_) => {
            if let Some(byte) = value.as_u128()
                && byte <= u8::MAX as u128
            {
                bytes.push(byte as u8);
            }
        }
        ValueDef::Composite(Composite::Unnamed(values)) => {
            for child in values {
                collect_u8_primitives(child, bytes);
            }
        }
        ValueDef::Composite(Composite::Named(values)) => {
            for (_, child) in values {
                collect_u8_primitives(child, bytes);
            }
        }
        ValueDef::Variant(variant) => match &variant.values {
            Composite::Unnamed(values) => {
                for child in values {
                    collect_u8_primitives(child, bytes);
                }
            }
            Composite::Named(values) => {
                for (_, child) in values {
                    collect_u8_primitives(child, bytes);
                }
            }
        },
        ValueDef::BitSequence(_) => {}
    }
}

fn parse_validator_points_entry(entry: &Value) -> Option<ValidatorPoints> {
    let address = entry
        .at(0)
        .or_else(|| entry.at("0"))
        .and_then(extract_validator_points_account_id)?;
    let points = entry
        .at(1)
        .or_else(|| entry.at("1"))
        .and_then(|v: &Value| v.as_u128())
        .map(|n| n as u32)?;

    Some(ValidatorPoints { address, points })
}

fn parse_wrapped_validator_points_entry(entry: &Value) -> Option<ValidatorPoints> {
    if let Some(points) = parse_validator_points_entry(entry) {
        return Some(points);
    }

    match &entry.value {
        ValueDef::Composite(Composite::Unnamed(values)) if values.len() == 1 => {
            parse_wrapped_validator_points_entry(&values[0])
        }
        ValueDef::Composite(Composite::Named(values)) if values.len() == 1 => {
            parse_wrapped_validator_points_entry(&values[0].1)
        }
        _ => None,
    }
}

fn parse_validator_points_entries(value: &Value) -> Vec<ValidatorPoints> {
    let mut points = Vec::new();
    collect_validator_points_entries(value, &mut points);
    points
}

fn collect_validator_points_entries(value: &Value, points: &mut Vec<ValidatorPoints>) {
    if let Some(entry) = parse_wrapped_validator_points_entry(value) {
        points.push(entry);
        return;
    }

    match &value.value {
        ValueDef::Composite(Composite::Unnamed(values)) => {
            for value in values {
                collect_validator_points_entries(value, points);
            }
        }
        ValueDef::Composite(Composite::Named(values)) => {
            for (_, value) in values {
                collect_validator_points_entries(value, points);
            }
        }
        ValueDef::Variant(variant) => match &variant.values {
            Composite::Unnamed(values) => {
                for value in values {
                    collect_validator_points_entries(value, points);
                }
            }
            Composite::Named(values) => {
                for (_, value) in values {
                    collect_validator_points_entries(value, points);
                }
            }
        },
        ValueDef::Primitive(_) | ValueDef::BitSequence(_) => {}
    }
}

fn has_sufficient_apy_exposure_coverage(points_len: usize, exposures_len: usize) -> bool {
    if points_len == 0 || exposures_len == 0 {
        return false;
    }

    let required = (points_len * MIN_APY_EXPOSURE_COVERAGE_NUMERATOR)
        .div_ceil(MIN_APY_EXPOSURE_COVERAGE_DENOMINATOR)
        .max(1);
    exposures_len >= required
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
    async fn get_validators_iterated(
        &self,
        allow_partial: bool,
    ) -> Result<ValidatorFetch, ChainError> {
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

        let mut complete = true;
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
                    complete = false;
                    tracing::warn!(
                        "Validator iteration interrupted after {} entries: {}",
                        validators_map.len(),
                        e
                    );
                    if !allow_partial {
                        return Err(ChainError::InvalidData(format!(
                            "Validator iteration interrupted after {} entries: {}",
                            validators_map.len(),
                            e
                        )));
                    }
                    break;
                }
                None => {
                    break;
                }
            }
        }

        Ok(ValidatorFetch {
            validators: validators_map.into_values().collect(),
            complete,
        })
    }

    /// Get all registered validators with their preferences.
    /// Note: This uses storage iteration which may be limited with light clients.
    /// Returns an error if iteration is interrupted instead of returning partial
    /// data that could poison the cache.
    /// Deduplicates by address (light clients may return duplicates during iteration).
    /// For light client mode, prefer `get_validator_preferences_batch` with known addresses.
    pub async fn get_validators(&self) -> Result<Vec<ValidatorInfo>, ChainError> {
        Ok(self.get_validators_iterated(false).await?.validators)
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
        Ok(self
            .get_validators_light_client_with_completeness()
            .await?
            .validators)
    }

    /// Get validators using the light-client approach and report whether the
    /// result came from a complete storage iteration.
    pub async fn get_validators_light_client_with_completeness(
        &self,
    ) -> Result<ValidatorFetch, ChainError> {
        // Use HashMap with address bytes as key since AccountId32 doesn't impl Hash
        let mut all_addresses: HashMap<[u8; 32], AccountId32> = HashMap::new();
        let mut complete = false;

        // Try multiple iterations - light client may return different partial results each time
        // Collect the union of all results for better coverage
        let max_attempts = 3;
        for attempt in 1..=max_attempts {
            let map_size_before = all_addresses.len();
            match self.get_validators_iterated(true).await {
                Ok(fetch) => {
                    let vals = fetch.validators;
                    complete |= fetch.complete;
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
        Ok(ValidatorFetch {
            validators: self.get_validator_preferences_batch(&addresses).await?,
            complete,
        })
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

        let validators = decoded
            .at("individual")
            .map(parse_validator_points_entries)
            .unwrap_or_default();

        Ok((total, validators))
    }

    /// Find the most recent completed era with enough chain data to calculate
    /// per-validator APY.
    pub async fn get_recent_validator_apy_data(
        &self,
        latest_completed_era: EraIndex,
        max_lookback: u32,
    ) -> Result<Option<ValidatorApyData>, ChainError> {
        let max_lookback = max_lookback.max(1);
        let earliest_era = latest_completed_era.saturating_sub(max_lookback.saturating_sub(1));

        for era in (earliest_era..=latest_completed_era).rev() {
            let era_reward = match self.get_era_validator_reward(era).await {
                Ok(Some(era_reward)) if era_reward > 0 => era_reward,
                Ok(Some(_)) => {
                    tracing::debug!("Era {} validator reward is zero; trying older era", era);
                    continue;
                }
                Ok(None) => {
                    tracing::debug!("Era {} has no validator reward; trying older era", era);
                    continue;
                }
                Err(error) => {
                    tracing::debug!(
                        "Failed to fetch era {} validator reward: {}; trying older era",
                        era,
                        error
                    );
                    continue;
                }
            };

            let (total_points, points) = match self.get_era_reward_points(era).await {
                Ok(points) => points,
                Err(error) => {
                    tracing::debug!(
                        "Failed to fetch era {} reward points: {}; trying older era",
                        era,
                        error
                    );
                    continue;
                }
            };
            if total_points == 0 || points.is_empty() {
                tracing::debug!(
                    "Era {} has no reward points (total={}, validators={}); trying older era",
                    era,
                    total_points,
                    points.len()
                );
                continue;
            }

            let exposures = match self.get_era_stakers_overview(era).await {
                Ok(exposures) => exposures,
                Err(error) => {
                    tracing::debug!(
                        "Failed to fetch era {} staker exposures: {}; trying older era",
                        era,
                        error
                    );
                    continue;
                }
            };
            if !has_sufficient_apy_exposure_coverage(points.len(), exposures.len()) {
                tracing::debug!(
                    "Era {} has insufficient staker exposure coverage (points={}, exposures={}); trying older era",
                    era,
                    points.len(),
                    exposures.len()
                );
                continue;
            }

            tracing::info!(
                "Using era {} for validator APY (reward={}, total_points={}, points={}, exposures={})",
                era,
                era_reward,
                total_points,
                points.len(),
                exposures.len()
            );
            return Ok(Some(ValidatorApyData {
                era,
                era_reward,
                total_points,
                points,
                exposures,
            }));
        }

        tracing::warn!(
            "No validator APY data found in eras {}..={}",
            earliest_era,
            latest_completed_era
        );
        Ok(None)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_validator_preferences_returns_zero_commission_and_not_blocked() {
        let prefs = default_validator_preferences();
        assert_eq!(prefs.commission, 0.0);
        assert!(!prefs.blocked);
    }

    #[test]
    fn test_parse_validator_preferences_typical_values() {
        let decoded = subxt::dynamic::Value::named_composite([
            ("commission", subxt::dynamic::Value::u128(150_000_000)),
            ("blocked", subxt::dynamic::Value::bool(false)),
        ]);
        let prefs = parse_validator_preferences(&decoded);
        assert_eq!(prefs.commission, 150_000_000.0 / 1_000_000_000.0);
        assert!(!prefs.blocked);
    }

    #[test]
    fn test_parse_validator_preferences_zero_commission_and_blocked() {
        let decoded = subxt::dynamic::Value::named_composite([
            ("commission", subxt::dynamic::Value::u128(0)),
            ("blocked", subxt::dynamic::Value::bool(true)),
        ]);
        let prefs = parse_validator_preferences(&decoded);
        assert_eq!(prefs.commission, 0.0);
        assert!(prefs.blocked);
    }

    #[test]
    fn test_parse_validator_preferences_max_commission() {
        let decoded = subxt::dynamic::Value::named_composite([
            ("commission", subxt::dynamic::Value::u128(1_000_000_000)),
            ("blocked", subxt::dynamic::Value::bool(false)),
        ]);
        let prefs = parse_validator_preferences(&decoded);
        assert_eq!(prefs.commission, 1.0);
        assert!(!prefs.blocked);
    }

    #[test]
    fn test_parse_validator_preferences_missing_fields_defaults() {
        let decoded =
            subxt::dynamic::Value::named_composite::<&str, [(&str, subxt::dynamic::Value); 0]>([]);
        let prefs = parse_validator_preferences(&decoded);
        assert_eq!(prefs.commission, 0.0);
        assert!(!prefs.blocked);
    }

    #[test]
    fn test_parse_validator_preferences_missing_commission() {
        let decoded = subxt::dynamic::Value::named_composite([(
            "blocked",
            subxt::dynamic::Value::bool(true),
        )]);
        let prefs = parse_validator_preferences(&decoded);
        assert_eq!(prefs.commission, 0.0);
        assert!(prefs.blocked);
    }

    #[test]
    fn test_parse_validator_preferences_missing_blocked() {
        let decoded = subxt::dynamic::Value::named_composite([(
            "commission",
            subxt::dynamic::Value::u128(50_000_000),
        )]);
        let prefs = parse_validator_preferences(&decoded);
        assert_eq!(prefs.commission, 50_000_000.0 / 1_000_000_000.0);
        assert!(!prefs.blocked);
    }

    #[test]
    fn test_parse_validator_preferences_fractional_commission() {
        let decoded = subxt::dynamic::Value::named_composite([
            ("commission", subxt::dynamic::Value::u128(1)),
            ("blocked", subxt::dynamic::Value::bool(false)),
        ]);
        let prefs = parse_validator_preferences(&decoded);
        assert_eq!(prefs.commission, 1.0 / 1_000_000_000.0);
        assert!(!prefs.blocked);
    }

    #[test]
    fn test_parse_validator_points_entries_unnamed_map_entries() {
        let account = [7u8; 32];
        let individual = subxt::dynamic::Value::unnamed_composite(vec![
            subxt::dynamic::Value::unnamed_composite(vec![
                subxt::dynamic::Value::from_bytes(account),
                subxt::dynamic::Value::u128(42),
            ]),
        ]);

        let points = parse_validator_points_entries(&individual);

        assert_eq!(points.len(), 1);
        assert_eq!(points[0].address, AccountId32::from(account));
        assert_eq!(points[0].points, 42);
    }

    #[test]
    fn test_parse_validator_points_entries_named_map_entries() {
        let account = [9u8; 32];
        let individual =
            subxt::dynamic::Value::unnamed_composite(vec![subxt::dynamic::Value::named_composite(
                [
                    ("0", subxt::dynamic::Value::from_bytes(account)),
                    ("1", subxt::dynamic::Value::u128(123)),
                ],
            )]);

        let points = parse_validator_points_entries(&individual);

        assert_eq!(points.len(), 1);
        assert_eq!(points[0].address, AccountId32::from(account));
        assert_eq!(points[0].points, 123);
    }

    #[test]
    fn test_parse_validator_points_entries_wrapped_map_entries() {
        fn wrap(value: subxt::dynamic::Value, depth: usize) -> subxt::dynamic::Value {
            (0..depth).fold(value, |value, _| {
                subxt::dynamic::Value::unnamed_composite(vec![value])
            })
        }

        let account = [11u8; 32];
        let second_account = [12u8; 32];
        let prefixed_account = subxt::dynamic::Value::unnamed_composite(vec![
            subxt::dynamic::Value::u128(0),
            subxt::dynamic::Value::u128(0),
            subxt::dynamic::Value::from_bytes(account),
        ]);
        let second_prefixed_account = subxt::dynamic::Value::unnamed_composite(vec![
            subxt::dynamic::Value::u128(0),
            subxt::dynamic::Value::u128(0),
            subxt::dynamic::Value::from_bytes(second_account),
        ]);
        let entry = wrap(
            subxt::dynamic::Value::unnamed_composite(vec![
                wrap(prefixed_account, 4),
                subxt::dynamic::Value::u128(84_940),
            ]),
            3,
        );
        let second_entry = wrap(
            subxt::dynamic::Value::unnamed_composite(vec![
                wrap(second_prefixed_account, 1),
                subxt::dynamic::Value::u128(83_340),
            ]),
            1,
        );
        let individual = subxt::dynamic::Value::unnamed_composite(vec![
            subxt::dynamic::Value::unnamed_composite(vec![entry, second_entry]),
        ]);

        let points = parse_validator_points_entries(&individual);

        assert_eq!(points.len(), 2);
        assert_eq!(points[0].address, AccountId32::from(account));
        assert_eq!(points[0].points, 84_940);
        assert_eq!(points[1].address, AccountId32::from(second_account));
        assert_eq!(points[1].points, 83_340);
    }

    #[test]
    fn test_has_sufficient_apy_exposure_coverage_rejects_sparse_data() {
        assert!(!has_sufficient_apy_exposure_coverage(100, 0));
        assert!(!has_sufficient_apy_exposure_coverage(100, 49));
    }

    #[test]
    fn test_has_sufficient_apy_exposure_coverage_accepts_half_or_better() {
        assert!(has_sufficient_apy_exposure_coverage(100, 50));
        assert!(has_sufficient_apy_exposure_coverage(100, 100));
        assert!(has_sufficient_apy_exposure_coverage(1, 1));
    }
}
