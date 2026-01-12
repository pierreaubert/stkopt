//! Transaction utilities for staking operations.

/// Transaction type for staking operations.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// Reward destination for staking payouts.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RewardDestination {
    /// Compound rewards back to staked balance.
    #[default]
    Staked,
    /// Send rewards to stash account.
    Stash,
    /// Send rewards to controller account.
    Controller,
    /// Send rewards to a custom account.
    Account(String),
    /// Do not receive rewards.
    None,
}

impl RewardDestination {
    /// Get display label.
    pub fn label(&self) -> &'static str {
        match self {
            RewardDestination::Staked => "Compound (Staked)",
            RewardDestination::Stash => "Stash Account",
            RewardDestination::Controller => "Controller Account",
            RewardDestination::Account(_) => "Custom Account",
            RewardDestination::None => "None",
        }
    }
}

/// Transaction status.
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
        !matches!(self, TransactionStatus::Finalized(_) | TransactionStatus::Failed(_))
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

/// Unsigned transaction ready for signing.
#[derive(Debug, Clone)]
pub struct UnsignedTransaction {
    /// Transaction type.
    pub tx_type: TransactionType,
    /// Encoded call data (hex).
    pub call_data: String,
    /// Human-readable description.
    pub description: String,
    /// Estimated fee in planck.
    pub estimated_fee: u128,
    /// Nonce for the transaction.
    pub nonce: u32,
    /// Genesis hash.
    pub genesis_hash: String,
    /// Block hash for mortality.
    pub block_hash: String,
    /// Spec version.
    pub spec_version: u32,
    /// Transaction version.
    pub tx_version: u32,
}

/// Build a nominate transaction.
pub fn build_nominate_tx(
    validators: &[String],
    nonce: u32,
    genesis_hash: &str,
    block_hash: &str,
    spec_version: u32,
    tx_version: u32,
) -> UnsignedTransaction {
    let description = format!("Nominate {} validators", validators.len());
    let call_data = encode_nominate_call(validators);

    UnsignedTransaction {
        tx_type: TransactionType::Nominate,
        call_data,
        description,
        estimated_fee: 10_000_000, // 0.001 DOT estimate
        nonce,
        genesis_hash: genesis_hash.to_string(),
        block_hash: block_hash.to_string(),
        spec_version,
        tx_version,
    }
}

/// Build a bond transaction.
pub fn build_bond_tx(
    amount: u128,
    payee: &RewardDestination,
    nonce: u32,
    genesis_hash: &str,
    block_hash: &str,
    spec_version: u32,
    tx_version: u32,
) -> UnsignedTransaction {
    let dot_amount = amount as f64 / 10_000_000_000.0;
    let description = format!("Bond {:.4} DOT", dot_amount);
    let call_data = encode_bond_call(amount, payee);

    UnsignedTransaction {
        tx_type: TransactionType::Bond,
        call_data,
        description,
        estimated_fee: 10_000_000,
        nonce,
        genesis_hash: genesis_hash.to_string(),
        block_hash: block_hash.to_string(),
        spec_version,
        tx_version,
    }
}

/// Build an unbond transaction.
pub fn build_unbond_tx(
    amount: u128,
    nonce: u32,
    genesis_hash: &str,
    block_hash: &str,
    spec_version: u32,
    tx_version: u32,
) -> UnsignedTransaction {
    let dot_amount = amount as f64 / 10_000_000_000.0;
    let description = format!("Unbond {:.4} DOT", dot_amount);
    let call_data = encode_unbond_call(amount);

    UnsignedTransaction {
        tx_type: TransactionType::Unbond,
        call_data,
        description,
        estimated_fee: 10_000_000,
        nonce,
        genesis_hash: genesis_hash.to_string(),
        block_hash: block_hash.to_string(),
        spec_version,
        tx_version,
    }
}

/// Encode nominate call data (mock implementation).
fn encode_nominate_call(validators: &[String]) -> String {
    // In real implementation, this would use SCALE encoding
    // For now, return a mock hex string
    let mut data = String::from("0x0705"); // Staking.nominate call index
    data.push_str(&format!("{:02x}", validators.len()));
    for v in validators {
        data.push_str(&v[..8.min(v.len())]);
    }
    data
}

/// Encode bond call data (mock implementation).
fn encode_bond_call(amount: u128, payee: &RewardDestination) -> String {
    let payee_byte = match payee {
        RewardDestination::Staked => "00",
        RewardDestination::Stash => "01",
        RewardDestination::Controller => "02",
        RewardDestination::Account(_) => "03",
        RewardDestination::None => "04",
    };
    format!("0x0700{:032x}{}", amount, payee_byte)
}

/// Encode unbond call data (mock implementation).
fn encode_unbond_call(amount: u128) -> String {
    format!("0x0702{:032x}", amount)
}

/// QR code data format for Polkadot Vault.
#[derive(Debug, Clone)]
pub struct QrPayload {
    /// Payload type (sign transaction).
    pub payload_type: u8,
    /// Encoded payload bytes.
    pub payload: Vec<u8>,
}

impl QrPayload {
    /// Create a new QR payload for signing.
    pub fn for_signing(tx: &UnsignedTransaction) -> Self {
        // Simplified payload construction
        let mut payload = Vec::new();
        
        // Add call data
        if let Some(hex_data) = tx.call_data.strip_prefix("0x") {
            if let Ok(bytes) = hex::decode(hex_data) {
                payload.extend_from_slice(&bytes);
            }
        }
        
        // Add metadata
        payload.extend_from_slice(&tx.nonce.to_le_bytes());
        payload.extend_from_slice(&tx.spec_version.to_le_bytes());
        payload.extend_from_slice(&tx.tx_version.to_le_bytes());

        Self {
            payload_type: 0x00, // Sign transaction
            payload,
        }
    }

    /// Encode payload to bytes for QR code generation.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![self.payload_type];
        bytes.extend_from_slice(&self.payload);
        bytes
    }

    /// Encode payload to hex string.
    pub fn to_hex(&self) -> String {
        hex::encode(self.to_bytes())
    }

    /// Estimate QR code size needed.
    pub fn estimated_qr_size(&self) -> usize {
        let bytes = self.payload.len();
        if bytes <= 50 {
            1 // Single QR
        } else if bytes <= 200 {
            2 // 2 QR codes
        } else if bytes <= 500 {
            4 // 4 QR codes
        } else {
            (bytes / 100) + 1 // Approximate
        }
    }
}

/// Signed transaction ready for submission.
#[derive(Debug, Clone)]
pub struct SignedTransaction {
    /// Original unsigned transaction.
    pub unsigned: UnsignedTransaction,
    /// Signature bytes (hex).
    pub signature: String,
    /// Full signed extrinsic (hex).
    pub signed_extrinsic: String,
}

/// Parse a signed transaction from QR code data.
pub fn parse_signed_qr(qr_data: &[u8]) -> Result<String, String> {
    if qr_data.is_empty() {
        return Err("Empty QR data".to_string());
    }

    // Check for valid signature prefix
    if qr_data[0] != 0x01 {
        return Err("Invalid signature payload type".to_string());
    }

    // Extract signature (simplified)
    if qr_data.len() < 65 {
        return Err("Signature too short".to_string());
    }

    Ok(hex::encode(&qr_data[1..]))
}

/// Validate transaction parameters.
pub fn validate_tx_params(
    tx_type: &TransactionType,
    amount: Option<u128>,
    validators: Option<&[String]>,
) -> Result<(), String> {
    match tx_type {
        TransactionType::Nominate => {
            let validators = validators.ok_or("Validators required for nominate")?;
            if validators.is_empty() {
                return Err("At least one validator required".to_string());
            }
            if validators.len() > 16 {
                return Err("Maximum 16 validators allowed".to_string());
            }
        }
        TransactionType::Bond | TransactionType::BondExtra | TransactionType::Unbond | TransactionType::Rebond => {
            let amount = amount.ok_or("Amount required")?;
            if amount == 0 {
                return Err("Amount must be greater than 0".to_string());
            }
        }
        TransactionType::PoolJoin | TransactionType::PoolBondExtra | TransactionType::PoolUnbond => {
            let amount = amount.ok_or("Amount required for pool operation")?;
            if amount == 0 {
                return Err("Amount must be greater than 0".to_string());
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_type_labels() {
        assert_eq!(TransactionType::Nominate.label(), "Nominate");
        assert_eq!(TransactionType::Bond.label(), "Bond");
        assert_eq!(TransactionType::PoolJoin.label(), "Join Pool");
    }

    #[test]
    fn test_transaction_type_descriptions() {
        assert!(!TransactionType::Nominate.description().is_empty());
        assert!(!TransactionType::Unbond.description().is_empty());
    }

    #[test]
    fn test_reward_destination_labels() {
        assert_eq!(RewardDestination::Staked.label(), "Compound (Staked)");
        assert_eq!(RewardDestination::Account("test".to_string()).label(), "Custom Account");
    }

    #[test]
    fn test_transaction_status_is_pending() {
        assert!(TransactionStatus::Building.is_pending());
        assert!(TransactionStatus::Submitted.is_pending());
        assert!(!TransactionStatus::Finalized("hash".to_string()).is_pending());
        assert!(!TransactionStatus::Failed("error".to_string()).is_pending());
    }

    #[test]
    fn test_transaction_status_labels() {
        assert_eq!(TransactionStatus::Building.label(), "Building");
        assert_eq!(TransactionStatus::Finalized("hash".to_string()).label(), "Finalized");
    }

    #[test]
    fn test_build_nominate_tx() {
        let validators = vec!["1abc".to_string(), "1def".to_string()];
        let tx = build_nominate_tx(&validators, 0, "genesis", "block", 1, 1);

        assert_eq!(tx.tx_type, TransactionType::Nominate);
        assert!(tx.description.contains("2 validators"));
        assert!(tx.call_data.starts_with("0x0705"));
    }

    #[test]
    fn test_build_bond_tx() {
        let tx = build_bond_tx(
            10_000_000_000, // 1 DOT
            &RewardDestination::Staked,
            0,
            "genesis",
            "block",
            1,
            1,
        );

        assert_eq!(tx.tx_type, TransactionType::Bond);
        assert!(tx.description.contains("1.0000 DOT"));
        assert!(tx.call_data.starts_with("0x0700"));
    }

    #[test]
    fn test_build_unbond_tx() {
        let tx = build_unbond_tx(
            5_000_000_000, // 0.5 DOT
            0,
            "genesis",
            "block",
            1,
            1,
        );

        assert_eq!(tx.tx_type, TransactionType::Unbond);
        assert!(tx.description.contains("0.5000 DOT"));
    }

    #[test]
    fn test_qr_payload_creation() {
        let tx = build_nominate_tx(&["1abc".to_string()], 0, "genesis", "block", 1, 1);
        let payload = QrPayload::for_signing(&tx);

        assert_eq!(payload.payload_type, 0x00);
        assert!(!payload.payload.is_empty());
    }

    #[test]
    fn test_qr_payload_to_hex() {
        let tx = build_nominate_tx(&["1abc".to_string()], 0, "genesis", "block", 1, 1);
        let payload = QrPayload::for_signing(&tx);
        let hex = payload.to_hex();

        assert!(hex.starts_with("00")); // Payload type
    }

    #[test]
    fn test_qr_payload_size_estimation() {
        let tx = build_nominate_tx(&["1abc".to_string()], 0, "genesis", "block", 1, 1);
        let payload = QrPayload::for_signing(&tx);

        assert!(payload.estimated_qr_size() >= 1);
    }

    #[test]
    fn test_parse_signed_qr_empty() {
        let result = parse_signed_qr(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_signed_qr_invalid_type() {
        let result = parse_signed_qr(&[0x00, 0x01, 0x02]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid signature"));
    }

    #[test]
    fn test_parse_signed_qr_too_short() {
        let result = parse_signed_qr(&[0x01, 0x02, 0x03]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("too short"));
    }

    #[test]
    fn test_parse_signed_qr_valid() {
        let mut data = vec![0x01]; // Signature type
        data.extend_from_slice(&[0u8; 64]); // 64 byte signature
        let result = parse_signed_qr(&data);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_tx_params_nominate() {
        let validators = vec!["1abc".to_string()];
        assert!(validate_tx_params(&TransactionType::Nominate, None, Some(&validators)).is_ok());
    }

    #[test]
    fn test_validate_tx_params_nominate_empty() {
        let validators: Vec<String> = vec![];
        let result = validate_tx_params(&TransactionType::Nominate, None, Some(&validators));
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_tx_params_nominate_too_many() {
        let validators: Vec<String> = (0..20).map(|i| format!("1{}", i)).collect();
        let result = validate_tx_params(&TransactionType::Nominate, None, Some(&validators));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Maximum 16"));
    }

    #[test]
    fn test_validate_tx_params_bond() {
        assert!(validate_tx_params(&TransactionType::Bond, Some(1000), None).is_ok());
    }

    #[test]
    fn test_validate_tx_params_bond_zero() {
        let result = validate_tx_params(&TransactionType::Bond, Some(0), None);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_tx_params_bond_missing() {
        let result = validate_tx_params(&TransactionType::Bond, None, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_tx_params_chill() {
        // Chill doesn't require any params
        assert!(validate_tx_params(&TransactionType::Chill, None, None).is_ok());
    }
}

#[cfg(test)]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn test_build_nominate_never_panics(
            count in 1usize..20,
            nonce in any::<u32>(),
            spec in any::<u32>(),
            tx_ver in any::<u32>()
        ) {
            let validators: Vec<String> = (0..count).map(|i| format!("1{:047}", i)).collect();
            let _ = build_nominate_tx(&validators, nonce, "genesis", "block", spec, tx_ver);
        }

        #[test]
        fn test_build_bond_never_panics(
            amount in any::<u128>(),
            nonce in any::<u32>()
        ) {
            let _ = build_bond_tx(amount, &RewardDestination::Staked, nonce, "genesis", "block", 1, 1);
        }

        #[test]
        fn test_qr_payload_never_panics(count in 0usize..10) {
            let validators: Vec<String> = (0..count.max(1)).map(|i| format!("1{}", i)).collect();
            let tx = build_nominate_tx(&validators, 0, "genesis", "block", 1, 1);
            let payload = QrPayload::for_signing(&tx);
            let _ = payload.to_hex();
            let _ = payload.estimated_qr_size();
        }

        #[test]
        fn test_validate_nominate_count(count in 0usize..30) {
            let validators: Vec<String> = (0..count).map(|i| format!("1{}", i)).collect();
            let result = validate_tx_params(&TransactionType::Nominate, None, Some(&validators));
            
            if count == 0 {
                prop_assert!(result.is_err());
            } else if count > 16 {
                prop_assert!(result.is_err());
            } else {
                prop_assert!(result.is_ok());
            }
        }
    }
}
