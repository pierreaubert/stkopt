//! Persistence utilities for app configuration and caching.
//!
//! Re-exports the unified configuration system from stkopt-core.

pub use stkopt_core::config::{
    AddressBook, AddressBookEntry, AppConfig, ConnectionModeConfig, HistoryCache, NetworkConfig,
    ThemeConfig, ValidatorCache, get_address_book_path, get_config_path, get_data_dir,
    load_address_book, load_config, save_address_book, save_config,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_config_default() {
        let config = AppConfig::default();
        assert!(config.last_account.is_none());
        assert_eq!(config.network, NetworkConfig::Polkadot);
        assert!(config.auto_connect);
    }

    #[test]
    fn test_network_config_labels() {
        assert_eq!(NetworkConfig::Polkadot.label(), "Polkadot");
        assert_eq!(NetworkConfig::Kusama.label(), "Kusama");
    }

    #[test]
    fn test_network_config_is_testnet() {
        assert!(!NetworkConfig::Polkadot.is_testnet());
        assert!(!NetworkConfig::Kusama.is_testnet());
        assert!(NetworkConfig::Westend.is_testnet());
    }

    #[test]
    fn test_connection_mode_labels() {
        assert_eq!(ConnectionModeConfig::LightClient.label(), "Light Client");
        assert_eq!(ConnectionModeConfig::Rpc.label(), "RPC");
    }

    #[test]
    fn test_theme_config_labels() {
        assert_eq!(ThemeConfig::System.label(), "System");
        assert_eq!(ThemeConfig::Dark.label(), "Dark");
    }

    #[test]
    fn test_address_book_add() {
        let mut book = AddressBook::default();
        let entry = AddressBookEntry {
            address: "1abc".to_string(),
            label: "Test".to_string(),
            network: NetworkConfig::Polkadot,
            notes: None,
            created_at: 0,
        };

        assert!(book.add(entry.clone()).is_ok());
        assert_eq!(book.len(), 1);

        // Duplicate should fail
        assert!(book.add(entry).is_err());
    }

    #[test]
    fn test_address_book_remove() {
        let mut book = AddressBook::default();
        let entry = AddressBookEntry {
            address: "1abc".to_string(),
            label: "Test".to_string(),
            network: NetworkConfig::Polkadot,
            notes: None,
            created_at: 0,
        };

        book.add(entry).unwrap();
        assert!(book.remove("1abc"));
        assert!(book.is_empty());
        assert!(!book.remove("1abc")); // Already removed
    }

    #[test]
    fn test_address_book_find() {
        let mut book = AddressBook::default();
        let entry = AddressBookEntry {
            address: "1abc".to_string(),
            label: "Test".to_string(),
            network: NetworkConfig::Polkadot,
            notes: None,
            created_at: 0,
        };

        book.add(entry).unwrap();
        assert!(book.find("1abc").is_some());
        assert!(book.find("1def").is_none());
    }

    #[test]
    fn test_address_book_update_label() {
        let mut book = AddressBook::default();
        let entry = AddressBookEntry {
            address: "1abc".to_string(),
            label: "Test".to_string(),
            network: NetworkConfig::Polkadot,
            notes: None,
            created_at: 0,
        };

        book.add(entry).unwrap();
        assert!(book.update_label("1abc", "Updated".to_string()));
        assert_eq!(book.find("1abc").unwrap().label, "Updated");
    }

    #[test]
    fn test_address_book_for_network() {
        let mut book = AddressBook::default();

        book.add(AddressBookEntry {
            address: "1abc".to_string(),
            label: "Polkadot".to_string(),
            network: NetworkConfig::Polkadot,
            notes: None,
            created_at: 0,
        })
        .unwrap();

        book.add(AddressBookEntry {
            address: "Cabc".to_string(),
            label: "Kusama".to_string(),
            network: NetworkConfig::Kusama,
            notes: None,
            created_at: 0,
        })
        .unwrap();

        assert_eq!(book.for_network(NetworkConfig::Polkadot).len(), 1);
        assert_eq!(book.for_network(NetworkConfig::Kusama).len(), 1);
        assert_eq!(book.for_network(NetworkConfig::Westend).len(), 0);
    }

    #[test]
    fn test_validator_cache_staleness() {
        let cache = ValidatorCache {
            network: NetworkConfig::Polkadot,
            era: 100,
            cached_at: 0,
            validator_count: 297,
        };

        assert!(!cache.is_stale(100));
        assert!(cache.is_stale(101));
        assert!(!cache.is_expired(105));
        assert!(cache.is_expired(108));
    }

    #[test]
    fn test_history_cache_needs_update() {
        let cache = HistoryCache {
            address: "1abc".to_string(),
            network: NetworkConfig::Polkadot,
            latest_era: 100,
            era_count: 30,
            updated_at: 0,
        };

        assert!(!cache.needs_update(100));
        assert!(cache.needs_update(101));
        assert_eq!(cache.eras_to_fetch(105), 5);
    }

    #[test]
    fn test_get_data_dir() {
        // Should not panic
        let result = get_data_dir();
        assert!(result.is_ok());
    }

    #[test]
    fn test_config_serialization() {
        let config = AppConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.network, config.network);
    }

    #[test]
    fn test_address_book_serialization() {
        let mut book = AddressBook::default();
        book.add(AddressBookEntry {
            address: "1abc".to_string(),
            label: "Test".to_string(),
            network: NetworkConfig::Polkadot,
            notes: Some("Notes".to_string()),
            created_at: 12345,
        })
        .unwrap();

        let json = serde_json::to_string(&book).unwrap();
        let parsed: AddressBook = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(
            parsed.find("1abc").unwrap().notes,
            Some("Notes".to_string())
        );
    }
}

#[cfg(test)]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn test_address_book_add_remove_idempotent(address in "[a-zA-Z0-9]{10,48}") {
            let mut book = AddressBook::default();
            let entry = AddressBookEntry {
                address: address.clone(),
                label: "Test".to_string(),
                network: NetworkConfig::Polkadot,
                notes: None,
                created_at: 0,
            };

            prop_assert!(book.add(entry).is_ok());
            prop_assert_eq!(book.len(), 1);
            prop_assert!(book.remove(&address));
            prop_assert!(book.is_empty());
        }

        #[test]
        fn test_validator_cache_era_logic(era in 0u32..10000, offset in 0u32..100) {
            let cache = ValidatorCache {
                network: NetworkConfig::Polkadot,
                era,
                cached_at: 0,
                validator_count: 100,
            };

            let current = era.saturating_add(offset);

            if offset == 0 {
                prop_assert!(!cache.is_stale(current));
            } else {
                prop_assert!(cache.is_stale(current));
            }
        }

        #[test]
        fn test_history_cache_eras_to_fetch(latest in 0u32..10000, current in 0u32..10000) {
            let cache = HistoryCache {
                address: "test".to_string(),
                network: NetworkConfig::Polkadot,
                latest_era: latest,
                era_count: 30,
                updated_at: 0,
            };

            let to_fetch = cache.eras_to_fetch(current);
            if current > latest {
                prop_assert_eq!(to_fetch, current - latest);
            } else {
                prop_assert_eq!(to_fetch, 0);
            }
        }
    }
}
