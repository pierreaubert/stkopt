//! Validator selection optimizer.

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
            target_count: 16, // Polkadot max nominations
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
                && v.apy > 0.0
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
            // Select from top 10% (but at least 3x target_count to have meaningful diversity)
            let top_count = (filtered.len() / 10).max(criteria.target_count * 3);
            let top: Vec<_> = filtered.into_iter().take(top_count).collect();

            // Fisher-Yates shuffle with time-based seed for true randomness
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            use std::time::{SystemTime, UNIX_EPOCH};

            let mut indices: Vec<usize> = (0..top.len()).collect();

            // Seed from current time for different results each run
            let seed = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);

            // Fisher-Yates shuffle with seeded pseudo-random
            for i in (1..indices.len()).rev() {
                let mut hasher = DefaultHasher::new();
                (seed, i).hash(&mut hasher);
                let j = (hasher.finish() as usize) % (i + 1);
                indices.swap(i, j);
            }

            let selected: Vec<_> = indices
                .into_iter()
                .take(criteria.target_count)
                .filter_map(|idx| top.get(idx).cloned())
                .collect();
            selected
        }
        SelectionStrategy::DiversifyByStake => {
            // Take top half by APY, bottom half by stake (to support smaller validators)
            let half = criteria.target_count / 2;
            let remaining = criteria.target_count - half;

            let mut selected: Vec<_> = filtered.iter().take(half).cloned().collect();
            let already_selected: std::collections::HashSet<_> =
                selected.iter().map(|v| v.address.clone()).collect();

            // Sort remaining by stake ascending (lower stake first)
            let mut by_stake: Vec<_> = filtered
                .iter()
                .filter(|v| !already_selected.contains(&v.address))
                .cloned()
                .collect();
            by_stake.sort_by(|a, b| a.total_stake.cmp(&b.total_stake));

            selected.extend(by_stake.into_iter().take(remaining));
            selected
        }
    };

    // Calculate APY statistics
    let (min, max, sum) = selected
        .iter()
        .fold((f64::MAX, f64::MIN, 0.0), |(min, max, sum), v| {
            (min.min(v.apy), max.max(v.apy), sum + v.apy)
        });

    let avg = if selected.is_empty() {
        0.0
    } else {
        sum / selected.len() as f64
    };

    OptimizationResult {
        selected,
        estimated_apy_min: if min == f64::MAX { 0.0 } else { min },
        estimated_apy_max: if max == f64::MIN { 0.0 } else { max },
        estimated_apy_avg: avg,
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
            .map(|i| make_candidate(&format!("v{}", i), 0.05, 0.10 + i as f64 * 0.001, false, 1000 + i as u128))
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
            .map(|i| make_candidate(&format!("v{}", i), 0.05, 0.10 + i as f64 * 0.01, false, 1000))
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
    fn test_select_validators_filters_zero_apy() {
        let candidates = vec![
            make_candidate("v1", 0.05, 0.0, false, 1000),  // Zero APY - should be filtered
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
        assert_ne!(SelectionStrategy::RandomFromTop, SelectionStrategy::DiversifyByStake);
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
    }

    #[test]
    fn test_optimization_criteria_clone() {
        let criteria = OptimizationCriteria::default();
        let criteria_clone = criteria.clone();

        assert_eq!(criteria.max_commission, criteria_clone.max_commission);
        assert_eq!(criteria.target_count, criteria_clone.target_count);
    }
}
