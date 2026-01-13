//! Optimization utilities for validator selection.

use crate::app::ValidatorInfo;

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
            max_commission: 0.15,
            exclude_blocked: true,
            target_count: 16,
            strategy: SelectionStrategy::TopApy,
        }
    }
}

/// Strategy for selecting validators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionStrategy {
    /// Select top validators by APY.
    #[default]
    TopApy,
    /// Randomly select from top performers.
    RandomFromTop,
    /// Diversify by stake (mix of high and low stake validators).
    DiversifyByStake,
    /// Minimize commission.
    MinCommission,
}

impl SelectionStrategy {
    /// Get all available strategies.
    pub fn all() -> &'static [SelectionStrategy] {
        &[
            SelectionStrategy::TopApy,
            SelectionStrategy::RandomFromTop,
            SelectionStrategy::DiversifyByStake,
            SelectionStrategy::MinCommission,
        ]
    }

    /// Get display label for the strategy.
    pub fn label(&self) -> &'static str {
        match self {
            SelectionStrategy::TopApy => "Top APY",
            SelectionStrategy::RandomFromTop => "Random from Top",
            SelectionStrategy::DiversifyByStake => "Diversify by Stake",
            SelectionStrategy::MinCommission => "Min Commission",
        }
    }

    /// Get description for the strategy.
    pub fn description(&self) -> &'static str {
        match self {
            SelectionStrategy::TopApy => "Select validators with highest estimated APY",
            SelectionStrategy::RandomFromTop => "Randomly select from top 10% performers",
            SelectionStrategy::DiversifyByStake => "Mix of high and low stake validators",
            SelectionStrategy::MinCommission => "Prioritize validators with lowest commission",
        }
    }
}

/// Result of optimization.
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
}

/// Run optimization on a list of validators.
pub fn optimize_selection(
    validators: &[ValidatorInfo],
    criteria: &OptimizationCriteria,
) -> OptimizationResult {
    // Filter validators based on criteria
    let mut candidates: Vec<(usize, &ValidatorInfo)> = validators
        .iter()
        .enumerate()
        .filter(|(_, v)| {
            v.commission <= criteria.max_commission
                && (!criteria.exclude_blocked || !v.blocked)
                && v.apy.unwrap_or(0.0) > 0.0
        })
        .collect();

    if candidates.is_empty() {
        return OptimizationResult::default();
    }

    // Sort based on strategy
    match criteria.strategy {
        SelectionStrategy::TopApy => {
            candidates.sort_by(|a, b| {
                let apy_a = a.1.apy.unwrap_or(0.0);
                let apy_b = b.1.apy.unwrap_or(0.0);
                apy_b.partial_cmp(&apy_a).unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        SelectionStrategy::MinCommission => {
            candidates.sort_by(|a, b| {
                a.1.commission
                    .partial_cmp(&b.1.commission)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        SelectionStrategy::RandomFromTop => {
            // Sort by APY first, then shuffle top portion
            candidates.sort_by(|a, b| {
                let apy_a = a.1.apy.unwrap_or(0.0);
                let apy_b = b.1.apy.unwrap_or(0.0);
                apy_b.partial_cmp(&apy_a).unwrap_or(std::cmp::Ordering::Equal)
            });
            // Take top 10% or at least 3x target
            let top_count = (candidates.len() / 10).max(criteria.target_count * 3).min(candidates.len());
            let top_portion = &mut candidates[..top_count];
            
            // Simple shuffle using time-based seed
            use std::time::{SystemTime, UNIX_EPOCH};
            let seed = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as usize)
                .unwrap_or(0);
            
            for i in (1..top_portion.len()).rev() {
                let j = (seed.wrapping_mul(i + 1).wrapping_add(i)) % (i + 1);
                top_portion.swap(i, j);
            }
        }
        SelectionStrategy::DiversifyByStake => {
            // Sort by APY, take half from top, half from low stake
            candidates.sort_by(|a, b| {
                let apy_a = a.1.apy.unwrap_or(0.0);
                let apy_b = b.1.apy.unwrap_or(0.0);
                apy_b.partial_cmp(&apy_a).unwrap_or(std::cmp::Ordering::Equal)
            });
        }
    }

    // Select top N based on target count
    let selected: Vec<(usize, &ValidatorInfo)> = if criteria.strategy == SelectionStrategy::DiversifyByStake {
        // Special handling: take half from top APY, half from low stake
        let half = criteria.target_count / 2;
        let remaining = criteria.target_count - half;
        
        let top_apy: Vec<_> = candidates.iter().take(half).cloned().collect();
        let selected_indices: std::collections::HashSet<_> = top_apy.iter().map(|(i, _)| *i).collect();
        
        // Sort remaining by stake ascending
        let mut by_stake: Vec<_> = candidates
            .iter()
            .filter(|(i, _)| !selected_indices.contains(i))
            .cloned()
            .collect();
        by_stake.sort_by(|a, b| a.1.total_stake.cmp(&b.1.total_stake));
        
        let mut result = top_apy;
        result.extend(by_stake.into_iter().take(remaining));
        result
    } else {
        candidates.into_iter().take(criteria.target_count).collect()
    };

    if selected.is_empty() {
        return OptimizationResult::default();
    }

    // Calculate statistics
    let selected_indices: Vec<usize> = selected.iter().map(|(i, _)| *i).collect();
    
    let (min_apy, max_apy, sum_apy, total_stake, sum_commission) = selected.iter().fold(
        (f64::MAX, f64::MIN, 0.0, 0u128, 0.0),
        |(min, max, sum, stake, comm), (_, v)| {
            let apy = v.apy.unwrap_or(0.0);
            (
                min.min(apy),
                max.max(apy),
                sum + apy,
                stake + v.total_stake,
                comm + v.commission,
            )
        },
    );

    let count = selected.len() as f64;

    OptimizationResult {
        selected_indices,
        estimated_apy_min: if min_apy == f64::MAX { 0.0 } else { min_apy },
        estimated_apy_max: if max_apy == f64::MIN { 0.0 } else { max_apy },
        estimated_apy_avg: sum_apy / count,
        total_stake,
        avg_commission: sum_commission / count,
    }
}

/// Validate optimization criteria.
pub fn validate_criteria(criteria: &OptimizationCriteria) -> Result<(), String> {
    if criteria.max_commission < 0.0 || criteria.max_commission > 1.0 {
        return Err("Max commission must be between 0% and 100%".to_string());
    }
    if criteria.target_count == 0 {
        return Err("Target count must be at least 1".to_string());
    }
    if criteria.target_count > 24 {
        return Err("Target count cannot exceed 24 (Polkadot limit)".to_string());
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
        // Should select High APY (15%) and Medium APY (12%)
        assert!(result.selected_indices.contains(&0));
        assert!(result.selected_indices.contains(&1));
    }

    #[test]
    fn test_optimize_min_commission() {
        let validators = sample_validators();
        let criteria = OptimizationCriteria {
            max_commission: 0.15,
            exclude_blocked: true,
            target_count: 2,
            strategy: SelectionStrategy::MinCommission,
        };

        let result = optimize_selection(&validators, &criteria);
        assert_eq!(result.selected_indices.len(), 2);
        // Should select Low APY (2%) and High APY (5%) by commission
        assert!(result.selected_indices.contains(&2)); // 2% commission
        assert!(result.selected_indices.contains(&0)); // 5% commission
    }

    #[test]
    fn test_optimize_excludes_blocked() {
        let validators = sample_validators();
        let criteria = OptimizationCriteria {
            max_commission: 1.0, // Allow all commissions
            exclude_blocked: true,
            target_count: 10,
            strategy: SelectionStrategy::TopApy,
        };

        let result = optimize_selection(&validators, &criteria);
        // Should not include the blocked validator (index 3)
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
        // Should not include high commission validator (index 4, 25%)
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
    fn test_optimize_calculates_stats() {
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
        assert!(result.total_stake > 0);
        assert!(result.avg_commission > 0.0);
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
            target_count: 25,
            ..Default::default()
        };
        assert!(validate_criteria(&criteria).is_err());
    }

    #[test]
    fn test_strategy_labels() {
        assert_eq!(SelectionStrategy::TopApy.label(), "Top APY");
        assert_eq!(SelectionStrategy::MinCommission.label(), "Min Commission");
    }

    #[test]
    fn test_strategy_all() {
        let strategies = SelectionStrategy::all();
        assert_eq!(strategies.len(), 4);
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
            Just(SelectionStrategy::MinCommission),
            Just(SelectionStrategy::DiversifyByStake),
            Just(SelectionStrategy::RandomFromTop),
        ]
    }

    proptest! {
        #[test]
        fn test_optimize_never_exceeds_target(
            count in 0usize..100,
            target in 1usize..24,
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
        fn test_optimize_indices_valid(count in 1usize..50, target in 1usize..16) {
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
