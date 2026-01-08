//! Actions for state updates.

use stkopt_chain::{
    AccountBalance, ChainInfo, NominatorInfo, PoolMembership, PoolState, StakingLedger,
    ValidatorExposure, ValidatorInfo,
};
use stkopt_core::{ConnectionStatus, EraIndex, EraInfo, Network, OptimizationResult};
use subxt::utils::AccountId32;

/// Aggregated validator data for display.
#[derive(Debug, Clone)]
pub struct DisplayValidator {
    pub address: String,
    /// Display name from identity (if available).
    pub name: Option<String>,
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

/// A single point in staking history.
#[derive(Debug, Clone)]
pub struct StakingHistoryPoint {
    /// Era index.
    pub era: u32,
    /// Estimated date string (YYYYMMDD format).
    pub date: String,
    /// Reward earned in this era (in planck).
    pub reward: u128,
    /// Total bonded amount at start of era.
    pub bonded: u128,
    /// APY for this era (as a ratio, e.g., 0.15 for 15%).
    pub apy: f64,
}

/// Transaction info for QR code display.
#[derive(Debug, Clone)]
pub struct TransactionInfo {
    /// Signing account.
    pub signer: String,
    /// Call description (e.g., "Staking.nominate").
    pub call: String,
    /// Target validator addresses.
    pub targets: Vec<String>,
    /// Call data size in bytes.
    pub call_data_size: usize,
    /// Spec version.
    pub spec_version: u32,
    pub tx_version: u32,
    /// Account nonce.
    pub nonce: u64,
    /// Whether CheckMetadataHash extension is included.
    pub include_metadata_hash: bool,
}

/// Actions that can update application state.
#[derive(Debug, Clone)]
pub enum Action {
    /// Update connection status.
    UpdateConnectionStatus(ConnectionStatus),
    /// Set chain info (name, spec version, validation status).
    SetChainInfo(ChainInfo),
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
    /// Run optimization with specific strategy (0=TopApy, 1=RandomFromTop, 2=DiversifyByStake).
    RunOptimizationWithStrategy(usize),
    /// Set optimization results.
    SetOptimizationResult(OptimizationResult),
    /// Toggle validator selection (for manual selection).
    ToggleValidatorSelection(usize),
    /// Clear nominations.
    ClearNominations,
    /// Generate QR code for nomination transaction.
    GenerateNominationQR,
    /// Set QR code data to display (raw bytes + transaction info).
    SetQRData(Option<Vec<u8>>, Option<TransactionInfo>),
    /// Set staking history for the watched account (replaces all).
    SetStakingHistory(Vec<StakingHistoryPoint>),
    /// Add a single staking history point (streaming).
    AddStakingHistoryPoint(StakingHistoryPoint),
    /// Start loading staking history.
    LoadStakingHistory,
    /// Cancel loading staking history.
    CancelLoadingHistory,
    /// Mark history loading as complete.
    HistoryLoadingComplete,
    /// Switch network.
    #[allow(dead_code)]
    SwitchNetwork(Network),
    /// Select an entry from the address book (by index).
    SelectAddressBookEntry(usize),
    /// Remove an account from the address book and purge its history.
    RemoveAccount(String),
    /// Quit the application.
    #[allow(dead_code)]
    Quit,
}
