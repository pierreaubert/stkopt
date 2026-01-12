//! SS58 address encoding utilities.

use subxt::utils::AccountId32;

const SS58_PREFIX: &[u8] = b"SS58PRE";

/// Encode an AccountId32 with a specific SS58 prefix.
pub fn encode_ss58(account: &AccountId32, prefix: u16) -> String {
    let account_bytes: &[u8; 32] = account.as_ref();

    // Build the payload for checksum
    let mut payload = Vec::new();

    // Add prefix bytes
    if prefix < 64 {
        payload.push(prefix as u8);
    } else if prefix < 16384 {
        // Two-byte encoding for larger prefixes
        let first = ((prefix & 0x00FC) >> 2) as u8 | 0x40;
        let second = ((prefix >> 8) as u8) | ((prefix & 0x03) << 6) as u8;
        payload.push(first);
        payload.push(second);
    } else {
        // Unsupported prefix, fall back to generic
        payload.push(42);
    }

    // Add account bytes
    payload.extend_from_slice(account_bytes);

    // Calculate checksum using blake2b-512
    let mut checksum_input = Vec::new();
    checksum_input.extend_from_slice(SS58_PREFIX);
    checksum_input.extend_from_slice(&payload);

    let hash = sp_crypto_hashing::blake2_512(&checksum_input);

    // Add first 2 bytes of hash as checksum
    payload.push(hash[0]);
    payload.push(hash[1]);

    // Base58 encode
    bs58::encode(payload).into_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_encode_ss58_polkadot() {
        // Known Polkadot address
        let addr = "15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5";
        let account = AccountId32::from_str(addr).unwrap();

        // Re-encode with Polkadot prefix (0)
        let encoded = encode_ss58(&account, 0);
        assert_eq!(encoded, addr);
    }

    #[test]
    fn test_encode_ss58_kusama() {
        // Parse a generic address and encode as Kusama
        let addr = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY"; // Alice
        let account = AccountId32::from_str(addr).unwrap();

        // Encode with Kusama prefix (2)
        let encoded = encode_ss58(&account, 2);
        // Kusama Alice address
        assert_eq!(encoded, "HNZata7iMYWmk5RvZRTiAsSDhV8366zq2YGb3tLH5Upf74F");
    }

    #[test]
    fn test_encode_ss58_roundtrip() {
        let original = "15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5";
        let account = AccountId32::from_str(original).unwrap();

        // Encode with different prefixes and verify they all decode to same account
        let polkadot = encode_ss58(&account, 0);
        let kusama = encode_ss58(&account, 2);
        let westend = encode_ss58(&account, 42);

        assert_eq!(AccountId32::from_str(&polkadot).unwrap(), account);
        assert_eq!(AccountId32::from_str(&kusama).unwrap(), account);
        assert_eq!(AccountId32::from_str(&westend).unwrap(), account);
    }
}
