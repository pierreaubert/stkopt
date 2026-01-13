//! Transaction utilities for staking operations.
//!
//! Core types (TransactionType, RewardDestination, TransactionStatus) are
//! defined in stkopt-core and re-exported here for convenience.
//!
//! Transaction payloads are built via the chain worker using stkopt-chain.

// Re-export core transaction types
pub use stkopt_core::{RewardDestination, TransactionStatus, TransactionType};

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
        TransactionType::Bond
        | TransactionType::BondExtra
        | TransactionType::Unbond
        | TransactionType::Rebond => {
            let amount = amount.ok_or("Amount required")?;
            if amount == 0 {
                return Err("Amount must be greater than 0".to_string());
            }
        }
        TransactionType::PoolJoin
        | TransactionType::PoolBondExtra
        | TransactionType::PoolUnbond => {
            let amount = amount.ok_or("Amount required for pool operation")?;
            if amount == 0 {
                return Err("Amount must be greater than 0".to_string());
            }
        }
        TransactionType::WithdrawUnbonded
        | TransactionType::Chill
        | TransactionType::SetController
        | TransactionType::SetPayee
        | TransactionType::PoolClaimPayout
        | TransactionType::PoolWithdrawUnbonded => {
            // These operations don't require amount or validators
        }
    }
    Ok(())
}

/// Parse a signed transaction from QR code data.
///
/// Expected format: 0x01 prefix followed by signature bytes.
pub fn parse_signed_qr(qr_data: &[u8]) -> Result<String, String> {
    if qr_data.is_empty() {
        return Err("Empty QR data".to_string());
    }

    // Check for valid signature prefix
    if qr_data[0] != 0x01 {
        return Err("Invalid signature payload type".to_string());
    }

    // Extract signature (simplified - real implementation would validate length based on signature scheme)
    if qr_data.len() < 65 {
        return Err("Signature too short".to_string());
    }

    Ok(hex::encode(&qr_data[1..]))
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
        assert_eq!(
            RewardDestination::Account("test".to_string()).label(),
            "Custom Account"
        );
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
        assert_eq!(
            TransactionStatus::Finalized("hash".to_string()).label(),
            "Finalized"
        );
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

    #[test]
    fn test_validate_tx_params_withdraw_unbonded() {
        // WithdrawUnbonded doesn't require any params
        assert!(validate_tx_params(&TransactionType::WithdrawUnbonded, None, None).is_ok());
    }

    #[test]
    fn test_validate_tx_params_pool_join() {
        assert!(validate_tx_params(&TransactionType::PoolJoin, Some(1000), None).is_ok());
        assert!(validate_tx_params(&TransactionType::PoolJoin, Some(0), None).is_err());
        assert!(validate_tx_params(&TransactionType::PoolJoin, None, None).is_err());
    }
}
