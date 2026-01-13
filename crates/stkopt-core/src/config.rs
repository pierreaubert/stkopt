//! Application configuration and persistence utilities.
//!
//! This module provides a unified configuration system used by both TUI and GPUI frontends.
//! It supports all features from both implementations including:
//! - Network and connection mode preferences
//! - Theme configuration
//! - Address book for saved accounts
//! - Legacy account list for TUI compatibility
//! - Validator and history cache metadata

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::types::Network;

/// Configuration error type.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// Other configuration error.
    #[error("{0}")]
    Other(String),
}

/// Network configuration for UI preferences.
/// Maps to the core Network type but includes Custom for custom RPC endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum NetworkConfig {
    #[default]
    Polkadot,
    Kusama,
    Westend,
    Paseo,
    Custom,
}

impl NetworkConfig {
    /// Get display label.
    pub fn label(&self) -> &'static str {
        match self {
            NetworkConfig::Polkadot => "Polkadot",
            NetworkConfig::Kusama => "Kusama",
            NetworkConfig::Westend => "Westend",
            NetworkConfig::Paseo => "Paseo",
            NetworkConfig::Custom => "Custom",
        }
    }

    /// Check if this is a testnet.
    pub fn is_testnet(&self) -> bool {
        matches!(self, NetworkConfig::Westend | NetworkConfig::Paseo)
    }

    /// Convert to core Network type if not Custom.
    pub fn to_network(&self) -> Option<Network> {
        match self {
            NetworkConfig::Polkadot => Some(Network::Polkadot),
            NetworkConfig::Kusama => Some(Network::Kusama),
            NetworkConfig::Westend => Some(Network::Westend),
            NetworkConfig::Paseo => Some(Network::Paseo),
            NetworkConfig::Custom => None,
        }
    }
}

impl From<Network> for NetworkConfig {
    fn from(network: Network) -> Self {
        match network {
            Network::Polkadot => NetworkConfig::Polkadot,
            Network::Kusama => NetworkConfig::Kusama,
            Network::Westend => NetworkConfig::Westend,
            Network::Paseo => NetworkConfig::Paseo,
        }
    }
}

/// Connection mode configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ConnectionModeConfig {
    /// Use the embedded smoldot light client (default).
    #[default]
    LightClient,
    /// Use traditional RPC connection.
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
    /// Follow system preference.
    #[default]
    System,
    /// Force light theme.
    Light,
    /// Force dark theme.
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

/// A saved account (legacy format from TUI, kept for backwards compatibility).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedAccount {
    /// SS58-encoded address.
    pub address: String,
    /// Optional label/name for the account.
    #[serde(default)]
    pub label: Option<String>,
    /// Network this account was added on (for display purposes).
    #[serde(default)]
    pub network: Option<String>,
}

/// Address book entry (richer format from GPUI).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AddressBookEntry {
    /// SS58-encoded address.
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
    /// Add a new entry. Returns error if address already exists.
    pub fn add(&mut self, entry: AddressBookEntry) -> Result<(), ConfigError> {
        if self.entries.iter().any(|e| e.address == entry.address) {
            return Err(ConfigError::Other("Address already exists".to_string()));
        }
        self.entries.push(entry);
        Ok(())
    }

    /// Remove an entry by address. Returns true if an entry was removed.
    pub fn remove(&mut self, address: &str) -> bool {
        let len_before = self.entries.len();
        self.entries.retain(|e| e.address != address);
        self.entries.len() < len_before
    }

    /// Find entry by address.
    pub fn find(&self, address: &str) -> Option<&AddressBookEntry> {
        self.entries.iter().find(|e| e.address == address)
    }

    /// Update an entry's label. Returns true if successful.
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
        self.entries
            .iter()
            .filter(|e| e.network == network)
            .collect()
    }

    /// Get entry count.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get the most recently added entry.
    pub fn last(&self) -> Option<&AddressBookEntry> {
        self.entries.last()
    }
}

/// Application configuration.
///
/// This unified config supports both TUI and GPUI features:
/// - `accounts` field for TUI backwards compatibility
/// - All GPUI fields (network, connection_mode, theme, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Last watched account address.
    #[serde(default)]
    pub last_account: Option<String>,
    /// Selected network.
    #[serde(default)]
    pub network: NetworkConfig,
    /// Connection mode preference.
    #[serde(default)]
    pub connection_mode: ConnectionModeConfig,
    /// Custom RPC endpoint (if any).
    #[serde(default)]
    pub custom_rpc: Option<String>,
    /// Theme preference.
    #[serde(default)]
    pub theme: ThemeConfig,
    /// Auto-connect on startup.
    #[serde(default = "default_auto_connect")]
    pub auto_connect: bool,
    /// Show testnet networks.
    #[serde(default)]
    pub show_testnets: bool,
    /// Saved accounts (legacy TUI format).
    #[serde(default)]
    pub accounts: Vec<SavedAccount>,
}

fn default_auto_connect() -> bool {
    true
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
            accounts: Vec::new(),
        }
    }
}

impl AppConfig {
    /// Add an account to the legacy accounts list (for TUI compatibility).
    /// Does not add duplicates.
    pub fn add_account(&mut self, address: String, label: Option<String>, network: Option<String>) {
        if self.accounts.iter().any(|a| a.address == address) {
            return;
        }
        self.accounts.push(SavedAccount {
            address,
            label,
            network,
        });
    }

    /// Remove an account from the legacy accounts list.
    pub fn remove_account(&mut self, address: &str) {
        self.accounts.retain(|a| a.address != address);
    }

    /// Get the most recently added account address from legacy list.
    pub fn last_saved_account(&self) -> Option<&str> {
        self.accounts.last().map(|a| a.address.as_str())
    }
}

/// Cached validator metadata.
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

/// Cached history metadata.
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
        current_era.saturating_sub(self.latest_era)
    }
}

// ==================== Path Utilities ====================

/// Get the application data directory.
/// Uses platform-specific paths via `directories` crate.
pub fn get_data_dir() -> Result<PathBuf, ConfigError> {
    ProjectDirs::from("xyz", "dotidx", "stkopt")
        .map(|dirs| dirs.data_dir().to_path_buf())
        .ok_or_else(|| ConfigError::Other("Could not determine data directory".to_string()))
}

/// Get the config directory.
pub fn get_config_dir() -> Result<PathBuf, ConfigError> {
    ProjectDirs::from("xyz", "dotidx", "stkopt")
        .map(|dirs| dirs.config_dir().to_path_buf())
        .ok_or_else(|| ConfigError::Other("Could not determine config directory".to_string()))
}

/// Get the database file path.
pub fn get_db_path() -> Result<PathBuf, ConfigError> {
    get_data_dir().map(|dir| dir.join("history.db"))
}

/// Get the config file path.
pub fn get_config_path() -> Result<PathBuf, ConfigError> {
    get_config_dir().map(|dir| dir.join("config.json"))
}

/// Get the address book file path.
pub fn get_address_book_path() -> Result<PathBuf, ConfigError> {
    get_data_dir().map(|dir| dir.join("address_book.json"))
}

// ==================== Config I/O ====================

/// Load configuration from disk.
pub fn load_config() -> Result<AppConfig, ConfigError> {
    let path = get_config_path()?;
    if !path.exists() {
        return Ok(AppConfig::default());
    }

    let content = fs::read_to_string(&path)?;
    let config = serde_json::from_str(&content)?;
    Ok(config)
}

/// Save configuration to disk.
pub fn save_config(config: &AppConfig) -> Result<(), ConfigError> {
    let path = get_config_path()?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let content = serde_json::to_string_pretty(config)?;
    fs::write(&path, content)?;
    Ok(())
}

/// Load address book from disk.
pub fn load_address_book() -> Result<AddressBook, ConfigError> {
    let path = get_address_book_path()?;
    if !path.exists() {
        return Ok(AddressBook::default());
    }

    let content = fs::read_to_string(&path)?;
    let book = serde_json::from_str(&content)?;
    Ok(book)
}

/// Save address book to disk.
pub fn save_address_book(book: &AddressBook) -> Result<(), ConfigError> {
    let path = get_address_book_path()?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let content = serde_json::to_string_pretty(book)?;
    fs::write(&path, content)?;
    Ok(())
}

/// Backup a corrupted config file for debugging.
pub fn backup_corrupted_config(path: &PathBuf) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        let backup_path = parent.join(format!(
            "config.backup.{}",
            chrono::Utc::now().format("%Y%m%d_%H%M%S")
        ));
        fs::copy(path, &backup_path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== NetworkConfig Tests ====================

    #[test]
    fn test_network_config_default() {
        let config: NetworkConfig = Default::default();
        assert_eq!(config, NetworkConfig::Polkadot);
    }

    #[test]
    fn test_network_config_labels() {
        assert_eq!(NetworkConfig::Polkadot.label(), "Polkadot");
        assert_eq!(NetworkConfig::Kusama.label(), "Kusama");
        assert_eq!(NetworkConfig::Westend.label(), "Westend");
        assert_eq!(NetworkConfig::Paseo.label(), "Paseo");
        assert_eq!(NetworkConfig::Custom.label(), "Custom");
    }

    #[test]
    fn test_network_config_is_testnet() {
        assert!(!NetworkConfig::Polkadot.is_testnet());
        assert!(!NetworkConfig::Kusama.is_testnet());
        assert!(NetworkConfig::Westend.is_testnet());
        assert!(NetworkConfig::Paseo.is_testnet());
        assert!(!NetworkConfig::Custom.is_testnet());
    }

    #[test]
    fn test_network_config_to_network() {
        assert_eq!(
            NetworkConfig::Polkadot.to_network(),
            Some(Network::Polkadot)
        );
        assert_eq!(NetworkConfig::Kusama.to_network(), Some(Network::Kusama));
        assert_eq!(NetworkConfig::Westend.to_network(), Some(Network::Westend));
        assert_eq!(NetworkConfig::Paseo.to_network(), Some(Network::Paseo));
        assert_eq!(NetworkConfig::Custom.to_network(), None);
    }

    #[test]
    fn test_network_config_from_network() {
        assert_eq!(
            NetworkConfig::from(Network::Polkadot),
            NetworkConfig::Polkadot
        );
        assert_eq!(NetworkConfig::from(Network::Kusama), NetworkConfig::Kusama);
        assert_eq!(
            NetworkConfig::from(Network::Westend),
            NetworkConfig::Westend
        );
        assert_eq!(NetworkConfig::from(Network::Paseo), NetworkConfig::Paseo);
    }

    // ==================== ConnectionModeConfig Tests ====================

    #[test]
    fn test_connection_mode_default() {
        let mode: ConnectionModeConfig = Default::default();
        assert_eq!(mode, ConnectionModeConfig::LightClient);
    }

    #[test]
    fn test_connection_mode_labels() {
        assert_eq!(ConnectionModeConfig::LightClient.label(), "Light Client");
        assert_eq!(ConnectionModeConfig::Rpc.label(), "RPC");
    }

    // ==================== ThemeConfig Tests ====================

    #[test]
    fn test_theme_config_default() {
        let theme: ThemeConfig = Default::default();
        assert_eq!(theme, ThemeConfig::System);
    }

    #[test]
    fn test_theme_config_labels() {
        assert_eq!(ThemeConfig::System.label(), "System");
        assert_eq!(ThemeConfig::Light.label(), "Light");
        assert_eq!(ThemeConfig::Dark.label(), "Dark");
    }

    // ==================== AppConfig Tests ====================

    #[test]
    fn test_app_config_default() {
        let config = AppConfig::default();
        assert!(config.last_account.is_none());
        assert_eq!(config.network, NetworkConfig::Polkadot);
        assert_eq!(config.connection_mode, ConnectionModeConfig::LightClient);
        assert_eq!(config.theme, ThemeConfig::System);
        assert!(config.auto_connect);
        assert!(!config.show_testnets);
        assert!(config.accounts.is_empty());
    }

    #[test]
    fn test_app_config_add_account() {
        let mut config = AppConfig::default();
        config.add_account(
            "15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5".to_string(),
            Some("Test".to_string()),
            Some("Polkadot".to_string()),
        );
        assert_eq!(config.accounts.len(), 1);
        assert_eq!(
            config.accounts[0].address,
            "15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5"
        );
    }

    #[test]
    fn test_app_config_no_duplicate_accounts() {
        let mut config = AppConfig::default();
        let addr = "15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5".to_string();
        config.add_account(addr.clone(), None, None);
        config.add_account(addr.clone(), None, None);
        assert_eq!(config.accounts.len(), 1);
    }

    #[test]
    fn test_app_config_remove_account() {
        let mut config = AppConfig::default();
        let addr = "15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5".to_string();
        config.add_account(addr.clone(), None, None);
        assert_eq!(config.accounts.len(), 1);
        config.remove_account(&addr);
        assert!(config.accounts.is_empty());
    }

    #[test]
    fn test_app_config_last_saved_account() {
        let mut config = AppConfig::default();
        assert!(config.last_saved_account().is_none());

        config.add_account("addr1".to_string(), None, None);
        config.add_account("addr2".to_string(), None, None);
        assert_eq!(config.last_saved_account(), Some("addr2"));
    }

    #[test]
    fn test_app_config_serialization() {
        let config = AppConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.network, config.network);
        assert_eq!(parsed.auto_connect, config.auto_connect);
    }

    #[test]
    fn test_app_config_deserialize_missing_fields() {
        // JSON without optional fields should deserialize correctly
        let json = r#"{"network":"Polkadot"}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.network, NetworkConfig::Polkadot);
        assert!(config.accounts.is_empty());
    }

    #[test]
    fn test_app_config_deserialize_legacy() {
        // Legacy TUI format with just accounts
        let json = r#"{"accounts":[{"address":"addr1"}]}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.accounts.len(), 1);
        assert_eq!(config.accounts[0].address, "addr1");
    }

    // ==================== AddressBook Tests ====================

    #[test]
    fn test_address_book_default() {
        let book = AddressBook::default();
        assert!(book.is_empty());
        assert_eq!(book.len(), 0);
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
        assert!(!book.update_label("nonexistent", "Label".to_string()));
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
    fn test_address_book_last() {
        let mut book = AddressBook::default();
        assert!(book.last().is_none());

        book.add(AddressBookEntry {
            address: "1abc".to_string(),
            label: "First".to_string(),
            network: NetworkConfig::Polkadot,
            notes: None,
            created_at: 0,
        })
        .unwrap();

        book.add(AddressBookEntry {
            address: "2def".to_string(),
            label: "Second".to_string(),
            network: NetworkConfig::Polkadot,
            notes: None,
            created_at: 0,
        })
        .unwrap();

        assert_eq!(book.last().unwrap().address, "2def");
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

    // ==================== ValidatorCache Tests ====================

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

    // ==================== HistoryCache Tests ====================

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
        assert_eq!(cache.eras_to_fetch(100), 0);
        assert_eq!(cache.eras_to_fetch(50), 0);
    }

    // ==================== Path Utility Tests ====================

    #[test]
    fn test_get_data_dir() {
        let result = get_data_dir();
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_config_dir() {
        let result = get_config_dir();
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_db_path() {
        let result = get_db_path();
        assert!(result.is_ok());
        assert!(result.unwrap().to_string_lossy().contains("history.db"));
    }

    #[test]
    fn test_get_config_path() {
        let result = get_config_path();
        assert!(result.is_ok());
        assert!(result.unwrap().to_string_lossy().contains("config.json"));
    }

    #[test]
    fn test_get_address_book_path() {
        let result = get_address_book_path();
        assert!(result.is_ok());
        assert!(
            result
                .unwrap()
                .to_string_lossy()
                .contains("address_book.json")
        );
    }

    // ==================== SavedAccount Tests ====================

    #[test]
    fn test_saved_account_clone() {
        let account = SavedAccount {
            address: "addr".to_string(),
            label: Some("Label".to_string()),
            network: Some("Polkadot".to_string()),
        };
        let cloned = account.clone();
        assert_eq!(account.address, cloned.address);
        assert_eq!(account.label, cloned.label);
    }

    #[test]
    fn test_saved_account_serialization() {
        let account = SavedAccount {
            address: "addr".to_string(),
            label: Some("Label".to_string()),
            network: None,
        };
        let json = serde_json::to_string(&account).unwrap();
        let parsed: SavedAccount = serde_json::from_str(&json).unwrap();
        assert_eq!(account.address, parsed.address);
    }
}
