//! Core domain types for staking optimization.

use serde::{Deserialize, Serialize};

pub type Balance = u128;
pub type EraIndex = u32;

/// Supported networks - exhaustive match required (no default case).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Network {
    Polkadot,
    Kusama,
    Westend,
    Paseo,
}

impl Network {
    pub fn token_symbol(&self) -> &'static str {
        match self {
            Network::Polkadot => "DOT",
            Network::Kusama => "KSM",
            Network::Westend => "WND",
            Network::Paseo => "PAS",
        }
    }

    pub fn token_decimals(&self) -> u8 {
        match self {
            Network::Polkadot => 10,
            Network::Kusama => 12,
            Network::Westend => 12,
            Network::Paseo => 10,
        }
    }

    pub fn ss58_format(&self) -> u16 {
        match self {
            Network::Polkadot => 0,
            Network::Kusama => 2,
            Network::Westend => 42,
            Network::Paseo => 0,
        }
    }

    /// Returns all known networks.
    pub fn all() -> &'static [Network] {
        &[
            Network::Polkadot,
            Network::Kusama,
            Network::Westend,
            Network::Paseo,
        ]
    }
}

impl std::fmt::Display for Network {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Network::Polkadot => write!(f, "Polkadot"),
            Network::Kusama => write!(f, "Kusama"),
            Network::Westend => write!(f, "Westend"),
            Network::Paseo => write!(f, "Paseo"),
        }
    }
}

/// Validator preferences from chain.
#[derive(Debug, Clone)]
pub struct ValidatorPreferences {
    /// Commission rate as a fraction (0.0 to 1.0).
    pub commission: f64,
    /// Whether the validator is blocking new nominations.
    pub blocked: bool,
}

/// Validator data for a specific era.
#[derive(Debug, Clone)]
pub struct ValidatorEraInfo {
    pub address: [u8; 32],
    pub active_bond: Balance,
    pub nominator_count: u32,
    pub reward: Balance,
    pub nominators_share: Balance,
    pub commission: f64,
    pub blocked: bool,
    pub points: u32,
}

/// Aggregated validator data across multiple eras.
#[derive(Debug, Clone)]
pub struct HistoricValidator {
    pub address: [u8; 32],
    pub commission: f64,
    pub blocked: bool,
    pub points: u32,
    pub active_bond: Balance,
    pub nominator_count: u32,
    /// APY for nominators (after commission).
    pub nominator_apy: f64,
    /// Total APY (before commission split).
    pub total_apy: f64,
    /// Fraction of eras the validator was active (0.0 to 1.0).
    pub active_ratio: f64,
}

/// Nomination pool information.
#[derive(Debug, Clone)]
pub struct NominationPool {
    pub id: u32,
    pub name: Option<String>,
    pub state: PoolState,
    pub points: Balance,
    pub member_count: u32,
    pub commission: f64,
    pub min_apy: f64,
    pub max_apy: f64,
    pub avg_apy: f64,
}

/// Pool state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PoolState {
    Open,
    Blocked,
    Destroying,
}

/// Era information.
#[derive(Debug, Clone)]
pub struct EraInfo {
    pub index: EraIndex,
    pub start_timestamp_ms: u64,
    pub duration_ms: u64,
    pub pct_complete: f64,
    pub estimated_end_ms: u64,
}

/// Account staking status.
#[derive(Debug, Clone)]
pub struct AccountStakingStatus {
    pub address: [u8; 32],
    pub free_balance: Balance,
    pub reserved_balance: Balance,
    pub bonded: Balance,
    pub nominations: Vec<[u8; 32]>,
    pub pending_rewards: Balance,
    pub unlocks: Vec<UnlockChunk>,
    pub nomination_pool: Option<NominationPoolMembership>,
}

/// Pending unlock chunk.
#[derive(Debug, Clone)]
pub struct UnlockChunk {
    pub value: Balance,
    pub era: EraIndex,
}

/// Nomination pool membership.
#[derive(Debug, Clone)]
pub struct NominationPoolMembership {
    pub pool_id: u32,
    pub points: Balance,
    pub pending_rewards: Balance,
}

/// Connection status for the chain client.
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Syncing { progress: f32 },
    Connected,
    Error(String),
}

/// Reward destination for staking rewards.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RewardDestination {
    /// Rewards are automatically bonded (compounding).
    #[default]
    Staked,
    /// Rewards are paid to the stash account.
    Stash,
    /// Rewards are paid to the controller account.
    Controller,
    /// Rewards are paid to a specific account (SS58 encoded).
    Account(String),
    /// Rewards are burned (do not use).
    None,
}

impl RewardDestination {
    /// Get display label for UI.
    pub fn label(&self) -> &'static str {
        match self {
            RewardDestination::Staked => "Compound (Staked)",
            RewardDestination::Stash => "Stash Account",
            RewardDestination::Controller => "Controller Account",
            RewardDestination::Account(_) => "Custom Account",
            RewardDestination::None => "None (Burn)",
        }
    }
}

/// Transaction type for staking operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionType {
    /// Nominate validators.
    Nominate,
    /// Bond tokens for staking.
    Bond,
    /// Bond extra tokens to existing stake.
    BondExtra,
    /// Unbond tokens from stake.
    Unbond,
    /// Withdraw unbonded tokens.
    WithdrawUnbonded,
    /// Chill (stop nominating).
    Chill,
    /// Set controller account.
    SetController,
    /// Set payee for rewards.
    SetPayee,
    /// Rebond tokens that are unbonding.
    Rebond,
    /// Join nomination pool.
    PoolJoin,
    /// Bond extra to pool.
    PoolBondExtra,
    /// Claim pool rewards.
    PoolClaimPayout,
    /// Unbond from pool.
    PoolUnbond,
    /// Withdraw from pool.
    PoolWithdrawUnbonded,
}

impl TransactionType {
    /// Get display label for the transaction type.
    pub fn label(&self) -> &'static str {
        match self {
            TransactionType::Nominate => "Nominate",
            TransactionType::Bond => "Bond",
            TransactionType::BondExtra => "Bond Extra",
            TransactionType::Unbond => "Unbond",
            TransactionType::WithdrawUnbonded => "Withdraw Unbonded",
            TransactionType::Chill => "Chill",
            TransactionType::SetController => "Set Controller",
            TransactionType::SetPayee => "Set Payee",
            TransactionType::Rebond => "Rebond",
            TransactionType::PoolJoin => "Join Pool",
            TransactionType::PoolBondExtra => "Pool Bond Extra",
            TransactionType::PoolClaimPayout => "Claim Pool Rewards",
            TransactionType::PoolUnbond => "Pool Unbond",
            TransactionType::PoolWithdrawUnbonded => "Pool Withdraw",
        }
    }

    /// Get description for the transaction type.
    pub fn description(&self) -> &'static str {
        match self {
            TransactionType::Nominate => "Select validators to nominate",
            TransactionType::Bond => "Lock tokens for staking",
            TransactionType::BondExtra => "Add more tokens to existing stake",
            TransactionType::Unbond => "Start unbonding tokens (28 day wait)",
            TransactionType::WithdrawUnbonded => "Withdraw fully unbonded tokens",
            TransactionType::Chill => "Stop nominating validators",
            TransactionType::SetController => "Change controller account",
            TransactionType::SetPayee => "Change reward destination",
            TransactionType::Rebond => "Cancel unbonding and restake",
            TransactionType::PoolJoin => "Join a nomination pool",
            TransactionType::PoolBondExtra => "Add more tokens to pool stake",
            TransactionType::PoolClaimPayout => "Claim pending pool rewards",
            TransactionType::PoolUnbond => "Start unbonding from pool",
            TransactionType::PoolWithdrawUnbonded => "Withdraw unbonded pool tokens",
        }
    }
}

/// Transaction status in the signing/submission lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TransactionStatus {
    /// Transaction is being built.
    #[default]
    Building,
    /// Transaction is ready for signing.
    ReadyToSign,
    /// Waiting for signature (QR displayed).
    AwaitingSignature,
    /// Signature received, ready to submit.
    Signed,
    /// Transaction submitted, waiting for inclusion.
    Submitted,
    /// Transaction included in block.
    InBlock(String),
    /// Transaction finalized.
    Finalized(String),
    /// Transaction failed.
    Failed(String),
}

impl TransactionStatus {
    /// Check if transaction is pending (not yet finalized or failed).
    pub fn is_pending(&self) -> bool {
        !matches!(
            self,
            TransactionStatus::Finalized(_) | TransactionStatus::Failed(_)
        )
    }

    /// Get display label.
    pub fn label(&self) -> &'static str {
        match self {
            TransactionStatus::Building => "Building",
            TransactionStatus::ReadyToSign => "Ready to Sign",
            TransactionStatus::AwaitingSignature => "Awaiting Signature",
            TransactionStatus::Signed => "Signed",
            TransactionStatus::Submitted => "Submitted",
            TransactionStatus::InBlock(_) => "In Block",
            TransactionStatus::Finalized(_) => "Finalized",
            TransactionStatus::Failed(_) => "Failed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_token_symbols() {
        assert_eq!(Network::Polkadot.token_symbol(), "DOT");
        assert_eq!(Network::Kusama.token_symbol(), "KSM");
        assert_eq!(Network::Westend.token_symbol(), "WND");
        assert_eq!(Network::Paseo.token_symbol(), "PAS");
    }

    #[test]
    fn test_network_token_decimals() {
        assert_eq!(Network::Polkadot.token_decimals(), 10);
        assert_eq!(Network::Kusama.token_decimals(), 12);
        assert_eq!(Network::Westend.token_decimals(), 12);
        assert_eq!(Network::Paseo.token_decimals(), 10);
    }

    #[test]
    fn test_network_ss58_format() {
        assert_eq!(Network::Polkadot.ss58_format(), 0);
        assert_eq!(Network::Kusama.ss58_format(), 2);
        assert_eq!(Network::Westend.ss58_format(), 42);
        assert_eq!(Network::Paseo.ss58_format(), 0);
    }

    #[test]
    fn test_network_all() {
        let all = Network::all();
        assert_eq!(all.len(), 4);
        assert!(all.contains(&Network::Polkadot));
        assert!(all.contains(&Network::Kusama));
        assert!(all.contains(&Network::Westend));
        assert!(all.contains(&Network::Paseo));
    }

    #[test]
    fn test_network_display() {
        assert_eq!(format!("{}", Network::Polkadot), "Polkadot");
        assert_eq!(format!("{}", Network::Kusama), "Kusama");
        assert_eq!(format!("{}", Network::Westend), "Westend");
        assert_eq!(format!("{}", Network::Paseo), "Paseo");
    }

    #[test]
    fn test_pool_state_equality() {
        assert_eq!(PoolState::Open, PoolState::Open);
        assert_ne!(PoolState::Open, PoolState::Blocked);
        assert_ne!(PoolState::Blocked, PoolState::Destroying);
    }

    #[test]
    fn test_connection_status_equality() {
        assert_eq!(ConnectionStatus::Connected, ConnectionStatus::Connected);
        assert_ne!(ConnectionStatus::Connected, ConnectionStatus::Disconnected);
        assert_eq!(
            ConnectionStatus::Error("test".into()),
            ConnectionStatus::Error("test".into())
        );
    }
}
