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
            // Select from top 10%
            let top_count = (filtered.len() / 10).max(criteria.target_count);
            let top: Vec<_> = filtered.into_iter().take(top_count).collect();

            // Randomly select from top (using simple deterministic shuffle for reproducibility)
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};

            let mut selected = Vec::with_capacity(criteria.target_count);
            let mut indices: Vec<usize> = (0..top.len()).collect();

            // Simple shuffle based on hash
            for i in 0..indices.len() {
                let mut hasher = DefaultHasher::new();
                i.hash(&mut hasher);
                let j = (hasher.finish() as usize) % (indices.len() - i) + i;
                indices.swap(i, j);
            }

            for &idx in indices.iter().take(criteria.target_count) {
                if idx < top.len() {
                    selected.push(top[idx].clone());
                }
            }
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
}
