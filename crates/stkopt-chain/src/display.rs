//! Shared chain-to-display enrichment helpers.
//!
//! These helpers keep staking, APY, pool, and history calculations out of the
//! frontend crates while returning the shared display types from `stkopt-core`.

use crate::{
    PoolInfo as ChainPoolInfo, PoolMetadata, PoolNominations, ValidatorApyData, ValidatorIdentity,
    ValidatorInfo as ChainValidatorInfo,
};
#[cfg(test)]
use crate::{ValidatorExposure, ValidatorPoints};
use std::collections::HashMap;
use stkopt_core::display::{DisplayPool, DisplayValidator, StakingHistoryPoint};
use stkopt_core::{Balance, get_era_apy};
use subxt::utils::AccountId32;

/// Maximum realistic APY (50%). Higher values indicate incomplete/corrupt data.
pub const MAX_REALISTIC_APY: f64 = 0.50;

/// Default recent-era window for finding complete validator APY data.
pub const DEFAULT_VALIDATOR_APY_LOOKBACK_ERAS: u32 = 7;

const MS_PER_DAY: u64 = 24 * 60 * 60 * 1000;
const MAX_REWARD_FRACTION: u128 = 200;

/// Summary for validator display enrichment.
#[derive(Debug, Clone)]
pub struct DisplayValidatorEnrichment {
    pub validators: Vec<DisplayValidator>,
    pub apy_era: u32,
    pub validators_with_apy: usize,
}

/// Check if an APY value is realistic enough to display/cache.
pub fn is_realistic_apy(apy: f64) -> bool {
    apy.is_finite() && (0.0..=MAX_REALISTIC_APY).contains(&apy)
}

/// Convert a history lookback window in days to the number of eras to query.
pub fn eras_for_lookback_days(lookback_days: u32, era_duration_ms: u64) -> u32 {
    if lookback_days == 0 || era_duration_ms == 0 {
        return 1;
    }
    let lookback_ms = lookback_days as u64 * MS_PER_DAY;
    lookback_ms.div_ceil(era_duration_ms).max(1) as u32
}

/// Return validators whose identity names are not already cached.
pub fn missing_validator_identity_addresses(
    validators: &[ChainValidatorInfo],
    identity_map: &HashMap<String, String>,
) -> Vec<AccountId32> {
    validators
        .iter()
        .filter(|validator| !identity_map.contains_key(&validator.address.to_string()))
        .map(|validator| validator.address)
        .collect()
}

/// Convert People-chain identity records into the display-name cache shape.
pub fn validator_identity_display_map(
    identities: Vec<ValidatorIdentity>,
) -> HashMap<String, String> {
    identities
        .into_iter()
        .filter_map(|identity| {
            identity
                .display_name
                .map(|name| (identity.address.to_string(), name))
        })
        .collect()
}

/// Convert raw validators without APY/exposure data.
pub fn basic_display_validators(validators: &[ChainValidatorInfo]) -> Vec<DisplayValidator> {
    validators
        .iter()
        .map(|validator| DisplayValidator {
            address: validator.address.to_string(),
            name: None,
            commission: validator.preferences.commission,
            blocked: validator.preferences.blocked,
            total_stake: 0,
            own_stake: 0,
            nominator_count: 0,
            points: 0,
            apy: None,
        })
        .collect()
}

/// Calculate a validator's nominator reward from era-level data using integer
/// arithmetic until the final APY step.
fn calculate_nominator_reward(
    era_reward: Balance,
    points: u32,
    total_points: u32,
    commission: f64,
) -> Balance {
    if total_points == 0 || points == 0 {
        return 0;
    }

    let points = points as u128;
    let total_points = total_points as u128;
    let validator_share = era_reward
        .checked_mul(points)
        .and_then(|product| product.checked_div(total_points))
        .unwrap_or_else(|| era_reward / total_points * points);

    let commission_factor = ((1.0 - commission.clamp(0.0, 1.0)) * 1_000_000_000.0) as u128;
    validator_share
        .checked_mul(commission_factor)
        .and_then(|product| product.checked_div(1_000_000_000))
        .unwrap_or_else(|| validator_share / 1_000_000_000 * commission_factor)
}

/// Enrich raw validators with identity, exposure, points, and APY data.
pub fn enrich_display_validators(
    validators: &[ChainValidatorInfo],
    identity_map: &HashMap<String, String>,
    validator_apy_data: Option<&ValidatorApyData>,
    fallback_apy_era: u32,
    era_duration_ms: u64,
) -> DisplayValidatorEnrichment {
    let apy_era = validator_apy_data
        .map(|data| data.era)
        .unwrap_or(fallback_apy_era);
    let exposures = validator_apy_data
        .map(|data| data.exposures.as_slice())
        .unwrap_or(&[]);
    let exposure_map: HashMap<[u8; 32], _> = exposures
        .iter()
        .map(|exposure| (*exposure.address.as_ref(), exposure))
        .collect();
    let points_map: HashMap<[u8; 32], u32> = validator_apy_data
        .map(|data| {
            data.points
                .iter()
                .map(|points| (*points.address.as_ref(), points.points))
                .collect()
        })
        .unwrap_or_default();
    let era_reward = validator_apy_data.map(|data| data.era_reward).unwrap_or(0);
    let total_points = validator_apy_data
        .map(|data| data.total_points)
        .unwrap_or(0);

    let mut display_validators: Vec<DisplayValidator> = validators
        .iter()
        .map(|validator| {
            let addr_bytes: [u8; 32] = *validator.address.as_ref();
            let exposure = exposure_map.get(&addr_bytes);
            let (total_stake, own_stake, nominator_count) = match exposure {
                Some(exposure) => (exposure.total, exposure.own, exposure.nominator_count),
                None => (0, 0, 0),
            };
            let points = points_map.get(&addr_bytes).copied().unwrap_or(0);
            let apy = if era_reward > 0 && total_points > 0 && points > 0 && total_stake > 0 {
                let nominator_reward = calculate_nominator_reward(
                    era_reward,
                    points,
                    total_points,
                    validator.preferences.commission,
                );
                let apy = get_era_apy(nominator_reward, total_stake, era_duration_ms);
                is_realistic_apy(apy).then_some(apy)
            } else {
                None
            };

            let address = validator.address.to_string();
            DisplayValidator {
                name: identity_map.get(&address).cloned(),
                address,
                commission: validator.preferences.commission,
                blocked: validator.preferences.blocked,
                total_stake,
                own_stake,
                nominator_count,
                points,
                apy,
            }
        })
        .collect();

    display_validators.sort_by(|a, b| match (a.apy, b.apy) {
        (Some(a_apy), Some(b_apy)) => b_apy
            .partial_cmp(&a_apy)
            .unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });

    let validators_with_apy = display_validators
        .iter()
        .filter(|validator| validator.apy.is_some())
        .count();

    DisplayValidatorEnrichment {
        validators: display_validators,
        apy_era,
        validators_with_apy,
    }
}

/// Build a map of display validator APY by SS58 address.
pub fn validator_apy_map(validators: &[DisplayValidator]) -> HashMap<String, f64> {
    validators
        .iter()
        .filter_map(|validator| validator.apy.map(|apy| (validator.address.clone(), apy)))
        .collect()
}

/// Average a nomination pool's validator APYs from known target APYs.
pub fn pool_nomination_apy(
    nominations: &PoolNominations,
    validator_apy_map: &HashMap<String, f64>,
) -> Option<f64> {
    let mut total_apy = 0.0;
    let mut count = 0;
    for target in &nominations.targets {
        if let Some(&validator_apy) = validator_apy_map.get(&target.to_string()) {
            total_apy += validator_apy;
            count += 1;
        }
    }

    if count > 0 {
        Some(total_apy / count as f64)
    } else {
        None
    }
}

/// Convert pool metadata entries to lookup map.
pub fn pool_metadata_map(metadata: &[PoolMetadata]) -> HashMap<u32, String> {
    metadata
        .iter()
        .map(|metadata| (metadata.id, metadata.name.clone()))
        .collect()
}

/// Convert raw pools without pool APY calculations.
pub fn basic_display_pools(
    pools: &[ChainPoolInfo],
    metadata_map: &HashMap<u32, String>,
) -> Vec<DisplayPool> {
    pools
        .iter()
        .map(|pool| DisplayPool {
            id: pool.id,
            name: metadata_map.get(&pool.id).cloned().unwrap_or_default(),
            state: pool.state.into(),
            member_count: pool.member_count,
            total_bonded: pool.points,
            commission: pool.commission,
            apy: None,
        })
        .collect()
}

/// Choose pool IDs for nomination/APY queries using a deterministic pre-APY sort.
pub fn pool_ids_for_nomination_queries(pools: &[ChainPoolInfo], max_pools: usize) -> Vec<u32> {
    let mut pools: Vec<_> = pools.iter().collect();
    pools.sort_by(|a, b| {
        let a_open = matches!(a.state, crate::PoolState::Open);
        let b_open = matches!(b.state, crate::PoolState::Open);
        b_open
            .cmp(&a_open)
            .then_with(|| b.member_count.cmp(&a.member_count))
            .then_with(|| b.points.cmp(&a.points))
            .then_with(|| a.id.cmp(&b.id))
    });
    pools
        .into_iter()
        .take(max_pools)
        .map(|pool| pool.id)
        .collect()
}

/// Convert pools with pool nomination APY, sorted for display.
pub fn enrich_display_pools(
    pools: &[ChainPoolInfo],
    metadata_map: &HashMap<u32, String>,
    nominations_map: &HashMap<u32, PoolNominations>,
    validator_apy_map: &HashMap<String, f64>,
    max_pools_to_query: usize,
) -> Vec<DisplayPool> {
    let queried_pool_ids: std::collections::HashSet<u32> =
        pool_ids_for_nomination_queries(pools, max_pools_to_query)
            .into_iter()
            .collect();
    let mut display_pools: Vec<DisplayPool> = pools
        .iter()
        .map(|pool| {
            let apy = if queried_pool_ids.contains(&pool.id) {
                nominations_map
                    .get(&pool.id)
                    .and_then(|nominations| pool_nomination_apy(nominations, validator_apy_map))
                    .map(|avg_apy| avg_apy * (1.0 - pool.commission.unwrap_or(0.0)))
            } else {
                None
            };

            DisplayPool {
                id: pool.id,
                name: metadata_map
                    .get(&pool.id)
                    .cloned()
                    .unwrap_or_else(|| format!("Pool #{}", pool.id)),
                state: pool.state.into(),
                member_count: pool.member_count,
                total_bonded: pool.points,
                commission: pool.commission,
                apy,
            }
        })
        .collect();

    display_pools.sort_by(|a, b| match (a.apy, b.apy) {
        (Some(a_apy), Some(b_apy)) => b_apy
            .partial_cmp(&a_apy)
            .unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => b.member_count.cmp(&a.member_count),
    });

    display_pools
}

/// Estimate user's reward for an era, capped to avoid unrealistic values.
pub fn estimate_user_reward(
    era_reward: Balance,
    user_bonded: Balance,
    total_staked: Balance,
) -> Balance {
    if user_bonded == 0 || total_staked == 0 {
        return 0;
    }
    let estimated = (era_reward as f64 * user_bonded as f64 / total_staked as f64) as u128;
    let max_reasonable = user_bonded / MAX_REWARD_FRACTION;
    if estimated > max_reasonable && max_reasonable > 0 {
        max_reasonable
    } else {
        estimated
    }
}

/// Calculate a compact persisted date for an era in YYYYMMDD format.
pub fn calculate_era_date(
    era: u32,
    current_era: u32,
    current_era_start_ms: u64,
    era_duration_ms: u64,
) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let eras_ago = current_era.saturating_sub(era) as u64;
    let elapsed_ms = eras_ago.saturating_mul(era_duration_ms);
    let reference_ms = if current_era_start_ms > 0 {
        current_era_start_ms
    } else {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
            .unwrap_or_default()
    };
    let era_start_ms = reference_ms.saturating_sub(elapsed_ms);
    let era_start_ms = era_start_ms.min(i64::MAX as u64) as i64;

    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(era_start_ms)
        .unwrap_or_else(chrono::Utc::now)
        .format("%Y%m%d")
        .to_string()
}

/// Build a display history point for a user from era-level reward/stake inputs.
pub fn staking_history_point(
    era: u32,
    current_era: u32,
    current_era_start_ms: u64,
    era_duration_ms: u64,
    era_reward: Balance,
    user_bonded: Balance,
    total_staked: Balance,
) -> StakingHistoryPoint {
    let reward = estimate_user_reward(era_reward, user_bonded, total_staked);
    let apy = if user_bonded > 0 {
        get_era_apy(reward, user_bonded, era_duration_ms)
    } else {
        0.0
    };

    StakingHistoryPoint::new(
        era,
        calculate_era_date(era, current_era, current_era_start_ms, era_duration_ms),
        reward,
        user_bonded,
        apy,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PoolRoles, PoolState};
    use stkopt_core::ValidatorPreferences;

    fn chain_validator(byte: u8, commission: f64) -> ChainValidatorInfo {
        ChainValidatorInfo {
            address: AccountId32::from([byte; 32]),
            preferences: ValidatorPreferences {
                commission,
                blocked: false,
            },
        }
    }

    #[test]
    fn validator_apy_map_collects_known_values() {
        let validators = vec![
            DisplayValidator::new("a".into(), None, 0.0, false, 0, 0, 0, 0, Some(0.12)),
            DisplayValidator::new("b".into(), None, 0.0, false, 0, 0, 0, 0, None),
        ];

        let map = validator_apy_map(&validators);

        assert_eq!(map.len(), 1);
        assert_eq!(map.get("a"), Some(&0.12));
    }

    #[test]
    fn pool_nomination_apy_averages_known_targets() {
        let nominations = PoolNominations {
            pool_id: 1,
            stash: AccountId32::from([1; 32]),
            targets: vec![AccountId32::from([2; 32]), AccountId32::from([3; 32])],
        };
        let validator_apy_map = HashMap::from([
            (AccountId32::from([2; 32]).to_string(), 0.10),
            (AccountId32::from([3; 32]).to_string(), 0.20),
        ]);

        let apy = pool_nomination_apy(&nominations, &validator_apy_map).unwrap();
        assert!((apy - 0.15).abs() < f64::EPSILON);
    }

    #[test]
    fn eras_for_lookback_days_uses_era_duration() {
        assert_eq!(eras_for_lookback_days(30, MS_PER_DAY), 30);
        assert_eq!(eras_for_lookback_days(30, 6 * 60 * 60 * 1000), 120);
        assert_eq!(eras_for_lookback_days(1, 7 * 60 * 60 * 1000), 4);
    }

    #[test]
    fn eras_for_lookback_days_never_returns_zero() {
        assert_eq!(eras_for_lookback_days(0, MS_PER_DAY), 1);
        assert_eq!(eras_for_lookback_days(30, 0), 1);
    }

    #[test]
    fn missing_validator_identity_addresses_skips_cached_names() {
        let validators = vec![chain_validator(1, 0.05), chain_validator(2, 0.05)];
        let identity_map =
            HashMap::from([(AccountId32::from([1; 32]).to_string(), "One".to_string())]);

        let missing = missing_validator_identity_addresses(&validators, &identity_map);

        assert_eq!(missing, vec![AccountId32::from([2; 32])]);
    }

    #[test]
    fn basic_display_validators_preserves_preferences() {
        let validators = vec![chain_validator(1, 0.05)];

        let display = basic_display_validators(&validators);

        assert_eq!(display.len(), 1);
        assert_eq!(display[0].commission, 0.05);
        assert_eq!(display[0].total_stake, 0);
        assert_eq!(display[0].apy, None);
    }

    #[test]
    fn estimate_user_reward_caps_unrealistic_values() {
        assert_eq!(estimate_user_reward(10_000, 10_000, 1_000), 50);
    }

    #[test]
    fn enrich_display_pools_sorts_by_apy() {
        let roles = PoolRoles {
            depositor: AccountId32::from([0; 32]),
            root: None,
            nominator: None,
            bouncer: None,
        };
        let pools = vec![
            ChainPoolInfo {
                id: 1,
                state: PoolState::Open,
                points: 100,
                member_count: 10,
                roles: roles.clone(),
                commission: None,
            },
            ChainPoolInfo {
                id: 2,
                state: PoolState::Open,
                points: 200,
                member_count: 20,
                roles,
                commission: None,
            },
        ];
        let metadata = HashMap::from([(1, "One".to_string()), (2, "Two".to_string())]);
        let target_one = AccountId32::from([1; 32]);
        let target_two = AccountId32::from([2; 32]);
        let nominations = HashMap::from([
            (
                1,
                PoolNominations {
                    pool_id: 1,
                    stash: AccountId32::from([11; 32]),
                    targets: vec![target_one],
                },
            ),
            (
                2,
                PoolNominations {
                    pool_id: 2,
                    stash: AccountId32::from([22; 32]),
                    targets: vec![target_two],
                },
            ),
        ]);
        let validator_apys = HashMap::from([
            (target_one.to_string(), 0.10),
            (target_two.to_string(), 0.20),
        ]);

        let display = enrich_display_pools(&pools, &metadata, &nominations, &validator_apys, 2);

        assert_eq!(display[0].id, 2);
        assert_eq!(display[0].apy, Some(0.20));
    }

    #[test]
    fn pool_ids_for_nomination_queries_uses_stable_candidate_order() {
        let roles = PoolRoles {
            depositor: AccountId32::from([0; 32]),
            root: None,
            nominator: None,
            bouncer: None,
        };
        let pools = vec![
            ChainPoolInfo {
                id: 1,
                state: PoolState::Open,
                points: 100,
                member_count: 1,
                roles: roles.clone(),
                commission: None,
            },
            ChainPoolInfo {
                id: 2,
                state: PoolState::Blocked,
                points: 10_000,
                member_count: 10_000,
                roles: roles.clone(),
                commission: None,
            },
            ChainPoolInfo {
                id: 3,
                state: PoolState::Open,
                points: 5_000,
                member_count: 500,
                roles,
                commission: None,
            },
        ];

        let ids = pool_ids_for_nomination_queries(&pools, 2);

        assert_eq!(ids, vec![3, 1]);
    }

    #[test]
    fn calculate_nominator_reward_uses_integer_math_for_large_rewards() {
        let era_reward = 10_000_000_000_000_000_000_000_000u128; // 10^25
        let points = 100_000u32;
        let total_points = 1_000_000u32;
        let commission = 0.1;

        let reward = calculate_nominator_reward(era_reward, points, total_points, commission);

        // validator_share = 10^25 * 10^5 / 10^6 = 10^24
        // commission_factor = 0.9 * 10^9 = 900_000_000
        // nominator_reward = 10^24 * 900_000_000 / 1_000_000_000 = 9 * 10^23
        let expected = 900_000_000_000_000_000_000_000u128;
        assert_eq!(reward, expected);
    }

    #[test]
    fn calculate_nominator_reward_handles_zero_points() {
        assert_eq!(calculate_nominator_reward(1_000_000, 0, 1_000_000, 0.1), 0);
    }

    #[test]
    fn enrich_display_validators_uses_integer_math_for_large_rewards() {
        let validator = chain_validator(1, 0.10);
        let exposure = ValidatorExposure {
            address: validator.address.clone(),
            own: 1_000_000_000_000u128,
            total: 9_000_000_000_000_000_000_000_000_000u128,
            nominator_count: 1,
        };
        let points = ValidatorPoints {
            address: validator.address.clone(),
            points: 100_000,
        };
        let apy_data = ValidatorApyData {
            era: 1,
            era_reward: 10_000_000_000_000_000_000_000_000u128,
            total_points: 1_000_000,
            points: vec![points],
            exposures: vec![exposure],
        };
        let validators = vec![validator];
        let identity_map = HashMap::new();

        let result = enrich_display_validators(
            &validators,
            &identity_map,
            Some(&apy_data),
            1,
            6 * 60 * 60 * 1000,
        );

        assert_eq!(result.validators.len(), 1);
        let apy = result.validators[0].apy;
        assert!(
            apy.map(is_realistic_apy).unwrap_or(false),
            "expected realistic APY for large reward, got {:?}",
            apy
        );
    }

    #[test]
    fn enrich_display_pools_applies_pool_commission() {
        let roles = PoolRoles {
            depositor: AccountId32::from([0; 32]),
            root: None,
            nominator: None,
            bouncer: None,
        };
        let pools = vec![ChainPoolInfo {
            id: 1,
            state: PoolState::Open,
            points: 100,
            member_count: 10,
            roles,
            commission: Some(0.05),
        }];
        let metadata = HashMap::from([(1, "One".to_string())]);
        let target = AccountId32::from([1; 32]);
        let nominations = HashMap::from([(
            1,
            PoolNominations {
                pool_id: 1,
                stash: AccountId32::from([11; 32]),
                targets: vec![target],
            },
        )]);
        let validator_apys = HashMap::from([(target.to_string(), 0.12)]);

        let display = enrich_display_pools(&pools, &metadata, &nominations, &validator_apys, 1);

        assert_eq!(display.len(), 1);
        let apy = display[0].apy.expect("pool APY should be present");
        assert!((apy - 0.114).abs() < 1e-9, "APY was {}", apy);
    }

    #[test]
    fn enrich_display_pools_zero_commission_preserves_average_apy() {
        let roles = PoolRoles {
            depositor: AccountId32::from([0; 32]),
            root: None,
            nominator: None,
            bouncer: None,
        };
        let pools = vec![ChainPoolInfo {
            id: 1,
            state: PoolState::Open,
            points: 100,
            member_count: 10,
            roles,
            commission: Some(0.0),
        }];
        let metadata = HashMap::from([(1, "One".to_string())]);
        let target = AccountId32::from([1; 32]);
        let nominations = HashMap::from([(
            1,
            PoolNominations {
                pool_id: 1,
                stash: AccountId32::from([11; 32]),
                targets: vec![target],
            },
        )]);
        let validator_apys = HashMap::from([(target.to_string(), 0.12)]);

        let display = enrich_display_pools(&pools, &metadata, &nominations, &validator_apys, 1);

        assert_eq!(display.len(), 1);
        let apy = display[0].apy.expect("pool APY should be present");
        assert!((apy - 0.12).abs() < 1e-9, "APY was {}", apy);
    }
}
