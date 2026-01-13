//! Application configuration persistence.
//!
//! This module re-exports the unified configuration from stkopt-core.
//! Uses the same config file format and storage location as the GPUI app.

pub use stkopt_core::config::{load_config, save_config};
