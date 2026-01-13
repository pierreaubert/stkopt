//! Unified display types for UI frontends.
//!
//! These types merge the display structures from both TUI and GPUI
//! to provide a common interface for displaying staking data.

use serde::{Deserialize, Serialize};

use crate::types::PoolState;

/// Unified validator display information.
///
/// Merges TUI's `DisplayValidator` and GPUI's `ValidatorInfo` with all fields
/// from both implementations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayValidator {
    /// SS58-encoded address.
    pub address: String,
    /// Display name from on-chain identity (if available).
    pub name: Option<String>,
    /// Commission rate as a fraction (0.0 to 1.0).
    pub commission: f64,
    /// Whether the validator is blocking new nominations.
    pub blocked: bool,
    /// Total stake (own + nominated).
    pub total_stake: u128,
    /// Validator's own stake.
    pub own_stake: u128,
    /// Number of nominators.
    pub nominator_count: u32,
    /// Era points earned (used by TUI).
    pub points: u32,
    /// Estimated APY for nominators (after commission).
    /// None if APY cannot be calculated (e.g., no historical data).
    pub apy: Option<f64>,
}

impl DisplayValidator {
    /// Creates a new DisplayValidator with all fields specified.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        address: String,
        name: Option<String>,
        commission: f64,
        blocked: bool,
        total_stake: u128,
        own_stake: u128,
        nominator_count: u32,
        points: u32,
        apy: Option<f64>,
    ) -> Self {
        Self {
            address,
            name,
            commission,
            blocked,
            total_stake,
            own_stake,
            nominator_count,
            points,
            apy,
        }
    }

    /// Returns the APY as a percentage, or 0.0 if not available.
    pub fn apy_percent(&self) -> f64 {
        self.apy.unwrap_or(0.0) * 100.0
    }

    /// Returns the display name or a truncated address if no name is set.
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or_else(|| {
            if self.address.len() > 16 {
                &self.address[..16]
            } else {
                &self.address
            }
        })
    }
}

/// Unified staking history point.
///
/// Merges TUI's `StakingHistoryPoint` and GPUI's `HistoryPoint`.
/// Field names follow TUI conventions (`reward`, `bonded`) since they're clearer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StakingHistoryPoint {
    /// Era index.
    pub era: u32,
    /// Estimated date string (YYYYMMDD format).
    /// Optional because GPUI doesn't use this field.
    pub date: Option<String>,
    /// Reward earned in this era (in planck).
    pub reward: u128,
    /// Total bonded amount at start of era.
    pub bonded: u128,
    /// APY for this era (as a ratio, e.g., 0.15 for 15%).
    pub apy: f64,
}

impl StakingHistoryPoint {
    /// Creates a new StakingHistoryPoint with date.
    pub fn new(era: u32, date: String, reward: u128, bonded: u128, apy: f64) -> Self {
        Self {
            era,
            date: Some(date),
            reward,
            bonded,
            apy,
        }
    }

    /// Creates a new StakingHistoryPoint without date (for GPUI compatibility).
    pub fn new_without_date(era: u32, reward: u128, bonded: u128, apy: f64) -> Self {
        Self {
            era,
            date: None,
            reward,
            bonded,
            apy,
        }
    }

    /// Returns the APY as a percentage.
    pub fn apy_percent(&self) -> f64 {
        self.apy * 100.0
    }
}

/// Unified nomination pool display information.
///
/// Merges TUI's `DisplayPool` and GPUI's `PoolInfo` with all fields
/// from both implementations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayPool {
    /// Pool ID.
    pub id: u32,
    /// Pool name from metadata.
    pub name: String,
    /// Pool state (Open, Blocked, Destroying).
    pub state: PoolState,
    /// Number of pool members.
    pub member_count: u32,
    /// Total bonded amount (GPUI: total_bonded, TUI: points).
    pub total_bonded: u128,
    /// Pool commission rate (from GPUI).
    pub commission: Option<f64>,
    /// Estimated APY based on nominated validators (from TUI).
    pub apy: Option<f64>,
}

impl DisplayPool {
    /// Creates a new DisplayPool with all fields specified.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: u32,
        name: String,
        state: PoolState,
        member_count: u32,
        total_bonded: u128,
        commission: Option<f64>,
        apy: Option<f64>,
    ) -> Self {
        Self {
            id,
            name,
            state,
            member_count,
            total_bonded,
            commission,
            apy,
        }
    }

    /// Returns the APY as a percentage, or None if not available.
    pub fn apy_percent(&self) -> Option<f64> {
        self.apy.map(|a| a * 100.0)
    }

    /// Returns true if the pool is open for new members.
    pub fn is_open(&self) -> bool {
        matches!(self.state, PoolState::Open)
    }
}

/// Unified account staking information for display.
///
/// Provides a summary view of an account's staking status.
/// This is similar to GPUI's `StakingInfo` struct.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct StakingInfo {
    /// Total account balance (free + reserved).
    pub total_balance: u128,
    /// Transferable (free) balance.
    pub transferable: u128,
    /// Currently bonded/staked amount.
    pub bonded: u128,
    /// Amount currently unbonding.
    pub unbonding: u128,
    /// Pending staking rewards to claim.
    pub rewards_pending: u128,
    /// Whether the account is actively nominating validators.
    pub is_nominating: bool,
    /// Number of validators being nominated.
    pub nomination_count: usize,
}

impl StakingInfo {
    /// Returns true if the account has any staked funds.
    pub fn is_staking(&self) -> bool {
        self.bonded > 0
    }

    /// Returns true if there are rewards to claim.
    pub fn has_pending_rewards(&self) -> bool {
        self.rewards_pending > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_validator_new() {
        let v = DisplayValidator::new(
            "1234".to_string(),
            Some("Alice".to_string()),
            0.10,
            false,
            1_000_000,
            100_000,
            50,
            1000,
            Some(0.15),
        );
        assert_eq!(v.address, "1234");
        assert_eq!(v.name, Some("Alice".to_string()));
        assert_eq!(v.commission, 0.10);
        assert!(!v.blocked);
        assert_eq!(v.total_stake, 1_000_000);
        assert_eq!(v.own_stake, 100_000);
        assert_eq!(v.nominator_count, 50);
        assert_eq!(v.points, 1000);
        assert_eq!(v.apy, Some(0.15));
    }

    #[test]
    fn test_display_validator_clone() {
        let v1 = DisplayValidator::new("addr".to_string(), None, 0.05, true, 500, 100, 10, 0, None);
        let v2 = v1.clone();
        assert_eq!(v1, v2);
    }

    #[test]
    fn test_display_validator_apy_percent() {
        let v = DisplayValidator::new("addr".to_string(), None, 0.0, false, 0, 0, 0, 0, Some(0.15));
        assert!((v.apy_percent() - 15.0).abs() < 0.001);
    }

    #[test]
    fn test_display_validator_apy_percent_none() {
        let v = DisplayValidator::new("addr".to_string(), None, 0.0, false, 0, 0, 0, 0, None);
        assert!((v.apy_percent() - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_display_validator_display_name_with_name() {
        let v = DisplayValidator::new(
            "1234567890abcdef1234567890abcdef".to_string(),
            Some("Bob".to_string()),
            0.0,
            false,
            0,
            0,
            0,
            0,
            None,
        );
        assert_eq!(v.display_name(), "Bob");
    }

    #[test]
    fn test_display_validator_display_name_truncated() {
        let v = DisplayValidator::new(
            "1234567890abcdef1234567890abcdef".to_string(),
            None,
            0.0,
            false,
            0,
            0,
            0,
            0,
            None,
        );
        assert_eq!(v.display_name(), "1234567890abcdef");
    }

    #[test]
    fn test_display_validator_high_values() {
        let max_u128 = u128::MAX;
        let v = DisplayValidator::new(
            "addr".to_string(),
            None,
            1.0,
            false,
            max_u128,
            max_u128,
            u32::MAX,
            u32::MAX,
            Some(1.0),
        );
        assert_eq!(v.total_stake, max_u128);
        assert_eq!(v.own_stake, max_u128);
    }

    #[test]
    fn test_staking_history_point_new() {
        let p = StakingHistoryPoint::new(100, "20240101".to_string(), 1000, 50000, 0.12);
        assert_eq!(p.era, 100);
        assert_eq!(p.date, Some("20240101".to_string()));
        assert_eq!(p.reward, 1000);
        assert_eq!(p.bonded, 50000);
        assert_eq!(p.apy, 0.12);
    }

    #[test]
    fn test_staking_history_point_without_date() {
        let p = StakingHistoryPoint::new_without_date(200, 2000, 100000, 0.15);
        assert_eq!(p.era, 200);
        assert_eq!(p.date, None);
        assert_eq!(p.reward, 2000);
        assert_eq!(p.bonded, 100000);
        assert_eq!(p.apy, 0.15);
    }

    #[test]
    fn test_staking_history_point_apy_percent() {
        let p = StakingHistoryPoint::new_without_date(1, 0, 0, 0.1234);
        assert!((p.apy_percent() - 12.34).abs() < 0.001);
    }

    #[test]
    fn test_staking_history_point_equality() {
        let p1 = StakingHistoryPoint::new(1, "20240101".to_string(), 100, 1000, 0.10);
        let p2 = StakingHistoryPoint::new(1, "20240101".to_string(), 100, 1000, 0.10);
        assert_eq!(p1, p2);
    }

    #[test]
    fn test_display_pool_new() {
        let pool = DisplayPool::new(
            1,
            "Test Pool".to_string(),
            PoolState::Open,
            100,
            1_000_000,
            Some(0.05),
            Some(0.14),
        );
        assert_eq!(pool.id, 1);
        assert_eq!(pool.name, "Test Pool");
        assert_eq!(pool.state, PoolState::Open);
        assert_eq!(pool.member_count, 100);
        assert_eq!(pool.total_bonded, 1_000_000);
        assert_eq!(pool.commission, Some(0.05));
        assert_eq!(pool.apy, Some(0.14));
    }

    #[test]
    fn test_display_pool_is_open() {
        let open_pool = DisplayPool::new(1, "Open".to_string(), PoolState::Open, 0, 0, None, None);
        let blocked_pool = DisplayPool::new(
            2,
            "Blocked".to_string(),
            PoolState::Blocked,
            0,
            0,
            None,
            None,
        );
        let destroying_pool = DisplayPool::new(
            3,
            "Destroying".to_string(),
            PoolState::Destroying,
            0,
            0,
            None,
            None,
        );

        assert!(open_pool.is_open());
        assert!(!blocked_pool.is_open());
        assert!(!destroying_pool.is_open());
    }

    #[test]
    fn test_display_pool_apy_percent() {
        let pool = DisplayPool::new(
            1,
            "Pool".to_string(),
            PoolState::Open,
            0,
            0,
            None,
            Some(0.123),
        );
        assert!((pool.apy_percent().unwrap() - 12.3).abs() < 0.001);
    }

    #[test]
    fn test_display_pool_apy_percent_none() {
        let pool = DisplayPool::new(1, "Pool".to_string(), PoolState::Open, 0, 0, None, None);
        assert!(pool.apy_percent().is_none());
    }

    #[test]
    fn test_staking_info_default() {
        let info = StakingInfo::default();
        assert_eq!(info.total_balance, 0);
        assert_eq!(info.transferable, 0);
        assert_eq!(info.bonded, 0);
        assert_eq!(info.unbonding, 0);
        assert_eq!(info.rewards_pending, 0);
        assert!(!info.is_nominating);
        assert_eq!(info.nomination_count, 0);
    }

    #[test]
    fn test_staking_info_is_staking() {
        let mut info = StakingInfo::default();
        assert!(!info.is_staking());

        info.bonded = 1000;
        assert!(info.is_staking());
    }

    #[test]
    fn test_staking_info_has_pending_rewards() {
        let mut info = StakingInfo::default();
        assert!(!info.has_pending_rewards());

        info.rewards_pending = 100;
        assert!(info.has_pending_rewards());
    }

    #[test]
    #[cfg(feature = "persistence")]
    fn test_display_validator_serialization() {
        let v = DisplayValidator::new(
            "addr".to_string(),
            Some("Name".to_string()),
            0.10,
            false,
            1000,
            100,
            5,
            50,
            Some(0.12),
        );
        let json = serde_json::to_string(&v).unwrap();
        let v2: DisplayValidator = serde_json::from_str(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    #[cfg(feature = "persistence")]
    fn test_staking_history_point_serialization() {
        let p = StakingHistoryPoint::new(100, "20240115".to_string(), 5000, 100000, 0.15);
        let json = serde_json::to_string(&p).unwrap();
        let p2: StakingHistoryPoint = serde_json::from_str(&json).unwrap();
        assert_eq!(p, p2);
    }

    #[test]
    #[cfg(feature = "persistence")]
    fn test_display_pool_serialization() {
        let pool = DisplayPool::new(
            42,
            "Serialized Pool".to_string(),
            PoolState::Blocked,
            50,
            500000,
            Some(0.03),
            Some(0.11),
        );
        let json = serde_json::to_string(&pool).unwrap();
        let pool2: DisplayPool = serde_json::from_str(&json).unwrap();
        assert_eq!(pool, pool2);
    }
}
