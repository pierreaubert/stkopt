//! Shared helpers for decoding values from chain storage queries.

use subxt::dynamic::{At, Value};
use subxt::utils::AccountId32;

/// Extract an AccountId32 from a dynamic Value by iterating its byte elements.
pub fn extract_account_id(value: &Value<u32>) -> Option<AccountId32> {
    let mut bytes = Vec::with_capacity(32);
    let mut i = 0;
    while let Some(byte_val) = value.at(i) {
        if let Some(byte) = byte_val.as_u128() {
            bytes.push(byte as u8);
        }
        i += 1;
    }

    if bytes.len() == 32 {
        let arr: [u8; 32] = bytes.try_into().ok()?;
        Some(AccountId32::from(arr))
    } else {
        None
    }
}
