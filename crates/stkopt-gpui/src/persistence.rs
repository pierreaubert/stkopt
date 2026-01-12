//! Persistence utilities for app configuration and caching.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Last watched account address.
    pub last_account: Option<String>,
    /// Selected network.
    pub network: NetworkConfig,
    /// Connection mode preference.
    pub connection_mode: ConnectionModeConfig,
    /// Custom RPC endpoint (if any).
    pub custom_rpc: Option<String>,
    /// Theme preference.
    pub theme: ThemeConfig,
    /// Auto-connect on startup.
    pub auto_connect: bool,
    /// Show testnet networks.
    pub show_testnets: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            last_account: None,
            network: NetworkConfig::Polkadot,
            connection_mode: ConnectionModeConfig::LightClient,
            custom_rpc: None,
            theme: ThemeConfig::System,
            auto_connect: true,
            show_testnets: false,
        }
    }
}

/// Network configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum NetworkConfig {
    #[default]
    Polkadot,
    Kusama,
    Westend,
    Custom,
}

impl NetworkConfig {
    /// Get display label.
    pub fn label(&self) -> &'static str {
        match self {
            NetworkConfig::Polkadot => "Polkadot",
            NetworkConfig::Kusama => "Kusama",
            NetworkConfig::Westend => "Westend",
            NetworkConfig::Custom => "Custom",
        }
    }

    /// Check if this is a testnet.
    pub fn is_testnet(&self) -> bool {
        matches!(self, NetworkConfig::Westend)
    }
}

/// Connection mode configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ConnectionModeConfig {
    #[default]
    LightClient,
    Rpc,
}

impl ConnectionModeConfig {
    /// Get display label.
    pub fn label(&self) -> &'static str {
        match self {
            ConnectionModeConfig::LightClient => "Light Client",
            ConnectionModeConfig::Rpc => "RPC",
        }
    }
}

/// Theme configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeConfig {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeConfig {
    /// Get display label.
    pub fn label(&self) -> &'static str {
        match self {
            ThemeConfig::System => "System",
            ThemeConfig::Light => "Light",
            ThemeConfig::Dark => "Dark",
        }
    }
}

/// Address book entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AddressBookEntry {
    /// SS58 address.
    pub address: String,
    /// User-defined label.
    pub label: String,
    /// Network this address belongs to.
    pub network: NetworkConfig,
    /// Optional notes.
    pub notes: Option<String>,
    /// Creation timestamp (Unix seconds).
    pub created_at: u64,
}

/// Address book for saved accounts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AddressBook {
    /// List of saved addresses.
    pub entries: Vec<AddressBookEntry>,
}

impl AddressBook {
    /// Add a new entry.
    pub fn add(&mut self, entry: AddressBookEntry) -> Result<(), String> {
        if self.entries.iter().any(|e| e.address == entry.address) {
            return Err("Address already exists".to_string());
        }
        self.entries.push(entry);
        Ok(())
    }

    /// Remove an entry by address.
    pub fn remove(&mut self, address: &str) -> bool {
        let len_before = self.entries.len();
        self.entries.retain(|e| e.address != address);
        self.entries.len() < len_before
    }

    /// Find entry by address.
    pub fn find(&self, address: &str) -> Option<&AddressBookEntry> {
        self.entries.iter().find(|e| e.address == address)
    }

    /// Update an entry's label.
    pub fn update_label(&mut self, address: &str, label: String) -> bool {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.address == address) {
            entry.label = label;
            true
        } else {
            false
        }
    }

    /// Get entries for a specific network.
    pub fn for_network(&self, network: NetworkConfig) -> Vec<&AddressBookEntry> {
        self.entries.iter().filter(|e| e.network == network).collect()
    }

    /// Get entry count.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Get the application data directory.
/// Uses the same path as the TUI app so they share config and cache.
pub fn get_data_dir() -> Result<PathBuf, String> {
    directories::ProjectDirs::from("xyz", "dotidx", "stkopt")
        .map(|dirs| dirs.data_dir().to_path_buf())
        .ok_or_else(|| "Could not determine data directory".to_string())
}

/// Get the config file path.
pub fn get_config_path() -> Result<PathBuf, String> {
    get_data_dir().map(|dir| dir.join("config.json"))
}

/// Get the address book file path.
pub fn get_address_book_path() -> Result<PathBuf, String> {
    get_data_dir().map(|dir| dir.join("address_book.json"))
}

/// Load configuration from disk.
pub fn load_config() -> Result<AppConfig, String> {
    let path = get_config_path()?;
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read config: {}", e))?;
    
    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse config: {}", e))
}

/// Save configuration to disk.
pub fn save_config(config: &AppConfig) -> Result<(), String> {
    let path = get_config_path()?;
    
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }
    
    let content = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    
    std::fs::write(&path, content)
        .map_err(|e| format!("Failed to write config: {}", e))
}

/// Load address book from disk.
pub fn load_address_book() -> Result<AddressBook, String> {
    let path = get_address_book_path()?;
    if !path.exists() {
        return Ok(AddressBook::default());
    }
    
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read address book: {}", e))?;
    
    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse address book: {}", e))
}

/// Save address book to disk.
pub fn save_address_book(book: &AddressBook) -> Result<(), String> {
    let path = get_address_book_path()?;
    
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;
    }
    
    let content = serde_json::to_string_pretty(book)
        .map_err(|e| format!("Failed to serialize address book: {}", e))?;
    
    std::fs::write(&path, content)
        .map_err(|e| format!("Failed to write address book: {}", e))
}

/// Cached validator data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorCache {
    /// Network this cache is for.
    pub network: NetworkConfig,
    /// Era when cache was created.
    pub era: u32,
    /// Timestamp when cache was created (Unix seconds).
    pub cached_at: u64,
    /// Number of validators in cache.
    pub validator_count: usize,
}

impl ValidatorCache {
    /// Check if cache is stale (older than 1 era / ~24 hours).
    pub fn is_stale(&self, current_era: u32) -> bool {
        current_era > self.era
    }

    /// Check if cache is expired (older than 7 eras).
    pub fn is_expired(&self, current_era: u32) -> bool {
        current_era > self.era + 7
    }
}

/// Cached history data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryCache {
    /// Account address.
    pub address: String,
    /// Network.
    pub network: NetworkConfig,
    /// Latest era in cache.
    pub latest_era: u32,
    /// Number of eras cached.
    pub era_count: usize,
    /// Timestamp when cache was updated.
    pub updated_at: u64,
}

impl HistoryCache {
    /// Check if cache needs update (missing recent eras).
    pub fn needs_update(&self, current_era: u32) -> bool {
        current_era > self.latest_era
    }

    /// Get number of eras to fetch.
    pub fn eras_to_fetch(&self, current_era: u32) -> u32 {
        if current_era > self.latest_era {
            current_era - self.latest_era
        } else {
            0
        }
    }
}

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
        }).unwrap();
        
        book.add(AddressBookEntry {
            address: "Cabc".to_string(),
            label: "Kusama".to_string(),
            network: NetworkConfig::Kusama,
            notes: None,
            created_at: 0,
        }).unwrap();

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
        }).unwrap();

        let json = serde_json::to_string(&book).unwrap();
        let parsed: AddressBook = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed.find("1abc").unwrap().notes, Some("Notes".to_string()));
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
