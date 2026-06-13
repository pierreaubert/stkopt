//! Optimization view adapter for validator selection.

use crate::app::ValidatorInfo;
use std::collections::HashMap;
use stkopt_core::{
    MAX_NOMINATIONS, OptimizationCriteria as CoreOptimizationCriteria, OptimizationDataSource,
    SelectionStrategy as CoreSelectionStrategy, optimize_display_validators,
};

/// Optimization criteria collected from the GPUI controls.
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
            max_commission: 0.15,
            exclude_blocked: true,
            target_count: MAX_NOMINATIONS,
            strategy: SelectionStrategy::TopApy,
        }
    }
}

/// Strategy for selecting validators in the GPUI controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionStrategy {
    /// Select top validators by APY.
    #[default]
    TopApy,
    /// Randomly select from top performers.
    RandomFromTop,
    /// Diversify by stake (mix of high and low stake validators).
    DiversifyByStake,
}

impl SelectionStrategy {
    /// Get all available strategies.
    pub fn all() -> &'static [SelectionStrategy] {
        &[
            SelectionStrategy::TopApy,
            SelectionStrategy::RandomFromTop,
            SelectionStrategy::DiversifyByStake,
        ]
    }

    /// Get display label for the strategy.
    pub fn label(&self) -> &'static str {
        match self {
            SelectionStrategy::TopApy => "Top APY",
            SelectionStrategy::RandomFromTop => "Random from Top",
            SelectionStrategy::DiversifyByStake => "Diversify by Stake",
        }
    }

    /// Get description for the strategy.
    pub fn description(&self) -> &'static str {
        match self {
            SelectionStrategy::TopApy => "Select validators with highest estimated APY",
            SelectionStrategy::RandomFromTop => "Randomly select from top 10% performers",
            SelectionStrategy::DiversifyByStake => "Mix of high and low stake validators",
        }
    }

    fn core_strategy(self) -> CoreSelectionStrategy {
        match self {
            SelectionStrategy::TopApy => CoreSelectionStrategy::TopApy,
            SelectionStrategy::RandomFromTop => CoreSelectionStrategy::RandomFromTop,
            SelectionStrategy::DiversifyByStake => CoreSelectionStrategy::DiversifyByStake,
        }
    }
}

/// Result of optimization, using GPUI validator indices for selection state.
#[derive(Debug, Clone, Default)]
pub struct OptimizationResult {
    /// Selected validator indices.
    pub selected_indices: Vec<usize>,
    /// Estimated minimum APY.
    pub estimated_apy_min: f64,
    /// Estimated maximum APY.
    pub estimated_apy_max: f64,
    /// Estimated average APY.
    pub estimated_apy_avg: f64,
    /// Total stake of selected validators.
    pub total_stake: u128,
    /// Average commission of selected validators.
    pub avg_commission: f64,
    /// Data source used by the shared optimizer.
    pub data_source: OptimizationDataSource,
    /// Validators with usable APY among eligible validators.
    pub validators_with_apy: usize,
    /// Eligible validator count before APY coverage gating.
    pub eligible_validators: usize,
    /// APY coverage ratio among eligible validators.
    pub apy_coverage: f64,
}

/// Run optimization on a list of validators using the core optimizer.
pub fn optimize_selection(
    validators: &[ValidatorInfo],
    criteria: &OptimizationCriteria,
) -> OptimizationResult {
    let core_criteria = CoreOptimizationCriteria {
        max_commission: criteria.max_commission,
        exclude_blocked: criteria.exclude_blocked,
        target_count: criteria.target_count,
        strategy: criteria.strategy.core_strategy(),
    };

    let optimized = optimize_display_validators(validators, &core_criteria);
    let result = optimized.result;
    let index_by_address: HashMap<&str, usize> = validators
        .iter()
        .enumerate()
        .map(|(index, validator)| (validator.address.as_str(), index))
        .collect();

    OptimizationResult {
        selected_indices: result
            .selected
            .iter()
            .filter_map(|validator| index_by_address.get(validator.address.as_str()).copied())
            .collect(),
        estimated_apy_min: result.estimated_apy_min,
        estimated_apy_max: result.estimated_apy_max,
        estimated_apy_avg: result.estimated_apy_avg,
        total_stake: result.total_stake,
        avg_commission: result.avg_commission,
        data_source: optimized.data_source,
        validators_with_apy: optimized.validators_with_apy,
        eligible_validators: optimized.eligible_validators,
        apy_coverage: optimized.apy_coverage,
    }
}

/// Format an APY ratio for display as a percentage.
pub fn format_apy_ratio(apy: f64) -> String {
    format!("{:.1}%", apy * 100.0)
}

/// Validate optimization criteria from the UI controls.
pub fn validate_criteria(criteria: &OptimizationCriteria) -> Result<(), String> {
    if criteria.max_commission < 0.0 || criteria.max_commission > 1.0 {
        return Err("Max commission must be between 0% and 100%".to_string());
    }
    if criteria.target_count == 0 {
        return Err("Target count must be at least 1".to_string());
    }
    if criteria.target_count > MAX_NOMINATIONS {
        return Err(format!(
            "Target count cannot exceed {} (Polkadot limit)",
            MAX_NOMINATIONS
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_validators() -> Vec<ValidatorInfo> {
        vec![
            ValidatorInfo {
                address: "1a".to_string(),
                name: Some("High APY".to_string()),
                commission: 0.05,
                total_stake: 1000,
                own_stake: 100,
                nominator_count: 50,
                points: 0,
                apy: Some(15.0),
                blocked: false,
            },
            ValidatorInfo {
                address: "1b".to_string(),
                name: Some("Medium APY".to_string()),
                commission: 0.10,
                total_stake: 2000,
                own_stake: 200,
                nominator_count: 100,
                points: 0,
                apy: Some(12.0),
                blocked: false,
            },
            ValidatorInfo {
                address: "1c".to_string(),
                name: Some("Low APY".to_string()),
                commission: 0.02,
                total_stake: 500,
                own_stake: 50,
                nominator_count: 25,
                points: 0,
                apy: Some(8.0),
                blocked: false,
            },
            ValidatorInfo {
                address: "1d".to_string(),
                name: Some("Blocked".to_string()),
                commission: 0.01,
                total_stake: 3000,
                own_stake: 300,
                nominator_count: 150,
                points: 0,
                apy: Some(20.0),
                blocked: true,
            },
            ValidatorInfo {
                address: "1e".to_string(),
                name: Some("High Commission".to_string()),
                commission: 0.25,
                total_stake: 1500,
                own_stake: 150,
                nominator_count: 75,
                points: 0,
                apy: Some(18.0),
                blocked: false,
            },
        ]
    }

    #[test]
    fn test_optimize_top_apy() {
        let validators = sample_validators();
        let criteria = OptimizationCriteria {
            max_commission: 0.15,
            exclude_blocked: true,
            target_count: 2,
            strategy: SelectionStrategy::TopApy,
        };

        let result = optimize_selection(&validators, &criteria);
        assert_eq!(result.selected_indices.len(), 2);
        assert_eq!(result.selected_indices, vec![0, 1]);
    }

    #[test]
    fn test_optimize_excludes_blocked() {
        let validators = sample_validators();
        let criteria = OptimizationCriteria {
            max_commission: 1.0,
            exclude_blocked: true,
            target_count: 10,
            strategy: SelectionStrategy::TopApy,
        };

        let result = optimize_selection(&validators, &criteria);
        assert!(!result.selected_indices.contains(&3));
    }

    #[test]
    fn test_optimize_excludes_high_commission() {
        let validators = sample_validators();
        let criteria = OptimizationCriteria {
            max_commission: 0.15,
            exclude_blocked: false,
            target_count: 10,
            strategy: SelectionStrategy::TopApy,
        };

        let result = optimize_selection(&validators, &criteria);
        assert!(!result.selected_indices.contains(&4));
    }

    #[test]
    fn test_optimize_empty_validators() {
        let validators: Vec<ValidatorInfo> = vec![];
        let criteria = OptimizationCriteria::default();

        let result = optimize_selection(&validators, &criteria);
        assert!(result.selected_indices.is_empty());
        assert_eq!(result.estimated_apy_avg, 0.0);
    }

    #[test]
    fn test_optimize_copies_core_stats() {
        let validators = sample_validators();
        let criteria = OptimizationCriteria {
            max_commission: 0.15,
            exclude_blocked: true,
            target_count: 3,
            strategy: SelectionStrategy::TopApy,
        };

        let result = optimize_selection(&validators, &criteria);
        assert_eq!(result.selected_indices.len(), 3);
        assert!(result.estimated_apy_min > 0.0);
        assert!(result.estimated_apy_max >= result.estimated_apy_min);
        assert!(result.estimated_apy_avg > 0.0);
        assert_eq!(result.total_stake, 3500);
        assert!(result.avg_commission > 0.0);
    }

    #[test]
    fn test_optimize_falls_back_without_apy() {
        let mut validators = sample_validators();
        for validator in &mut validators {
            validator.apy = None;
        }

        let criteria = OptimizationCriteria {
            max_commission: 0.15,
            exclude_blocked: true,
            target_count: 2,
            strategy: SelectionStrategy::TopApy,
        };

        let result = optimize_selection(&validators, &criteria);
        assert_eq!(result.selected_indices, vec![2, 0]);
        assert_eq!(result.estimated_apy_avg, 0.0);
    }

    #[test]
    fn test_format_apy_ratio() {
        assert_eq!(format_apy_ratio(0.068), "6.8%");
        assert_eq!(format_apy_ratio(0.0), "0.0%");
    }

    #[test]
    fn test_validate_criteria_valid() {
        let criteria = OptimizationCriteria::default();
        assert!(validate_criteria(&criteria).is_ok());
    }

    #[test]
    fn test_validate_criteria_invalid_commission() {
        let criteria = OptimizationCriteria {
            max_commission: 1.5,
            ..Default::default()
        };
        assert!(validate_criteria(&criteria).is_err());
    }

    #[test]
    fn test_validate_criteria_zero_target() {
        let criteria = OptimizationCriteria {
            target_count: 0,
            ..Default::default()
        };
        assert!(validate_criteria(&criteria).is_err());
    }

    #[test]
    fn test_validate_criteria_exceeds_max() {
        let criteria = OptimizationCriteria {
            target_count: MAX_NOMINATIONS + 1,
            ..Default::default()
        };
        assert!(validate_criteria(&criteria).is_err());
    }

    #[test]
    fn test_strategy_labels() {
        assert_eq!(SelectionStrategy::TopApy.label(), "Top APY");
        assert_eq!(
            SelectionStrategy::DiversifyByStake.label(),
            "Diversify by Stake"
        );
    }

    #[test]
    fn test_strategy_all() {
        let strategies = SelectionStrategy::all();
        assert_eq!(strategies.len(), 3);
    }

    #[test]
    fn test_diversify_by_stake() {
        let validators = sample_validators();
        let criteria = OptimizationCriteria {
            max_commission: 0.15,
            exclude_blocked: true,
            target_count: 2,
            strategy: SelectionStrategy::DiversifyByStake,
        };

        let result = optimize_selection(&validators, &criteria);
        assert_eq!(result.selected_indices.len(), 2);
    }
}

#[cfg(test)]
mod proptest_tests {
    use super::*;
    use crate::validators::generate_mock_validators;
    use proptest::prelude::*;

    fn arb_strategy() -> impl Strategy<Value = SelectionStrategy> {
        prop_oneof![
            Just(SelectionStrategy::TopApy),
            Just(SelectionStrategy::DiversifyByStake),
            Just(SelectionStrategy::RandomFromTop),
        ]
    }

    proptest! {
        #[test]
        fn test_optimize_never_exceeds_target(
            count in 0usize..100,
            target in 1usize..=MAX_NOMINATIONS,
            strategy in arb_strategy()
        ) {
            let validators = generate_mock_validators(count);
            let criteria = OptimizationCriteria {
                target_count: target,
                strategy,
                ..Default::default()
            };

            let result = optimize_selection(&validators, &criteria);
            prop_assert!(result.selected_indices.len() <= target);
        }

        #[test]
        fn test_optimize_indices_valid(count in 1usize..50, target in 1usize..=MAX_NOMINATIONS) {
            let validators = generate_mock_validators(count);
            let criteria = OptimizationCriteria {
                target_count: target,
                ..Default::default()
            };

            let result = optimize_selection(&validators, &criteria);
            for idx in &result.selected_indices {
                prop_assert!(*idx < validators.len());
            }
        }

        #[test]
        fn test_validate_criteria_commission_range(commission in -1.0f64..2.0) {
            let criteria = OptimizationCriteria {
                max_commission: commission,
                ..Default::default()
            };

            let result = validate_criteria(&criteria);
            if commission >= 0.0 && commission <= 1.0 {
                prop_assert!(result.is_ok());
            } else {
                prop_assert!(result.is_err());
            }
        }
    }
}
