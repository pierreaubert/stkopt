//! History utilities for staking rewards and era data.

use crate::app::HistoryPoint;

/// Time range for history queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HistoryRange {
    /// Last 7 days / ~7 eras.
    Week,
    /// Last 30 days / ~30 eras.
    #[default]
    Month,
    /// Last 90 days / ~90 eras.
    Quarter,
    /// Last 365 days / ~365 eras.
    Year,
    /// All available history.
    All,
}

impl HistoryRange {
    /// Get all available ranges.
    pub fn all() -> &'static [HistoryRange] {
        &[
            HistoryRange::Week,
            HistoryRange::Month,
            HistoryRange::Quarter,
            HistoryRange::Year,
            HistoryRange::All,
        ]
    }

    /// Get display label.
    pub fn label(&self) -> &'static str {
        match self {
            HistoryRange::Week => "7 Days",
            HistoryRange::Month => "30 Days",
            HistoryRange::Quarter => "90 Days",
            HistoryRange::Year => "1 Year",
            HistoryRange::All => "All Time",
        }
    }

    /// Get approximate era count for this range.
    pub fn era_count(&self) -> usize {
        match self {
            HistoryRange::Week => 7,
            HistoryRange::Month => 30,
            HistoryRange::Quarter => 90,
            HistoryRange::Year => 365,
            HistoryRange::All => usize::MAX,
        }
    }
}

/// Statistics computed from history data.
#[derive(Debug, Clone, Default)]
pub struct HistoryStats {
    /// Total rewards earned.
    pub total_rewards: u128,
    /// Average rewards per era.
    pub avg_rewards_per_era: f64,
    /// Average APY over the period.
    pub avg_apy: f64,
    /// Minimum APY in the period.
    pub min_apy: f64,
    /// Maximum APY in the period.
    pub max_apy: f64,
    /// Total staked at end of period.
    pub final_stake: u128,
    /// Number of eras in the data.
    pub era_count: usize,
}

/// Compute statistics from history data.
pub fn compute_stats(history: &[HistoryPoint]) -> HistoryStats {
    if history.is_empty() {
        return HistoryStats::default();
    }

    let total_rewards: u128 = history.iter().map(|h| h.rewards).sum();
    let (min_apy, max_apy, sum_apy) = history.iter().fold(
        (f64::MAX, f64::MIN, 0.0),
        |(min, max, sum), h| (min.min(h.apy), max.max(h.apy), sum + h.apy),
    );

    let era_count = history.len();
    let avg_apy = sum_apy / era_count as f64;
    let avg_rewards_per_era = total_rewards as f64 / era_count as f64;
    let final_stake = history.last().map(|h| h.staked).unwrap_or(0);

    HistoryStats {
        total_rewards,
        avg_rewards_per_era,
        avg_apy,
        min_apy: if min_apy == f64::MAX { 0.0 } else { min_apy },
        max_apy: if max_apy == f64::MIN { 0.0 } else { max_apy },
        final_stake,
        era_count,
    }
}

/// Filter history by range.
pub fn filter_by_range(history: &[HistoryPoint], range: HistoryRange) -> Vec<&HistoryPoint> {
    let count = range.era_count();
    if count >= history.len() {
        history.iter().collect()
    } else {
        history.iter().rev().take(count).collect::<Vec<_>>().into_iter().rev().collect()
    }
}

/// Generate mock history data for testing/demo.
pub fn generate_mock_history(era_count: usize, starting_era: u32) -> Vec<HistoryPoint> {
    let base_stake = 10_000_000_000_000u128; // 1000 DOT
    let base_apy = 14.0;

    (0..era_count)
        .map(|i| {
            let era = starting_era + i as u32;
            // Add some variation
            let stake_variation = (i as f64 * 0.01).sin() * 0.1 + 1.0;
            let apy_variation = (i as f64 * 0.05).cos() * 2.0;
            
            let staked = (base_stake as f64 * stake_variation) as u128;
            let apy = (base_apy + apy_variation).max(0.0);
            let rewards = (staked as f64 * apy / 100.0 / 365.0) as u128;

            HistoryPoint {
                era,
                staked,
                rewards,
                apy,
            }
        })
        .collect()
}

/// Format rewards for display (in DOT).
pub fn format_rewards(planck: u128) -> String {
    let dot = planck as f64 / 10_000_000_000.0;
    if dot >= 1000.0 {
        format!("{:.2}K DOT", dot / 1000.0)
    } else if dot >= 1.0 {
        format!("{:.4} DOT", dot)
    } else {
        format!("{:.6} DOT", dot)
    }
}

/// Format APY percentage.
pub fn format_apy(apy: f64) -> String {
    format!("{:.2}%", apy)
}

/// Calculate cumulative rewards from history.
pub fn cumulative_rewards(history: &[HistoryPoint]) -> Vec<(u32, u128)> {
    let mut cumulative = 0u128;
    history
        .iter()
        .map(|h| {
            cumulative += h.rewards;
            (h.era, cumulative)
        })
        .collect()
}

/// Calculate moving average APY over a window.
pub fn moving_average_apy(history: &[HistoryPoint], window: usize) -> Vec<(u32, f64)> {
    if history.len() < window || window == 0 {
        return history.iter().map(|h| (h.era, h.apy)).collect();
    }

    let mut result = Vec::with_capacity(history.len());
    
    // First window-1 points use available data
    for i in 0..window.min(history.len()) {
        let sum: f64 = history[..=i].iter().map(|h| h.apy).sum();
        result.push((history[i].era, sum / (i + 1) as f64));
    }
    
    // Remaining points use full window
    for i in window..history.len() {
        let sum: f64 = history[i + 1 - window..=i].iter().map(|h| h.apy).sum();
        result.push((history[i].era, sum / window as f64));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_history() -> Vec<HistoryPoint> {
        vec![
            HistoryPoint { era: 100, staked: 1000, rewards: 10, apy: 10.0 },
            HistoryPoint { era: 101, staked: 1100, rewards: 12, apy: 11.0 },
            HistoryPoint { era: 102, staked: 1200, rewards: 15, apy: 12.5 },
            HistoryPoint { era: 103, staked: 1300, rewards: 14, apy: 10.8 },
            HistoryPoint { era: 104, staked: 1400, rewards: 18, apy: 12.9 },
        ]
    }

    #[test]
    fn test_compute_stats() {
        let history = sample_history();
        let stats = compute_stats(&history);

        assert_eq!(stats.total_rewards, 69);
        assert_eq!(stats.era_count, 5);
        assert_eq!(stats.final_stake, 1400);
        assert!(stats.avg_apy > 10.0 && stats.avg_apy < 13.0);
        assert_eq!(stats.min_apy, 10.0);
        assert_eq!(stats.max_apy, 12.9);
    }

    #[test]
    fn test_compute_stats_empty() {
        let stats = compute_stats(&[]);
        assert_eq!(stats.total_rewards, 0);
        assert_eq!(stats.era_count, 0);
        assert_eq!(stats.avg_apy, 0.0);
    }

    #[test]
    fn test_filter_by_range_week() {
        let history = sample_history();
        let filtered = filter_by_range(&history, HistoryRange::Week);
        assert_eq!(filtered.len(), 5); // All 5 points (less than 7)
    }

    #[test]
    fn test_filter_by_range_limits() {
        let history = generate_mock_history(100, 1000);
        let filtered = filter_by_range(&history, HistoryRange::Month);
        assert_eq!(filtered.len(), 30);
        // Should be the last 30 eras
        assert_eq!(filtered.first().unwrap().era, 1070);
        assert_eq!(filtered.last().unwrap().era, 1099);
    }

    #[test]
    fn test_generate_mock_history() {
        let history = generate_mock_history(10, 500);
        assert_eq!(history.len(), 10);
        assert_eq!(history[0].era, 500);
        assert_eq!(history[9].era, 509);
        assert!(history.iter().all(|h| h.staked > 0));
        assert!(history.iter().all(|h| h.apy > 0.0));
    }

    #[test]
    fn test_format_rewards() {
        assert_eq!(format_rewards(10_000_000_000), "1.0000 DOT");
        assert_eq!(format_rewards(10_000_000_000_000), "1.00K DOT");
        assert_eq!(format_rewards(1_000_000), "0.000100 DOT");
    }

    #[test]
    fn test_format_apy() {
        assert_eq!(format_apy(12.5), "12.50%");
        assert_eq!(format_apy(0.0), "0.00%");
    }

    #[test]
    fn test_cumulative_rewards() {
        let history = sample_history();
        let cumulative = cumulative_rewards(&history);

        assert_eq!(cumulative.len(), 5);
        assert_eq!(cumulative[0], (100, 10));
        assert_eq!(cumulative[1], (101, 22));
        assert_eq!(cumulative[4], (104, 69));
    }

    #[test]
    fn test_moving_average_apy() {
        let history = sample_history();
        let ma = moving_average_apy(&history, 3);

        assert_eq!(ma.len(), 5);
        // First point is just itself
        assert_eq!(ma[0].0, 100);
        assert!((ma[0].1 - 10.0).abs() < 0.01);
        // Third point is average of first 3
        assert_eq!(ma[2].0, 102);
        let expected = (10.0 + 11.0 + 12.5) / 3.0;
        assert!((ma[2].1 - expected).abs() < 0.01);
    }

    #[test]
    fn test_moving_average_empty() {
        let ma = moving_average_apy(&[], 3);
        assert!(ma.is_empty());
    }

    #[test]
    fn test_history_range_labels() {
        assert_eq!(HistoryRange::Week.label(), "7 Days");
        assert_eq!(HistoryRange::Year.label(), "1 Year");
    }

    #[test]
    fn test_history_range_all() {
        let ranges = HistoryRange::all();
        assert_eq!(ranges.len(), 5);
    }

    #[test]
    fn test_history_range_era_count() {
        assert_eq!(HistoryRange::Week.era_count(), 7);
        assert_eq!(HistoryRange::Month.era_count(), 30);
        assert_eq!(HistoryRange::Quarter.era_count(), 90);
    }
}

#[cfg(test)]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn test_compute_stats_never_panics(era_count in 0usize..200) {
            let history = generate_mock_history(era_count, 1000);
            let _ = compute_stats(&history);
        }

        #[test]
        fn test_filter_never_exceeds_input(era_count in 0usize..100) {
            let history = generate_mock_history(era_count, 1000);
            for range in HistoryRange::all() {
                let filtered = filter_by_range(&history, *range);
                prop_assert!(filtered.len() <= history.len());
            }
        }

        #[test]
        fn test_cumulative_is_monotonic(era_count in 1usize..50) {
            let history = generate_mock_history(era_count, 1000);
            let cumulative = cumulative_rewards(&history);
            
            for i in 1..cumulative.len() {
                prop_assert!(cumulative[i].1 >= cumulative[i - 1].1);
            }
        }

        #[test]
        fn test_moving_average_preserves_length(era_count in 0usize..50, window in 1usize..10) {
            let history = generate_mock_history(era_count, 1000);
            let ma = moving_average_apy(&history, window);
            prop_assert_eq!(ma.len(), history.len());
        }

        #[test]
        fn test_format_rewards_never_panics(planck in any::<u128>()) {
            let _ = format_rewards(planck);
        }

        #[test]
        fn test_format_apy_never_panics(apy in any::<f64>()) {
            let _ = format_apy(apy);
        }
    }
}
