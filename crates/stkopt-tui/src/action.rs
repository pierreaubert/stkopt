//! Actions for state updates.

use stkopt_chain::{
    AccountBalance, ChainInfo, NominatorInfo, PoolMembership, PoolState, RewardDestination,
    StakingLedger, UnsignedPayload, ValidatorExposure, ValidatorInfo,
};
use stkopt_core::{ConnectionStatus, EraIndex, EraInfo, Network, OptimizationResult};
use subxt::utils::AccountId32;

/// Input mode for staking operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StakingInputMode {
    #[default]
    None,
    Bond,
    Unbond,
    BondExtra,
    SetPayee,
    PoolJoin,
    PoolUnbond,
    PoolBondExtra,
}

/// Pool operation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PoolOperation {
    #[default]
    None,
    Join,
    BondExtra,
    Claim,
    Unbond,
    Withdraw,
}

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

/// Status of transaction submission.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum TxSubmissionStatus {
    /// Waiting to scan signature QR code.
    WaitingForSignature,
    /// Signature scanned, ready to submit.
    ReadyToSubmit,
    /// Submitting to network.
    Submitting,
    /// Included in a block (not yet finalized).
    InBlock { block_hash: [u8; 32] },
    /// Finalized in a block.
    Finalized { block_hash: [u8; 32] },
    /// Submission failed.
    Failed(String),
}

/// QR scan status for visual feedback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QrScanStatus {
    /// Scanning, no QR detected.
    Scanning,
    /// QR code detected but not decoded.
    Detected,
    /// Successfully decoded.
    Success,
}

/// Pending unsigned transaction (waiting for signature from Vault).
#[derive(Debug, Clone)]
pub struct PendingUnsignedTx {
    /// The unsigned payload.
    pub payload: UnsignedPayload,
    /// The signer account.
    pub signer: AccountId32,
}

/// Signed transaction ready for submission.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PendingTransaction {
    /// The unsigned payload (for reference).
    pub description: String,
    /// The signed extrinsic bytes.
    pub signed_extrinsic: Vec<u8>,
    /// The transaction hash.
    pub tx_hash: [u8; 32],
    /// Current submission status.
    pub status: TxSubmissionStatus,
}

/// Actions that can update application state.
#[derive(Debug, Clone)]
#[allow(dead_code)]
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
    /// Set loading progress (progress 0.0-1.0, bytes_loaded, estimated_total_bytes).
    SetLoadingProgress(f32, Option<u64>, Option<u64>),
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

    // === Staking Operations ===
    /// Generate QR for bonding.
    GenerateBondQR { value: u128 },
    /// Generate QR for unbonding.
    GenerateUnbondQR { value: u128 },
    /// Generate QR for bonding extra.
    GenerateBondExtraQR { value: u128 },
    /// Generate QR for setting payee.
    GenerateSetPayeeQR { destination: RewardDestination },
    /// Generate QR for withdrawing unbonded.
    GenerateWithdrawUnbondedQR,
    /// Generate QR for chill.
    GenerateChillQR,

    // === Pool Operations ===
    /// Generate QR for joining a pool.
    GeneratePoolJoinQR { pool_id: u32, amount: u128 },
    /// Generate QR for bonding extra to pool.
    GeneratePoolBondExtraQR { amount: u128 },
    /// Generate QR for claiming pool rewards.
    GeneratePoolClaimQR,
    /// Generate QR for unbonding from pool.
    GeneratePoolUnbondQR { amount: u128 },
    /// Generate QR for withdrawing unbonded from pool.
    GeneratePoolWithdrawQR,

    // === UI State Updates ===
    /// Set the input mode for staking operations.
    SetStakingInputMode(StakingInputMode),
    /// Update the staking amount input.
    UpdateStakingAmount(String),
    /// Set the selected reward destination.
    SetRewardsDestination(RewardDestination),
    /// Set the current pool operation.
    SetPoolOperation(PoolOperation),
    /// Select a pool for joining.
    SelectPoolForJoin(usize),

    /// Set QR code data to display (raw bytes + transaction info).
    SetQRData(Option<Vec<u8>>, Option<TransactionInfo>),
    /// Store the pending unsigned transaction for later signature.
    SetPendingUnsignedTx(Option<PendingUnsignedTx>),
    /// Start scanning for signed transaction QR from Vault.
    StartSignatureScan,
    /// Stop scanning for signature.
    StopSignatureScan,
    /// Signature scanned from Vault QR code (raw bytes).
    SignatureScanned(Vec<u8>),
    /// QR scan failed with error message.
    QrScanFailed(String),
    /// Update QR scan status for visual feedback.
    UpdateScanStatus(QrScanStatus),
    /// Update camera preview for braille display.
    /// Contains: (pixels, width, height, qr_bounds).
    UpdateCameraPreview(Vec<u8>, usize, usize, Option<[(f32, f32); 4]>),
    /// Submit the signed transaction to the network.
    SubmitTransaction,
    /// Update transaction submission status.
    SetTxStatus(TxSubmissionStatus),
    /// Clear the pending transaction.
    ClearPendingTx,
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
    /// Validate an account address and show error if invalid.
    ValidateAccount(String),
    /// Clear the validation error message.
    ClearValidationError,
    /// Quit the application.
    #[allow(dead_code)]
    Quit,
}
