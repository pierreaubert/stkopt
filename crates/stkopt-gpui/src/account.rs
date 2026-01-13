//! Account management and SS58 address validation.
//!
//! This module provides utilities for validating and managing Polkadot accounts.

use subxt::utils::AccountId32;

/// Result of address validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationResult {
    /// Address is valid.
    Valid(AccountId32),
    /// Address is invalid with error message.
    Invalid(String),
    /// Address is empty (not an error, just nothing entered).
    Empty,
}

impl ValidationResult {
    /// Returns true if the validation result is valid.
    pub fn is_valid(&self) -> bool {
        matches!(self, ValidationResult::Valid(_))
    }

    /// Returns the account ID if valid.
    pub fn account_id(&self) -> Option<&AccountId32> {
        match self {
            ValidationResult::Valid(id) => Some(id),
            _ => None,
        }
    }

    /// Returns the error message if invalid.
    pub fn error_message(&self) -> Option<&str> {
        match self {
            ValidationResult::Invalid(msg) => Some(msg),
            _ => None,
        }
    }
}

/// Validate an SS58 address string.
///
/// Returns a `ValidationResult` indicating whether the address is valid,
/// invalid (with a helpful error message), or empty.
pub fn validate_address(input: &str) -> ValidationResult {
    let input = input.trim();

    if input.is_empty() {
        return ValidationResult::Empty;
    }

    if input.len() < 3 {
        return ValidationResult::Invalid(
            "Address is too short (minimum 3 characters)".to_string(),
        );
    }

    // SS58 addresses start with alphanumeric characters
    // Polkadot: starts with 1
    // Kusama: starts with uppercase letters (C, D, E, F, G, H, J)
    // Westend: starts with 5
    let first_char = input.chars().next().unwrap();
    if !first_char.is_ascii_alphanumeric() {
        return ValidationResult::Invalid(
            "Address should start with an alphanumeric character".to_string(),
        );
    }

    // Try to parse as SS58 address
    match input.parse::<AccountId32>() {
        Ok(account) => ValidationResult::Valid(account),
        Err(_) => {
            let hint = if input.contains(|c: char| !c.is_ascii_alphanumeric() && c != '-') {
                "Remove special characters from the address".to_string()
            } else if input.len() < 47 {
                format!(
                    "SS58 address too short (got {}, expected ~47 characters)",
                    input.len()
                )
            } else if input.len() > 49 {
                format!(
                    "SS58 address too long (got {}, expected ~47 characters)",
                    input.len()
                )
            } else {
                "Invalid SS58 format - check for typos".to_string()
            };
            ValidationResult::Invalid(format!("Invalid address: {}", hint))
        }
    }
}

/// Truncate an address for display (e.g., "1abc...xyz").
pub fn truncate_address(address: &str, prefix_len: usize, suffix_len: usize) -> String {
    if address.len() <= prefix_len + suffix_len + 3 {
        return address.to_string();
    }
    format!(
        "{}...{}",
        &address[..prefix_len],
        &address[address.len() - suffix_len..]
    )
}

/// Format an account ID as a string.
pub fn format_account_id(account: &AccountId32) -> String {
    account.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Valid Polkadot address for testing
    const VALID_POLKADOT_ADDRESS: &str = "15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5";

    #[test]
    fn test_validate_empty_address() {
        assert_eq!(validate_address(""), ValidationResult::Empty);
        assert_eq!(validate_address("   "), ValidationResult::Empty);
    }

    #[test]
    fn test_validate_too_short() {
        let result = validate_address("ab");
        assert!(matches!(result, ValidationResult::Invalid(_)));
        assert!(result.error_message().unwrap().contains("too short"));
    }

    #[test]
    fn test_validate_invalid_format() {
        // This address starts with valid alphanumeric but is not valid SS58
        let result = validate_address("abc123456789012345678901234567890123456789012345");
        assert!(matches!(result, ValidationResult::Invalid(_)));
        // Should fail SS58 parsing, not the prefix check
        assert!(result.error_message().unwrap().contains("Invalid"));
    }

    #[test]
    fn test_validate_special_char_prefix() {
        // Address starting with special character should be rejected
        let result = validate_address("@abc123456789012345678901234567890123456789012345");
        assert!(matches!(result, ValidationResult::Invalid(_)));
        assert!(result.error_message().unwrap().contains("alphanumeric"));
    }

    #[test]
    fn test_validate_valid_address() {
        let result = validate_address(VALID_POLKADOT_ADDRESS);
        assert!(result.is_valid());
        assert!(result.account_id().is_some());
    }

    #[test]
    fn test_validate_address_with_whitespace() {
        let result = validate_address(&format!("  {}  ", VALID_POLKADOT_ADDRESS));
        assert!(result.is_valid());
    }

    #[test]
    fn test_validate_invalid_characters() {
        let result = validate_address("1abc!@#$%^&*()");
        assert!(matches!(result, ValidationResult::Invalid(_)));
        assert!(
            result
                .error_message()
                .unwrap()
                .contains("special characters")
        );
    }

    #[test]
    fn test_validate_too_long() {
        let result =
            validate_address("1abcdefghijklmnopqrstuvwxyz1234567890abcdefghijklmnopqrstuvwxyz");
        assert!(matches!(result, ValidationResult::Invalid(_)));
        assert!(result.error_message().unwrap().contains("too long"));
    }

    #[test]
    fn test_truncate_address() {
        let addr = "15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5";
        let truncated = truncate_address(addr, 6, 6);
        assert_eq!(truncated, "15oF4u...Hr6Sp5");
    }

    #[test]
    fn test_truncate_short_address() {
        let addr = "short";
        let truncated = truncate_address(addr, 6, 6);
        assert_eq!(truncated, "short");
    }

    #[test]
    fn test_validation_result_is_valid() {
        assert!(ValidationResult::Valid(VALID_POLKADOT_ADDRESS.parse().unwrap()).is_valid());
        assert!(!ValidationResult::Invalid("error".to_string()).is_valid());
        assert!(!ValidationResult::Empty.is_valid());
    }

    #[test]
    fn test_validation_result_error_message() {
        assert_eq!(
            ValidationResult::Invalid("test error".to_string()).error_message(),
            Some("test error")
        );
        assert_eq!(ValidationResult::Empty.error_message(), None);
        assert_eq!(
            ValidationResult::Valid(VALID_POLKADOT_ADDRESS.parse().unwrap()).error_message(),
            None
        );
    }
}

#[cfg(test)]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn test_empty_input_always_empty(s in "\\s*") {
            let result = validate_address(&s);
            prop_assert!(matches!(result, ValidationResult::Empty));
        }

        #[test]
        fn test_short_input_always_invalid(s in "[a-zA-Z0-9]{1,2}") {
            let result = validate_address(&s);
            prop_assert!(matches!(result, ValidationResult::Invalid(_) | ValidationResult::Empty));
        }

        #[test]
        fn test_truncate_never_panics(
            addr in "[a-zA-Z0-9]{0,100}",
            prefix in 0usize..20,
            suffix in 0usize..20
        ) {
            let _ = truncate_address(&addr, prefix, suffix);
        }

        #[test]
        fn test_validation_result_consistency(input in ".*") {
            let result = validate_address(&input);
            match &result {
                ValidationResult::Valid(_) => {
                    prop_assert!(result.is_valid());
                    prop_assert!(result.account_id().is_some());
                    prop_assert!(result.error_message().is_none());
                }
                ValidationResult::Invalid(_) => {
                    prop_assert!(!result.is_valid());
                    prop_assert!(result.account_id().is_none());
                    prop_assert!(result.error_message().is_some());
                }
                ValidationResult::Empty => {
                    prop_assert!(!result.is_valid());
                    prop_assert!(result.account_id().is_none());
                    prop_assert!(result.error_message().is_none());
                }
            }
        }
    }
}

#[cfg(test)]
mod negative_tests {
    use super::*;

    #[test]
    fn test_invalid_base58_characters() {
        // Base58 doesn't include 0, O, I, l
        let result = validate_address("1OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO");
        assert!(matches!(result, ValidationResult::Invalid(_)));
    }

    #[test]
    fn test_wrong_checksum() {
        // Valid format but wrong checksum (changed last character)
        let result = validate_address("15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp6");
        assert!(matches!(result, ValidationResult::Invalid(_)));
    }

    #[test]
    fn test_unicode_input() {
        let result = validate_address("1αβγδεζηθικλμνξοπρστυφχψω");
        assert!(matches!(result, ValidationResult::Invalid(_)));
    }

    #[test]
    fn test_null_bytes() {
        let result = validate_address("1abc\0def");
        assert!(matches!(result, ValidationResult::Invalid(_)));
    }
}
