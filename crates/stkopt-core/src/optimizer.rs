//! Validator selection optimizer.

use std::collections::HashSet;

use crate::apy::MAX_NOMINATIONS;
use crate::display::DisplayValidator;

const MIN_APY_COVERAGE_RATIO: f64 = 0.50;

/// Validator candidate for nomination selection.
#[derive(Debug, Clone)]
pub struct ValidatorCandidate {
    pub address: String,
    pub commission: f64,
    pub blocked: bool,
    pub apy: f64,
    pub total_stake: u128,
    pub nominator_count: u32,
}

/// Optimization criteria for validator selection.
#[derive(Debug, Clone)]
pub struct OptimizationCriteria {
    /// Maximum commission rate (0.0 - 1.0).
    pub max_commission: f64,
    /// Exclude blocked validators.
    pub exclude_blocked: bool,
    /// Number of validators to select.
    pub target_count: usize,
    /// Selection strategy.
    pub strategy: SelectionStrategy,
}

impl Default for OptimizationCriteria {
    fn default() -> Self {
        Self {
            max_commission: 0.15, // 15% max commission
            exclude_blocked: true,
            target_count: MAX_NOMINATIONS,
            strategy: SelectionStrategy::TopApy,
        }
    }
}

/// Strategy for selecting validators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionStrategy {
    /// Select top validators by APY.
    TopApy,
    /// Randomly select from top 10% by APY.
    RandomFromTop,
    /// Diversify by stake (mix of high and low stake validators).
    DiversifyByStake,
}

/// Result of optimization.
#[derive(Debug, Clone)]
pub struct OptimizationResult {
    pub selected: Vec<ValidatorCandidate>,
    pub estimated_apy_min: f64,
    pub estimated_apy_max: f64,
    pub estimated_apy_avg: f64,
    pub total_stake: u128,
    pub avg_commission: f64,
}

/// Data source used to select validators.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OptimizationDataSource {
    /// Validators were scored by chain-derived APY.
    ChainApy,
    /// APY was unavailable, so validators were selected by active stake and commission.
    #[default]
    NoApyFallback,
}

/// Optimization result plus the scoring path used.
#[derive(Debug, Clone)]
pub struct DisplayOptimizationResult {
    pub result: OptimizationResult,
    pub data_source: OptimizationDataSource,
    pub validators_with_apy: usize,
    pub eligible_validators: usize,
    pub apy_coverage: f64,
}

/// Convert display validators into optimizer candidates.
pub fn validator_candidates_from_display(
    validators: &[DisplayValidator],
) -> Vec<ValidatorCandidate> {
    validators
        .iter()
        .map(|validator| ValidatorCandidate {
            address: validator.address.clone(),
            commission: validator.commission,
            blocked: validator.blocked,
            apy: validator.apy.unwrap_or(0.0),
            total_stake: validator.total_stake,
            nominator_count: validator.nominator_count,
        })
        .collect()
}

/// Optimize display validators using chain-derived APY when available.
///
/// If no validator has APY data, this falls back to selecting active validators
/// by commission, stake, and nominator count.
pub fn optimize_display_validators(
    validators: &[DisplayValidator],
    criteria: &OptimizationCriteria,
) -> DisplayOptimizationResult {
    let candidates = validator_candidates_from_display(validators);
    let filter_eligible_validators = candidates
        .iter()
        .filter(|validator| {
            validator.commission <= criteria.max_commission
                && (!criteria.exclude_blocked || !validator.blocked)
        })
        .count();
    let stake_eligible_validators = candidates
        .iter()
        .filter(|validator| {
            validator.commission <= criteria.max_commission
                && (!criteria.exclude_blocked || !validator.blocked)
                && validator.total_stake > 0
        })
        .count();
    let require_stake = stake_eligible_validators > 0;
    let eligible_validators = if require_stake {
        stake_eligible_validators
    } else {
        filter_eligible_validators
    };
    let validators_with_apy = candidates
        .iter()
        .filter(|validator| {
            validator.commission <= criteria.max_commission
                && (!criteria.exclude_blocked || !validator.blocked)
                && (!require_stake || validator.total_stake > 0)
                && validator.apy.is_finite()
                && validator.apy > 0.0
        })
        .count();
    let apy_coverage = if eligible_validators == 0 {
        0.0
    } else {
        validators_with_apy as f64 / eligible_validators as f64
    };

    let has_sufficient_apy = validators_with_apy >= criteria.target_count.min(eligible_validators)
        && apy_coverage >= MIN_APY_COVERAGE_RATIO;

    if has_sufficient_apy {
        let result = select_validators(&candidates, criteria);
        return DisplayOptimizationResult {
            result,
            data_source: OptimizationDataSource::ChainApy,
            validators_with_apy,
            eligible_validators,
            apy_coverage,
        };
    }

    DisplayOptimizationResult {
        result: select_validators_without_apy(&candidates, criteria),
        data_source: OptimizationDataSource::NoApyFallback,
        validators_with_apy,
        eligible_validators,
        apy_coverage,
    }
}

/// Select optimal validators based on criteria.
pub fn select_validators(
    candidates: &[ValidatorCandidate],
    criteria: &OptimizationCriteria,
) -> OptimizationResult {
    // Filter candidates
    let mut filtered: Vec<_> = candidates
        .iter()
        .filter(|v| {
            v.commission <= criteria.max_commission
                && (!criteria.exclude_blocked || !v.blocked)
                && v.apy.is_finite()
                && v.apy > 0.0
                && v.total_stake > 0
        })
        .cloned()
        .collect();

    // Sort by APY descending
    filtered.sort_by(|a, b| {
        b.apy
            .partial_cmp(&a.apy)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let selected: Vec<ValidatorCandidate> = match criteria.strategy {
        SelectionStrategy::TopApy => filtered.into_iter().take(criteria.target_count).collect(),
        SelectionStrategy::RandomFromTop => {
            use rand::seq::SliceRandom;

            // Select from top 10% (but at least 3x target_count to have meaningful diversity)
            let top_count = (filtered.len() / 10).max(criteria.target_count * 3);
            let mut top: Vec<_> = filtered.into_iter().take(top_count).collect();

            top.shuffle(&mut rand::thread_rng());
            top.into_iter().take(criteria.target_count).collect()
        }
        SelectionStrategy::DiversifyByStake => {
            // Take top half by APY, bottom half by stake (to support smaller validators)
            let half = criteria.target_count / 2;
            let remaining = criteria.target_count - half;

            let mut selected: Vec<_> = filtered.iter().take(half).cloned().collect();
            let selected_addresses: HashSet<_> =
                selected.iter().map(|v| v.address.clone()).collect();

            // Sort remaining by stake ascending (lower stake first)
            let mut by_stake: Vec<_> = filtered
                .iter()
                .filter(|v| !selected_addresses.contains(&v.address))
                .cloned()
                .collect();
            by_stake.sort_by_key(|v| v.total_stake);

            selected.extend(by_stake.into_iter().take(remaining));
            selected
        }
    };

    let (min, max, sum_apy, total_stake, sum_commission) = selected.iter().fold(
        (f64::MAX, f64::MIN, 0.0, 0u128, 0.0),
        |(min, max, sum_apy, total_stake, sum_commission), v| {
            (
                min.min(v.apy),
                max.max(v.apy),
                sum_apy + v.apy,
                total_stake + v.total_stake,
                sum_commission + v.commission,
            )
        },
    );

    let avg = if selected.is_empty() {
        0.0
    } else {
        sum_apy / selected.len() as f64
    };
    let avg_commission = if selected.is_empty() {
        0.0
    } else {
        sum_commission / selected.len() as f64
    };

    OptimizationResult {
        selected,
        estimated_apy_min: if min == f64::MAX { 0.0 } else { min },
        estimated_apy_max: if max == f64::MIN { 0.0 } else { max },
        estimated_apy_avg: avg,
        total_stake,
        avg_commission,
    }
}

/// Select active validators when APY data is unavailable.
///
/// This fallback prefers validators with non-zero total stake so inactive or
/// partially loaded no-exposure validators are not selected when any exposure
/// data is available. If every otherwise eligible validator is missing stake
/// data, it still returns a conservative commission-ranked selection instead of
/// an empty result.
pub fn select_validators_without_apy(
    candidates: &[ValidatorCandidate],
    criteria: &OptimizationCriteria,
) -> OptimizationResult {
    let mut eligible: Vec<_> = candidates
        .iter()
        .filter(|v| {
            v.commission <= criteria.max_commission && (!criteria.exclude_blocked || !v.blocked)
        })
        .cloned()
        .collect();

    if eligible.iter().any(|validator| validator.total_stake > 0) {
        eligible.retain(|validator| validator.total_stake > 0);
    }

    eligible.sort_by(|a, b| {
        a.commission
            .partial_cmp(&b.commission)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.total_stake.cmp(&a.total_stake))
            .then_with(|| b.nominator_count.cmp(&a.nominator_count))
    });

    let selected: Vec<_> = eligible.into_iter().take(criteria.target_count).collect();
    let total_stake = selected.iter().map(|v| v.total_stake).sum();
    let avg_commission = if selected.is_empty() {
        0.0
    } else {
        selected.iter().map(|v| v.commission).sum::<f64>() / selected.len() as f64
    };

    OptimizationResult {
        selected,
        estimated_apy_min: 0.0,
        estimated_apy_max: 0.0,
        estimated_apy_avg: 0.0,
        total_stake,
        avg_commission,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_candidate(
        address: &str,
        commission: f64,
        apy: f64,
        blocked: bool,
        stake: u128,
    ) -> ValidatorCandidate {
        ValidatorCandidate {
            address: address.to_string(),
            commission,
            blocked,
            apy,
            total_stake: stake,
            nominator_count: 100,
        }
    }

    #[test]
    fn test_select_validators_top_apy() {
        let candidates = vec![
            make_candidate("v1", 0.05, 0.10, false, 1000),
            make_candidate("v2", 0.05, 0.15, false, 2000),
            make_candidate("v3", 0.05, 0.12, false, 1500),
            make_candidate("v4", 0.05, 0.08, false, 800),
        ];

        let criteria = OptimizationCriteria {
            max_commission: 0.10,
            exclude_blocked: true,
            target_count: 2,
            strategy: SelectionStrategy::TopApy,
        };

        let result = select_validators(&candidates, &criteria);

        assert_eq!(result.selected.len(), 2);
        assert_eq!(result.selected[0].address, "v2"); // Highest APY
        assert_eq!(result.selected[1].address, "v3"); // Second highest
        assert!((result.estimated_apy_max - 0.15).abs() < 0.001);
        assert!((result.estimated_apy_min - 0.12).abs() < 0.001);
    }

    #[test]
    fn test_select_validators_filters_blocked() {
        let candidates = vec![
            make_candidate("v1", 0.05, 0.20, true, 1000), // Blocked
            make_candidate("v2", 0.05, 0.15, false, 2000),
            make_candidate("v3", 0.05, 0.12, false, 1500),
        ];

        let criteria = OptimizationCriteria {
            max_commission: 0.10,
            exclude_blocked: true,
            target_count: 2,
            strategy: SelectionStrategy::TopApy,
        };

        let result = select_validators(&candidates, &criteria);

        assert_eq!(result.selected.len(), 2);
        assert_eq!(result.selected[0].address, "v2");
        assert_eq!(result.selected[1].address, "v3");
    }

    #[test]
    fn test_select_validators_filters_high_commission() {
        let candidates = vec![
            make_candidate("v1", 0.25, 0.20, false, 1000), // High commission
            make_candidate("v2", 0.05, 0.15, false, 2000),
            make_candidate("v3", 0.10, 0.18, false, 1500),
        ];

        let criteria = OptimizationCriteria {
            max_commission: 0.15,
            exclude_blocked: true,
            target_count: 2,
            strategy: SelectionStrategy::TopApy,
        };

        let result = select_validators(&candidates, &criteria);

        assert_eq!(result.selected.len(), 2);
        assert_eq!(result.selected[0].address, "v3"); // 18% APY, 10% commission
        assert_eq!(result.selected[1].address, "v2"); // 15% APY, 5% commission
    }

    #[test]
    fn test_select_validators_diversify_by_stake() {
        let candidates = vec![
            make_candidate("v1", 0.05, 0.15, false, 5000),
            make_candidate("v2", 0.05, 0.14, false, 4000),
            make_candidate("v3", 0.05, 0.13, false, 3000),
            make_candidate("v4", 0.05, 0.12, false, 200), // Low stake
            make_candidate("v5", 0.05, 0.11, false, 100), // Lowest stake
        ];

        let criteria = OptimizationCriteria {
            max_commission: 0.10,
            exclude_blocked: true,
            target_count: 4,
            strategy: SelectionStrategy::DiversifyByStake,
        };

        let result = select_validators(&candidates, &criteria);

        assert_eq!(result.selected.len(), 4);
        // First 2 should be top APY
        assert_eq!(result.selected[0].address, "v1");
        assert_eq!(result.selected[1].address, "v2");
        // Last 2 should include low stake validators
        let addresses: Vec<_> = result.selected.iter().map(|v| v.address.as_str()).collect();
        assert!(addresses.contains(&"v5") || addresses.contains(&"v4"));
    }

    #[test]
    fn test_select_validators_empty_candidates() {
        let candidates: Vec<ValidatorCandidate> = vec![];
        let criteria = OptimizationCriteria::default();

        let result = select_validators(&candidates, &criteria);

        assert!(result.selected.is_empty());
        assert_eq!(result.estimated_apy_min, 0.0);
        assert_eq!(result.estimated_apy_max, 0.0);
        assert_eq!(result.estimated_apy_avg, 0.0);
    }

    #[test]
    fn test_optimization_criteria_default() {
        let criteria = OptimizationCriteria::default();

        assert!((criteria.max_commission - 0.15).abs() < 0.001);
        assert!(criteria.exclude_blocked);
        assert_eq!(criteria.target_count, 16);
        assert_eq!(criteria.strategy, SelectionStrategy::TopApy);
    }

    #[test]
    fn test_select_validators_random_from_top() {
        // Create many candidates to test random selection
        let candidates: Vec<_> = (0..100)
            .map(|i| {
                make_candidate(
                    &format!("v{}", i),
                    0.05,
                    0.10 + i as f64 * 0.001,
                    false,
                    1000 + i as u128,
                )
            })
            .collect();

        let criteria = OptimizationCriteria {
            max_commission: 0.10,
            exclude_blocked: true,
            target_count: 10,
            strategy: SelectionStrategy::RandomFromTop,
        };

        let result = select_validators(&candidates, &criteria);

        // Should select target_count validators
        assert_eq!(result.selected.len(), 10);
        // All selected should be from the original candidates
        for v in &result.selected {
            assert!(candidates.iter().any(|c| c.address == v.address));
        }
    }

    #[test]
    fn test_select_validators_random_from_top_small_pool() {
        // Small pool where top 10% < target_count
        let candidates: Vec<_> = (0..20)
            .map(|i| {
                make_candidate(
                    &format!("v{}", i),
                    0.05,
                    0.10 + i as f64 * 0.01,
                    false,
                    1000,
                )
            })
            .collect();

        let criteria = OptimizationCriteria {
            max_commission: 0.10,
            exclude_blocked: true,
            target_count: 10,
            strategy: SelectionStrategy::RandomFromTop,
        };

        let result = select_validators(&candidates, &criteria);

        assert_eq!(result.selected.len(), 10);
    }

    #[test]
    fn test_select_validators_filters_zero_stake_with_apy() {
        let candidates = vec![
            make_candidate("zero-stake-high-apy", 0.05, 0.50, false, 0),
            make_candidate("active", 0.05, 0.10, false, 2_000),
        ];

        let criteria = OptimizationCriteria {
            max_commission: 0.10,
            exclude_blocked: true,
            target_count: 2,
            strategy: SelectionStrategy::TopApy,
        };

        let result = select_validators(&candidates, &criteria);

        assert_eq!(result.selected.len(), 1);
        assert_eq!(result.selected[0].address, "active");
    }

    #[test]
    fn test_select_validators_filters_zero_apy() {
        let candidates = vec![
            make_candidate("v1", 0.05, 0.0, false, 1000), // Zero APY - should be filtered
            make_candidate("v2", 0.05, 0.15, false, 2000),
            make_candidate("v3", 0.05, -0.01, false, 1500), // Negative APY - should be filtered
        ];

        let criteria = OptimizationCriteria {
            max_commission: 0.10,
            exclude_blocked: true,
            target_count: 3,
            strategy: SelectionStrategy::TopApy,
        };

        let result = select_validators(&candidates, &criteria);

        assert_eq!(result.selected.len(), 1);
        assert_eq!(result.selected[0].address, "v2");
    }

    #[test]
    fn test_select_validators_include_blocked_when_disabled() {
        let candidates = vec![
            make_candidate("v1", 0.05, 0.20, true, 1000), // Blocked
            make_candidate("v2", 0.05, 0.15, false, 2000),
        ];

        let criteria = OptimizationCriteria {
            max_commission: 0.10,
            exclude_blocked: false, // Allow blocked validators
            target_count: 2,
            strategy: SelectionStrategy::TopApy,
        };

        let result = select_validators(&candidates, &criteria);

        assert_eq!(result.selected.len(), 2);
        assert_eq!(result.selected[0].address, "v1"); // Blocked but highest APY
    }

    #[test]
    fn test_select_validators_fewer_than_target() {
        let candidates = vec![
            make_candidate("v1", 0.05, 0.15, false, 1000),
            make_candidate("v2", 0.05, 0.12, false, 2000),
        ];

        let criteria = OptimizationCriteria {
            max_commission: 0.10,
            exclude_blocked: true,
            target_count: 10, // More than available
            strategy: SelectionStrategy::TopApy,
        };

        let result = select_validators(&candidates, &criteria);

        assert_eq!(result.selected.len(), 2); // Only 2 available
    }

    #[test]
    fn test_select_validators_nan_apy_sorting() {
        let candidates = vec![
            make_candidate("v1", 0.05, 0.15, false, 1000),
            make_candidate("v2", 0.05, f64::NAN, false, 2000), // NaN APY
            make_candidate("v3", 0.05, 0.12, false, 1500),
        ];

        let criteria = OptimizationCriteria {
            max_commission: 0.10,
            exclude_blocked: true,
            target_count: 3,
            strategy: SelectionStrategy::TopApy,
        };

        // Should not panic with NaN values
        let result = select_validators(&candidates, &criteria);
        assert!(!result.selected.is_empty());
    }

    #[test]
    fn test_diversify_by_stake_odd_target() {
        let candidates = vec![
            make_candidate("v1", 0.05, 0.15, false, 5000),
            make_candidate("v2", 0.05, 0.14, false, 4000),
            make_candidate("v3", 0.05, 0.13, false, 3000),
            make_candidate("v4", 0.05, 0.12, false, 200),
            make_candidate("v5", 0.05, 0.11, false, 100),
        ];

        let criteria = OptimizationCriteria {
            max_commission: 0.10,
            exclude_blocked: true,
            target_count: 3, // Odd number
            strategy: SelectionStrategy::DiversifyByStake,
        };

        let result = select_validators(&candidates, &criteria);

        assert_eq!(result.selected.len(), 3);
        // First should be top APY (half = 1)
        assert_eq!(result.selected[0].address, "v1");
    }

    #[test]
    fn test_selection_strategy_equality() {
        assert_eq!(SelectionStrategy::TopApy, SelectionStrategy::TopApy);
        assert_ne!(SelectionStrategy::TopApy, SelectionStrategy::RandomFromTop);
        assert_ne!(
            SelectionStrategy::RandomFromTop,
            SelectionStrategy::DiversifyByStake
        );
    }

    #[test]
    fn test_validator_candidate_clone() {
        let v = make_candidate("v1", 0.05, 0.15, false, 1000);
        let v_clone = v.clone();
        assert_eq!(v.address, v_clone.address);
        assert_eq!(v.commission, v_clone.commission);
        assert_eq!(v.apy, v_clone.apy);
    }

    #[test]
    fn test_optimization_result_clone() {
        let candidates = vec![make_candidate("v1", 0.05, 0.15, false, 1000)];
        let criteria = OptimizationCriteria::default();
        let result = select_validators(&candidates, &criteria);
        let result_clone = result.clone();

        assert_eq!(result.selected.len(), result_clone.selected.len());
        assert_eq!(result.estimated_apy_avg, result_clone.estimated_apy_avg);
        assert_eq!(result.total_stake, result_clone.total_stake);
        assert_eq!(result.avg_commission, result_clone.avg_commission);
    }

    #[test]
    fn test_optimization_criteria_clone() {
        let criteria = OptimizationCriteria::default();
        let criteria_clone = criteria.clone();

        assert_eq!(criteria.max_commission, criteria_clone.max_commission);
        assert_eq!(criteria.target_count, criteria_clone.target_count);
    }

    #[test]
    fn test_select_validators_without_apy_sorts_by_commission() {
        let candidates = vec![
            make_candidate("high-comm", 0.10, 0.0, false, 1_000),
            make_candidate("low-comm", 0.01, 0.0, false, 1_000),
        ];

        let result = select_validators_without_apy(&candidates, &OptimizationCriteria::default());

        assert_eq!(result.selected.len(), 2);
        assert_eq!(result.selected[0].address, "low-comm");
        assert_eq!(result.selected[1].address, "high-comm");
    }

    #[test]
    fn test_select_validators_without_apy_excludes_zero_stake() {
        let candidates = vec![
            make_candidate("inactive-low-comm", 0.0, 0.0, false, 0),
            make_candidate("active", 0.05, 0.0, false, 1_000),
        ];

        let result = select_validators_without_apy(&candidates, &OptimizationCriteria::default());

        assert_eq!(result.selected.len(), 1);
        assert_eq!(result.selected[0].address, "active");
    }

    #[test]
    fn test_select_validators_without_apy_falls_back_when_all_stake_unknown() {
        let candidates = vec![
            make_candidate("high-comm", 0.10, 0.0, false, 0),
            make_candidate("blocked-low-comm", 0.0, 0.0, true, 0),
            make_candidate("low-comm", 0.01, 0.0, false, 0),
        ];

        let result = select_validators_without_apy(&candidates, &OptimizationCriteria::default());

        assert_eq!(result.selected.len(), 2);
        assert_eq!(result.selected[0].address, "low-comm");
        assert_eq!(result.selected[1].address, "high-comm");
        assert_eq!(result.total_stake, 0);
    }

    #[test]
    fn test_select_validators_without_apy_filters_blocked_and_high_commission() {
        let candidates = vec![
            make_candidate("blocked", 0.01, 0.0, true, 10_000),
            make_candidate("high-comm", 0.50, 0.0, false, 10_000),
            make_candidate("eligible", 0.05, 0.0, false, 1_000),
        ];

        let result = select_validators_without_apy(&candidates, &OptimizationCriteria::default());

        assert_eq!(result.selected.len(), 1);
        assert_eq!(result.selected[0].address, "eligible");
    }

    #[test]
    fn test_select_validators_without_apy_honors_include_blocked() {
        let candidates = vec![
            make_candidate("blocked-low-comm", 0.01, 0.0, true, 10_000),
            make_candidate("eligible", 0.05, 0.0, false, 1_000),
        ];
        let criteria = OptimizationCriteria {
            exclude_blocked: false,
            ..OptimizationCriteria::default()
        };

        let result = select_validators_without_apy(&candidates, &criteria);

        assert_eq!(result.selected.len(), 2);
        assert_eq!(result.selected[0].address, "blocked-low-comm");
    }

    #[test]
    fn test_select_validators_without_apy_tiebreaks_by_stake_then_nominators() {
        let candidates = vec![
            ValidatorCandidate {
                nominator_count: 100,
                ..make_candidate("low-stake", 0.05, 0.0, false, 500)
            },
            ValidatorCandidate {
                nominator_count: 5,
                ..make_candidate("high-stake-few-noms", 0.05, 0.0, false, 5_000)
            },
            ValidatorCandidate {
                nominator_count: 50,
                ..make_candidate("high-stake-many-noms", 0.05, 0.0, false, 5_000)
            },
        ];

        let result = select_validators_without_apy(&candidates, &OptimizationCriteria::default());

        assert_eq!(result.selected.len(), 3);
        assert_eq!(result.selected[0].address, "high-stake-many-noms");
        assert_eq!(result.selected[1].address, "high-stake-few-noms");
        assert_eq!(result.selected[2].address, "low-stake");
    }

    #[test]
    fn test_select_validators_without_apy_respects_target_count() {
        let candidates: Vec<ValidatorCandidate> = (0..20)
            .map(|index| make_candidate(&format!("v{}", index), 0.01, 0.0, false, 1_000))
            .collect();

        let result = select_validators_without_apy(&candidates, &OptimizationCriteria::default());

        assert_eq!(result.selected.len(), 16);
    }

    #[test]
    fn test_validator_candidates_from_display_maps_fields() {
        let validators = vec![DisplayValidator::new(
            "addr".to_string(),
            Some("Name".to_string()),
            0.05,
            true,
            1_000,
            100,
            12,
            42,
            Some(0.12),
        )];

        let candidates = validator_candidates_from_display(&validators);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].address, "addr");
        assert_eq!(candidates[0].commission, 0.05);
        assert!(candidates[0].blocked);
        assert_eq!(candidates[0].total_stake, 1_000);
        assert_eq!(candidates[0].nominator_count, 12);
        assert_eq!(candidates[0].apy, 0.12);
    }

    #[test]
    fn test_optimize_display_validators_uses_chain_apy_when_available() {
        let validators = vec![
            DisplayValidator::new(
                "lower".to_string(),
                None,
                0.01,
                false,
                10_000,
                0,
                1,
                0,
                Some(0.05),
            ),
            DisplayValidator::new(
                "higher".to_string(),
                None,
                0.10,
                false,
                1_000,
                0,
                1,
                0,
                Some(0.12),
            ),
        ];

        let optimized = optimize_display_validators(&validators, &OptimizationCriteria::default());

        assert_eq!(optimized.data_source, OptimizationDataSource::ChainApy);
        assert_eq!(optimized.result.selected[0].address, "higher");
        assert!(optimized.result.estimated_apy_avg > 0.0);
    }

    #[test]
    fn test_optimize_display_validators_falls_back_only_without_apy() {
        let validators = vec![
            DisplayValidator::new(
                "high-comm".to_string(),
                None,
                0.10,
                false,
                10_000,
                0,
                1,
                0,
                None,
            ),
            DisplayValidator::new(
                "low-comm".to_string(),
                None,
                0.01,
                false,
                1_000,
                0,
                1,
                0,
                None,
            ),
        ];

        let optimized = optimize_display_validators(&validators, &OptimizationCriteria::default());

        assert_eq!(optimized.data_source, OptimizationDataSource::NoApyFallback);
        assert_eq!(optimized.result.selected[0].address, "low-comm");
        assert_eq!(optimized.result.estimated_apy_avg, 0.0);
    }

    #[test]
    fn test_optimize_display_validators_falls_back_on_sparse_apy_coverage() {
        let mut validators: Vec<_> = (0..10)
            .map(|index| {
                DisplayValidator::new(
                    format!("no-apy-{}", index),
                    None,
                    0.01,
                    false,
                    10_000 + index as u128,
                    0,
                    1,
                    0,
                    None,
                )
            })
            .collect();
        validators.push(DisplayValidator::new(
            "only-apy".to_string(),
            None,
            0.01,
            false,
            1_000,
            0,
            1,
            0,
            Some(0.20),
        ));

        let optimized = optimize_display_validators(
            &validators,
            &OptimizationCriteria {
                target_count: 2,
                ..OptimizationCriteria::default()
            },
        );

        assert_eq!(optimized.data_source, OptimizationDataSource::NoApyFallback);
        assert_eq!(optimized.validators_with_apy, 1);
        assert_eq!(optimized.eligible_validators, 11);
        assert!(
            !optimized
                .result
                .selected
                .iter()
                .any(|validator| validator.address == "only-apy")
        );
        assert_eq!(optimized.result.estimated_apy_avg, 0.0);
    }

    #[test]
    fn test_optimize_display_validators_falls_back_when_stake_is_unavailable() {
        let validators = vec![
            DisplayValidator::new("high-comm".to_string(), None, 0.10, false, 0, 0, 0, 0, None),
            DisplayValidator::new(
                "blocked-low-comm".to_string(),
                None,
                0.0,
                true,
                0,
                0,
                0,
                0,
                None,
            ),
            DisplayValidator::new("low-comm".to_string(), None, 0.01, false, 0, 0, 0, 0, None),
        ];

        let optimized = optimize_display_validators(&validators, &OptimizationCriteria::default());

        assert_eq!(optimized.data_source, OptimizationDataSource::NoApyFallback);
        assert_eq!(optimized.eligible_validators, 2);
        assert_eq!(optimized.validators_with_apy, 0);
        assert_eq!(optimized.result.selected.len(), 2);
        assert_eq!(optimized.result.selected[0].address, "low-comm");
        assert_eq!(optimized.result.selected[1].address, "high-comm");
        assert_eq!(optimized.result.total_stake, 0);
    }

    #[test]
    fn test_diversify_by_stake_is_fast_on_large_set() {
        let candidates: Vec<_> = (0..10_000)
            .map(|i| {
                make_candidate(
                    &format!("v{}", i),
                    0.05,
                    0.10 + i as f64 * 0.000_01,
                    false,
                    1000 + i as u128,
                )
            })
            .collect();

        let criteria = OptimizationCriteria {
            max_commission: 0.10,
            exclude_blocked: true,
            target_count: 16,
            strategy: SelectionStrategy::DiversifyByStake,
        };

        let start = std::time::Instant::now();
        let result = select_validators(&candidates, &criteria);
        let elapsed = start.elapsed();

        assert_eq!(result.selected.len(), criteria.target_count);
        assert!(
            elapsed.as_secs_f64() < 1.0,
            "DiversifyByStake took too long: {:?}",
            elapsed
        );
    }

    #[test]
    fn test_select_validators_excludes_non_finite_apy() {
        let candidates = vec![
            make_candidate("nan", 0.05, f64::NAN, false, 1000),
            make_candidate("inf", 0.05, f64::INFINITY, false, 1000),
            make_candidate("neg-inf", 0.05, f64::NEG_INFINITY, false, 1000),
            make_candidate("negative", 0.05, -0.01, false, 1000),
            make_candidate("zero", 0.05, 0.0, false, 1000),
            make_candidate("valid", 0.05, 0.15, false, 1000),
        ];

        let criteria = OptimizationCriteria {
            max_commission: 0.10,
            exclude_blocked: true,
            target_count: 5,
            strategy: SelectionStrategy::TopApy,
        };

        let result = select_validators(&candidates, &criteria);

        assert_eq!(result.selected.len(), 1);
        assert_eq!(result.selected[0].address, "valid");
    }

    #[test]
    fn test_optimize_display_validators_excludes_non_finite_apy_from_count() {
        let validators = vec![
            DisplayValidator::new(
                "nan".to_string(),
                None,
                0.01,
                false,
                10_000,
                0,
                1,
                0,
                Some(f64::NAN),
            ),
            DisplayValidator::new(
                "neg".to_string(),
                None,
                0.01,
                false,
                10_000,
                0,
                1,
                0,
                Some(-0.05),
            ),
            DisplayValidator::new(
                "inf".to_string(),
                None,
                0.01,
                false,
                10_000,
                0,
                1,
                0,
                Some(f64::INFINITY),
            ),
            DisplayValidator::new(
                "valid1".to_string(),
                None,
                0.01,
                false,
                10_000,
                0,
                1,
                0,
                Some(0.12),
            ),
            DisplayValidator::new(
                "valid2".to_string(),
                None,
                0.01,
                false,
                10_000,
                0,
                1,
                0,
                Some(0.11),
            ),
            DisplayValidator::new(
                "valid3".to_string(),
                None,
                0.01,
                false,
                10_000,
                0,
                1,
                0,
                Some(0.10),
            ),
        ];

        let optimized = optimize_display_validators(
            &validators,
            &OptimizationCriteria {
                target_count: 2,
                ..OptimizationCriteria::default()
            },
        );

        assert_eq!(optimized.validators_with_apy, 3);
        assert_eq!(optimized.data_source, OptimizationDataSource::ChainApy);
        assert_eq!(optimized.result.selected[0].address, "valid1");
    }
}
