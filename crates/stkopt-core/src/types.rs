//! Core domain types for staking optimization.

pub type Balance = u128;
pub type EraIndex = u32;

/// Supported networks - exhaustive match required (no default case).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
