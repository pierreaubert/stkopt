//! Application configuration persistence.
//!
//! Stores user configuration (watched accounts) in a JSON file in the
//! platform-specific config directory. Never stores private keys.

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Application configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    /// Saved account addresses (public keys only, SS58 encoded).
    #[serde(default)]
    pub accounts: Vec<SavedAccount>,
}

/// A saved account (public key only).
#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl AppConfig {
    /// Get the config directory path.
    pub fn config_dir() -> Option<PathBuf> {
        ProjectDirs::from("xyz", "dotidx", "stkopt").map(|dirs| dirs.config_dir().to_path_buf())
    }

    /// Get the config file path.
    pub fn config_path() -> Option<PathBuf> {
        Self::config_dir().map(|dir| dir.join("config.json"))
    }

    /// Load configuration from disk.
    pub fn load() -> Self {
        let Some(path) = Self::config_path() else {
            tracing::warn!("Could not determine config directory");
            return Self::default();
        };

        if !path.exists() {
            tracing::debug!("No config file found at {:?}, using defaults", path);
            return Self::default();
        }

        match fs::read_to_string(&path) {
            Ok(contents) => match serde_json::from_str(&contents) {
                Ok(config) => {
                    tracing::info!("Loaded config from {:?}", path);
                    config
                }
                Err(e) => {
                    tracing::error!("Failed to parse config file: {} - using defaults", e);
                    Self::backup_corrupted_config(&path);
                    Self::default()
                }
            },
            Err(e) => {
                tracing::error!("Failed to read config file: {} - using defaults", e);
                Self::default()
            }
        }
    }

    /// Backup a corrupted config file for debugging.
    fn backup_corrupted_config(path: &std::path::PathBuf) {
        if let Some(parent) = path.parent() {
            let backup_path = parent.join(format!(
                "config.backup.{}",
                chrono::Utc::now().format("%Y%m%d_%H%M%S")
            ));
            if let Err(e) = fs::copy(path, &backup_path) {
                tracing::warn!(
                    "Failed to backup corrupted config to {:?}: {}",
                    backup_path,
                    e
                );
            } else {
                tracing::info!("Backed up corrupted config to {:?}", backup_path);
            }
        }
    }

    /// Save configuration to disk.
    pub fn save(&self) -> Result<(), String> {
        let Some(dir) = Self::config_dir() else {
            return Err("Could not determine config directory".to_string());
        };

        let path = dir.join("config.json");

        // Create directory if it doesn't exist
        if let Err(e) = fs::create_dir_all(&dir) {
            return Err(format!("Failed to create config directory: {}", e));
        }

        let contents = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;

        fs::write(&path, contents).map_err(|e| format!("Failed to write config: {}", e))?;

        tracing::info!("Saved config to {:?}", path);
        Ok(())
    }

    /// Add an account to the saved list.
    pub fn add_account(&mut self, address: String, label: Option<String>, network: Option<String>) {
        // Don't add duplicates
        if self.accounts.iter().any(|a| a.address == address) {
            tracing::debug!("Account {} already saved", address);
            return;
        }

        self.accounts.push(SavedAccount {
            address,
            label,
            network,
        });
    }

    /// Remove an account from the saved list.
    pub fn remove_account(&mut self, address: &str) {
        self.accounts.retain(|a| a.address != address);
    }

    /// Get the most recently added account address.
    pub fn last_account(&self) -> Option<&str> {
        self.accounts.last().map(|a| a.address.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = AppConfig::default();
        assert!(config.accounts.is_empty());
    }

    #[test]
    fn test_add_account() {
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
    fn test_no_duplicate_accounts() {
        let mut config = AppConfig::default();
        let addr = "15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5".to_string();
        config.add_account(addr.clone(), None, None);
        config.add_account(addr.clone(), None, None);
        assert_eq!(config.accounts.len(), 1);
    }

    #[test]
    fn test_remove_account() {
        let mut config = AppConfig::default();
        let addr = "15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5".to_string();
        config.add_account(addr.clone(), None, None);
        assert_eq!(config.accounts.len(), 1);
        config.remove_account(&addr);
        assert!(config.accounts.is_empty());
    }

    #[test]
    fn test_serialize_deserialize() {
        let mut config = AppConfig::default();
        config.add_account(
            "15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5".to_string(),
            Some("My Account".to_string()),
            Some("Polkadot".to_string()),
        );

        let json = serde_json::to_string(&config).unwrap();
        let loaded: AppConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.accounts.len(), 1);
        assert_eq!(loaded.accounts[0].label, Some("My Account".to_string()));
    }

    #[test]
    fn test_last_account_empty() {
        let config = AppConfig::default();
        assert!(config.last_account().is_none());
    }

    #[test]
    fn test_last_account() {
        let mut config = AppConfig::default();
        config.add_account("addr1".to_string(), None, None);
        config.add_account("addr2".to_string(), None, None);
        config.add_account("addr3".to_string(), None, None);

        assert_eq!(config.last_account(), Some("addr3"));
    }

    #[test]
    fn test_add_account_without_optional_fields() {
        let mut config = AppConfig::default();
        config.add_account("addr".to_string(), None, None);

        assert_eq!(config.accounts.len(), 1);
        assert_eq!(config.accounts[0].address, "addr");
        assert!(config.accounts[0].label.is_none());
        assert!(config.accounts[0].network.is_none());
    }

    #[test]
    fn test_remove_nonexistent_account() {
        let mut config = AppConfig::default();
        config.add_account("addr1".to_string(), None, None);

        // Should not panic when removing non-existent account
        config.remove_account("nonexistent");
        assert_eq!(config.accounts.len(), 1);
    }

    #[test]
    fn test_multiple_accounts() {
        let mut config = AppConfig::default();
        config.add_account(
            "addr1".to_string(),
            Some("Account 1".to_string()),
            Some("Polkadot".to_string()),
        );
        config.add_account(
            "addr2".to_string(),
            Some("Account 2".to_string()),
            Some("Kusama".to_string()),
        );
        config.add_account("addr3".to_string(), None, None);

        assert_eq!(config.accounts.len(), 3);
        assert_eq!(config.accounts[0].label, Some("Account 1".to_string()));
        assert_eq!(config.accounts[1].network, Some("Kusama".to_string()));
        assert!(config.accounts[2].label.is_none());
    }

    #[test]
    fn test_saved_account_clone() {
        let account = SavedAccount {
            address: "addr".to_string(),
            label: Some("Label".to_string()),
            network: Some("Polkadot".to_string()),
        };
        let account_clone = account.clone();

        assert_eq!(account.address, account_clone.address);
        assert_eq!(account.label, account_clone.label);
        assert_eq!(account.network, account_clone.network);
    }

    #[test]
    fn test_app_config_clone() {
        let mut config = AppConfig::default();
        config.add_account("addr".to_string(), Some("Label".to_string()), None);

        let config_clone = config.clone();
        assert_eq!(config.accounts.len(), config_clone.accounts.len());
        assert_eq!(config.accounts[0].address, config_clone.accounts[0].address);
    }

    #[test]
    fn test_config_dir() {
        // Just verify config_dir() returns something or None, doesn't panic
        let _dir = AppConfig::config_dir();
    }

    #[test]
    fn test_config_path() {
        // Just verify config_path() returns something or None, doesn't panic
        let _path = AppConfig::config_path();
    }

    #[test]
    fn test_deserialize_with_missing_optional_fields() {
        // JSON without optional fields should deserialize correctly
        let json = r#"{"accounts":[{"address":"addr1"}]}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();

        assert_eq!(config.accounts.len(), 1);
        assert_eq!(config.accounts[0].address, "addr1");
        assert!(config.accounts[0].label.is_none());
        assert!(config.accounts[0].network.is_none());
    }

    #[test]
    fn test_deserialize_empty_accounts() {
        let json = r#"{"accounts":[]}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert!(config.accounts.is_empty());
    }

    #[test]
    fn test_deserialize_missing_accounts() {
        // JSON without accounts field should use default (empty)
        let json = r#"{}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert!(config.accounts.is_empty());
    }

    #[test]
    fn test_serialize_pretty() {
        let mut config = AppConfig::default();
        config.add_account("addr".to_string(), Some("Label".to_string()), None);

        let json = serde_json::to_string_pretty(&config).unwrap();
        // Pretty-printed JSON should contain newlines
        assert!(json.contains('\n'));
    }

    #[test]
    fn test_chrono_usage() {
        // Test that chrono is properly imported and works (used in backup_corrupted_config)
        let now = chrono::Utc::now();
        let formatted = now.format("%Y%m%d_%H%M%S").to_string();
        assert_eq!(formatted.len(), 15); // YYYYMMDD_HHMMSS
    }
}
