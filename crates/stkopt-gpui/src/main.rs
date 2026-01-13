#![recursion_limit = "512"]

//! Staking Optimizer Desktop - A GPUI desktop application for Polkadot staking optimization.

pub mod account;
pub mod actions;
pub mod app;
pub mod chain;
pub mod db_service;
pub mod errors;
pub mod gpui_tokio;
pub mod history;
pub mod log;
pub mod optimization;
pub mod persistence;
pub mod qr_reader;
pub mod shortcuts;
pub mod tcc;
mod tests;
pub mod transactions;
pub mod validators;
pub mod views;

use app::StkoptApp;
use gpui::prelude::*;
use gpui_ui_kit::{MiniApp, MiniAppConfig};

use tracing_subscriber::prelude::*;

fn main() {
    // Create tokio runtime for async chain operations
    let runtime = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    let handle = runtime.handle().clone();

    // Setup tracing with DEBUG level filter (no TRACE)
    // LogBuffer is already Arc<Mutex<...>> internally, so we just clone the handle
    let logger = crate::log::LogBuffer::new();
    let app_logger = logger.clone(); // Cheap clone - just clones the inner Arc
    tracing_subscriber::registry()
        .with(crate::log::LogBufferLayer::new(logger))
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::filter::LevelFilter::DEBUG)
        .init();

    MiniApp::run(
        MiniAppConfig::new("Staking Optimizer")
            .size(1400.0, 900.0)
            .scrollable(false)
            .with_theme(true)
            .with_i18n(false),
        move |cx| {
            // Initialize gpui_tokio bridge with the runtime handle
            gpui_tokio::init_from_handle(cx, handle.clone());
            cx.new(|cx| StkoptApp::new(cx, app_logger.clone())) // LogBuffer clone is cheap (inner Arc)
        },
    );

    // Keep runtime alive - it will be dropped when main exits
    drop(runtime);
}
