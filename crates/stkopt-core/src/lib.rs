//! Core domain logic and persistence for staking optimization.
//!
//! This crate provides:
//! - APY calculations (`apy` module)
//! - Validator selection optimization (`optimizer` module)
//! - Core domain types (`types` module)
//! - Display types for UI (`display` module)
//!
//! With the `persistence` feature enabled:
//! - SQLite database for caching (`db` module)
//! - Configuration management (`config` module)

pub mod apy;
pub mod display;
pub mod optimizer;
pub mod types;

#[cfg(feature = "persistence")]
pub mod config;
#[cfg(feature = "persistence")]
pub mod db;

// Re-export commonly used items from core modules
pub use apy::*;
pub use display::*;
pub use optimizer::*;
pub use types::*;

// Re-export key persistence types when feature is enabled
#[cfg(feature = "persistence")]
pub use config::{
    AddressBook, AddressBookEntry, AppConfig, ConfigError, ConnectionModeConfig, HistoryCache,
    NetworkConfig, SavedAccount, ThemeConfig, ValidatorCache,
};
#[cfg(feature = "persistence")]
pub use db::{
    AccountStatusService, CacheFreshness, CacheKind, CachePolicy, CacheSnapshot, Cached,
    CachedAccountStatus, CachedChainMetadata, DEFAULT_ACCOUNT_CACHE_MAX_AGE_SECS,
    DEFAULT_HISTORY_MAX_APY, DEFAULT_IDENTITY_MAX_AGE_SECS, DEFAULT_STARTUP_CACHE_MAX_AGE_SECS,
    DEFAULT_STARTUP_CACHE_MAX_ERA_LAG, HistoryService, StakingDb, StartupDataCache,
    StartupDataService,
};
