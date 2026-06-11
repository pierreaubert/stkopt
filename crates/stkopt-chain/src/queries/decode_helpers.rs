//! Shared helpers for decoding values from chain storage queries.

use subxt::dynamic::Value;
use subxt::ext::scale_value::{Composite, ValueDef};
use subxt::utils::AccountId32;

/// Extract an AccountId32 from a dynamic Value.
pub fn extract_account_id(value: &Value) -> Option<AccountId32> {
    extract_account_bytes(value).map(AccountId32::from)
}

fn extract_account_bytes(value: &Value) -> Option<[u8; 32]> {
    let mut bytes = Vec::with_capacity(32);

    collect_account_bytes(value, &mut bytes);
    if bytes.len() == 32 {
        bytes.try_into().ok()
    } else {
        None
    }
}

fn collect_account_bytes(value: &Value, bytes: &mut Vec<u8>) {
    if bytes.len() > 32 {
        return;
    }

    match &value.value {
        ValueDef::Primitive(_) => {
            if let Some(byte) = value.as_u128()
                && byte <= u8::MAX as u128
            {
                bytes.push(byte as u8);
            }
        }
        ValueDef::Composite(Composite::Unnamed(values)) => {
            for child in values {
                collect_account_bytes(child, bytes);
                if bytes.len() > 32 {
                    break;
                }
            }
        }
        ValueDef::Composite(Composite::Named(values)) => {
            for (_, child) in values {
                collect_account_bytes(child, bytes);
                if bytes.len() > 32 {
                    break;
                }
            }
        }
        ValueDef::Variant(variant) => match &variant.values {
            Composite::Unnamed(values) => {
                for child in values {
                    collect_account_bytes(child, bytes);
                    if bytes.len() > 32 {
                        break;
                    }
                }
            }
            Composite::Named(values) => {
                for (_, child) in values {
                    collect_account_bytes(child, bytes);
                    if bytes.len() > 32 {
                        break;
                    }
                }
            }
        },
        ValueDef::BitSequence(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_account_id_rejects_wrong_length_without_panicking() {
        let value = Value::from_bytes([0u8; 33]);

        assert!(extract_account_id(&value).is_none());
    }

    #[test]
    fn extract_account_id_accepts_exactly_32_bytes() {
        let value = Value::from_bytes([7u8; 32]);

        assert_eq!(
            extract_account_id(&value),
            Some(AccountId32::from([7u8; 32]))
        );
    }
}
