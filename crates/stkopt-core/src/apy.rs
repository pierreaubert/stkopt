//! APY calculation functions.

use crate::types::Balance;

/// Milliseconds per year (accounting for leap years).
pub const MS_PER_YEAR: f64 = 365.24219 * 24.0 * 60.0 * 60.0 * 1000.0;

/// Maximum nominations allowed per nominator.
pub const MAX_NOMINATIONS: usize = 16;

/// Default maximum commission threshold for validator selection.
pub const DEFAULT_MAX_COMMISSION: f64 = 0.15;

/// History depth for era data (number of past eras available).
pub const HISTORY_DEPTH: u32 = 21;

/// Calculate APY from era reward data.
///
/// Formula from the TypeScript reference:
/// ```text
/// erasInAYear = MS_PER_YEAR / eraDurationInMs
/// rewardPct = eraReward / invested
/// APY = (1 + rewardPct)^erasInAYear - 1
/// ```
///
/// # Arguments
/// * `era_reward` - Total reward for the era
/// * `invested` - Amount invested/bonded
/// * `era_duration_ms` - Era duration in milliseconds
///
/// # Returns
/// APY as a decimal (e.g., 0.15 for 15%)
pub fn get_era_apy(era_reward: Balance, invested: Balance, era_duration_ms: u64) -> f64 {
    if invested == 0 {
        return 0.0;
    }

    let eras_in_year = MS_PER_YEAR / era_duration_ms as f64;
    let reward_pct = era_reward as f64 / invested as f64;

    (1.0 + reward_pct).powf(eras_in_year) - 1.0
}

/// Calculate nominator APY (after validator commission).
///
/// # Arguments
/// * `total_reward` - Total reward for the validator
/// * `commission` - Validator commission as decimal (0.0 to 1.0)
/// * `invested` - Amount invested by nominator
/// * `era_duration_ms` - Era duration in milliseconds
pub fn get_nominator_apy(
    total_reward: Balance,
    commission: f64,
    invested: Balance,
    era_duration_ms: u64,
) -> f64 {
    let nominator_share = (total_reward as f64 * (1.0 - commission)) as Balance;
    get_era_apy(nominator_share, invested, era_duration_ms)
}

/// Moving average type for validator aggregation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MovingAverageType {
    #[default]
    Simple,
    Exponential,
}

/// Calculate simple moving average.
pub fn simple_moving_average(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

/// Calculate exponential moving average.
///
/// EMA formula: result = current * smoothing + previous * (1 - smoothing)
/// Where smoothing = 2 / (period + 1)
pub fn exponential_moving_average(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    if values.len() == 1 {
        return values[0];
    }

    let smoothing = 2.0 / (values.len() as f64 + 1.0);
    let mut ema = values[0];

    for &value in &values[1..] {
        ema = value * smoothing + ema * (1.0 - smoothing);
    }

    ema
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_get_era_apy_zero_invested() {
        assert_eq!(get_era_apy(1000, 0, 86_400_000), 0.0);
    }

    #[test]
    fn test_get_era_apy_calculation() {
        // 24 hour era duration
        let era_duration_ms = 86_400_000u64;
        // 1% reward per era
        let reward = 100u128;
        let invested = 10_000u128;

        let apy = get_era_apy(reward, invested, era_duration_ms);

        // With daily 1% compound interest, APY should be about 3678% (e^365.24 - 1)
        // But with discrete compounding: (1.01)^365.24 - 1 ≈ 37.78
        assert!(apy > 30.0 && apy < 40.0);
    }

    #[test]
    fn test_simple_moving_average() {
        let values = vec![10.0, 20.0, 30.0, 40.0, 50.0];
        assert_relative_eq!(simple_moving_average(&values), 30.0);
    }

    #[test]
    fn test_simple_moving_average_empty() {
        assert_eq!(simple_moving_average(&[]), 0.0);
    }

    #[test]
    fn test_exponential_moving_average() {
        let values = vec![10.0, 20.0, 30.0];
        let ema = exponential_moving_average(&values);
        // EMA gives more weight to recent values
        assert!(ema > 20.0);
    }
}
