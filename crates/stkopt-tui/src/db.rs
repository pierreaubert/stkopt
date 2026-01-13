//! SQLite database for caching staking history, validator data, and chain metadata.
//!
//! This module re-exports the unified database layer from stkopt-core.
//! The `HistoryDb` type alias maintains backwards compatibility with existing TUI code.

pub use stkopt_core::db::StakingDb;

/// Type alias for backwards compatibility with existing TUI code.
pub type HistoryDb = StakingDb;
