//! Actions for state updates.

use stkopt_chain::{
    AccountBalance, NominatorInfo, PoolMembership, PoolState, StakingLedger, ValidatorExposure,
    ValidatorInfo,
};
use stkopt_core::{ConnectionStatus, EraIndex, EraInfo, Network, OptimizationResult};
use subxt::utils::AccountId32;

/// Aggregated validator data for display.
#[derive(Debug, Clone)]
pub struct DisplayValidator {
    pub address: String,
    pub commission: f64,
    pub blocked: bool,
    pub total_stake: u128,
    pub own_stake: u128,
    pub nominator_count: u32,
    #[allow(dead_code)]
    pub points: u32,
    pub apy: f64,
}

/// Nomination pool data for display.
#[derive(Debug, Clone)]
pub struct DisplayPool {
    pub id: u32,
    pub name: String,
    pub state: PoolState,
    pub member_count: u32,
    pub points: u128,
    /// Estimated APY based on nominated validators
    pub apy: Option<f64>,
}

/// Account status information for display.
#[derive(Debug, Clone)]
pub struct AccountStatus {
    pub address: AccountId32,
    pub balance: AccountBalance,
    pub staking_ledger: Option<StakingLedger>,
    pub nominations: Option<NominatorInfo>,
    pub pool_membership: Option<PoolMembership>,
}

/// Actions that can update application state.
#[derive(Debug, Clone)]
pub enum Action {
    /// Update connection status.
    UpdateConnectionStatus(ConnectionStatus),
    /// Set active era information.
    SetActiveEra(EraInfo),
    /// Set era duration in milliseconds.
    SetEraDuration(u64),
    /// Set registered validators.
    #[allow(dead_code)]
    SetValidators(Vec<ValidatorInfo>),
    /// Set validator exposures for an era.
    #[allow(dead_code)]
    SetEraExposures(EraIndex, Vec<ValidatorExposure>),
    /// Set total era reward.
    #[allow(dead_code)]
    SetEraReward(EraIndex, u128),
    /// Set display validators (aggregated data).
    SetDisplayValidators(Vec<DisplayValidator>),
    /// Set display pools (aggregated data).
    SetDisplayPools(Vec<DisplayPool>),
    /// Set loading progress.
    SetLoadingProgress(f32),
    /// Set the watched account address.
    SetWatchedAccount(AccountId32),
    /// Set account status (balance, staking, nominations).
    SetAccountStatus(Box<AccountStatus>),
    /// Clear the watched account.
    ClearAccount,
    /// Run validator optimization and get results.
    RunOptimization,
    /// Set optimization results.
    SetOptimizationResult(OptimizationResult),
    /// Toggle validator selection (for manual selection).
    ToggleValidatorSelection(usize),
    /// Clear nominations.
    ClearNominations,
    /// Generate QR code for nomination transaction.
    GenerateNominationQR,
    /// Set QR code data to display.
    SetQRData(Option<Vec<u8>>),
    /// Switch network.
    #[allow(dead_code)]
    SwitchNetwork(Network),
    /// Quit the application.
    #[allow(dead_code)]
    Quit,
}
