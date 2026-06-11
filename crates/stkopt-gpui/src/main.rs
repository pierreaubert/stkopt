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
pub mod theme;
pub mod transactions;
pub mod validators;
pub mod views;

use app::StkoptApp;
use gpui::prelude::*;
use gpui_ui_kit::{MiniApp, MiniAppConfig};

use tracing_subscriber::EnvFilter;
use tracing_subscriber::prelude::*;

fn main() {
    let config = crate::persistence::load_config().unwrap_or_default();
    let initial_theme = crate::theme::theme_variant_for_config(config.theme);

    // Create tokio runtime for async chain operations
    let runtime = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    let handle = runtime.handle().clone();

    // Setup tracing with DEBUG level filter (no TRACE) while suppressing
    // smoldot's local light-client chatter.
    // LogBuffer is already Arc<Mutex<...>> internally, so we just clone the handle
    let logger = crate::log::LogBuffer::new();
    let app_logger = logger.clone(); // Cheap clone - just clones the inner Arc
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug"));
    let env_filter = suppress_light_client_chatter(env_filter);
    tracing_subscriber::registry()
        .with(env_filter)
        .with(crate::log::LogBufferLayer::new(logger))
        .with(tracing_subscriber::fmt::layer())
        .init();

    MiniApp::run(
        MiniAppConfig::new("Staking Optimizer")
            .size(1400.0, 900.0)
            .scrollable(false)
            .with_theme(true)
            .initial_theme(initial_theme)
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

fn suppress_light_client_chatter(mut filter: EnvFilter) -> EnvFilter {
    for directive in [
        "json-rpc=warn",
        "network=info",
        "runtime=info",
        "sync-service=info",
        "bitswap-service=info",
        "tx-service=info",
        "stkopt_chain::lightclient=info",
        "stkopt_chain::queries::identity=info",
    ] {
        filter = filter.add_directive(directive.parse().expect("log directive is valid"));
    }
    filter
}
