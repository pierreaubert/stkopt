#![recursion_limit = "512"]

//! Staking Optimizer Desktop - A GPUI desktop application for Polkadot staking optimization.

mod account;
mod actions;
mod app;
mod chain;
mod db;
mod db_service;
mod errors;
mod gpui_tokio;
mod history;
mod log;
mod optimization;
mod persistence;
mod shortcuts;
mod transactions;
mod validators;
mod views;
mod tests;


use app::StkoptApp;
use gpui::prelude::*;
use gpui_ui_kit::{MiniApp, MiniAppConfig};

use tracing_subscriber::prelude::*;

fn main() {
    // Initialize logging
    let log_buffer = std::sync::Arc::new(crate::log::LogBuffer::new());
    let log_layer = crate::log::LogBufferLayer::new((*log_buffer).clone()); // LogBuffer is Clone, but we want shared Arc for app
    // Wait, LogBufferLayer takes LogBuffer struct, which wraps Arc.
    // So log_buffer (Arc) is for App.
    // log_layer needs a LogBuffer instance. 
    // `crate::log::LogBuffer` implements Clone (cloning the internal Arc).
    // So `log_layer` gets a clone of the struct (cheap).
    // `log_buffer` variable can be the struct itself?
    // StkoptApp expects `Arc<LogBuffer>`. 
    // Ah, `app.rs` expects `Arc<crate::log::LogBuffer>`.
    // But `LogBuffer` itself is `Arc<Mutex<..>>` wrapper.
    // If `LogBuffer` is `Clone`, `Arc<LogBuffer>` is double indirection?
    // `log.rs`: `pub struct LogBuffer { inner: Arc<Mutex<VecDeque>> }`.
    // So `LogBuffer` IS a cheap handle.
    // `StkoptApp` should verify if it takes `LogBuffer` or `Arc<LogBuffer>`.
    // I defined `app.rs` as `Arc<crate::log::LogBuffer>`. This is double Arc.
    // It's fine, just overhead.
    // But better if `StkoptApp` took `LogBuffer` directly.
    // I can change `app.rs` later if needed, or just wrap it.
    // Let's assume `app.rs` takes `Arc`.
    // So:
    // val buffer = crate::log::LogBuffer::new(); // struct with Arc inside
    // val app_buffer = std::sync::Arc::new(buffer.clone()); // outer Arc
    // registry.with(LogBufferLayer::new(buffer)).init();
    
    // Create tokio runtime for async chain operations
    let runtime = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    let handle = runtime.handle().clone();

    // Setup tracing with DEBUG level filter (no TRACE)
    let logger = crate::log::LogBuffer::new();
    let app_logger = std::sync::Arc::new(logger.clone());
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
            cx.new(|cx| StkoptApp::new(cx, app_logger.clone()))
        },
    );

    // Keep runtime alive - it will be dropped when main exits
    drop(runtime);
}
