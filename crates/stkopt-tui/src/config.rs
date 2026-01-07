//! Application configuration persistence.
//!
//! Stores user configuration (watched accounts) in a JSON file in the
//! platform-specific config directory. Never stores private keys.

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[cfg(test)]
use chrono::TimeZone;

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
}
