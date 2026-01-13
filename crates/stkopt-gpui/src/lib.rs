//! Staking Optimizer Desktop - GPUI library for testing.
//!
//! This module exposes the app components for testing purposes.

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
pub mod transactions;
pub mod validators;
pub mod views;

#[cfg(test)]
mod tests {
    use super::app::*;

    #[test]
    fn test_section_all_returns_all_sections() {
        let sections = Section::all();
        assert_eq!(sections.len(), 6);
        assert!(sections.contains(&Section::Dashboard));
        assert!(sections.contains(&Section::Account));
        assert!(sections.contains(&Section::Validators));
        assert!(sections.contains(&Section::Optimization));
        assert!(sections.contains(&Section::Pools));
        assert!(sections.contains(&Section::History));
    }

    #[test]
    fn test_section_label() {
        assert_eq!(Section::Dashboard.label(), "Dashboard");
        assert_eq!(Section::Account.label(), "Account");
        assert_eq!(Section::Validators.label(), "Validators");
        assert_eq!(Section::Optimization.label(), "Optimization");
        assert_eq!(Section::Pools.label(), "Pools");
        assert_eq!(Section::History.label(), "History");
    }

    #[test]
    fn test_section_icon() {
        assert_eq!(Section::Dashboard.icon(), "📊");
        assert_eq!(Section::Account.icon(), "👤");
        assert_eq!(Section::Validators.icon(), "✓");
        assert_eq!(Section::Optimization.icon(), "⚡");
        assert_eq!(Section::Pools.icon(), "🏊");
        assert_eq!(Section::History.icon(), "📈");
    }

    #[test]
    fn test_section_default() {
        assert_eq!(Section::default(), Section::Dashboard);
    }

    #[test]
    fn test_network_label() {
        assert_eq!(Network::Polkadot.label(), "Polkadot");
        assert_eq!(Network::Kusama.label(), "Kusama");
        assert_eq!(Network::Westend.label(), "Westend");
    }

    #[test]
    fn test_network_symbol() {
        assert_eq!(Network::Polkadot.symbol(), "DOT");
        assert_eq!(Network::Kusama.symbol(), "KSM");
        assert_eq!(Network::Westend.symbol(), "WND");
    }

    #[test]
    fn test_network_default() {
        assert_eq!(Network::default(), Network::Polkadot);
    }

    #[test]
    fn test_connection_status_default() {
        assert_eq!(ConnectionStatus::default(), ConnectionStatus::Disconnected);
    }

    #[test]
    fn test_connection_mode_default() {
        assert_eq!(ConnectionMode::default(), ConnectionMode::Rpc);
    }

    #[test]
    fn test_connection_mode_label() {
        assert_eq!(ConnectionMode::Rpc.label(), "RPC");
        assert_eq!(ConnectionMode::LightClient.label(), "Light Client");
    }

    #[test]
    fn test_connection_mode_description() {
        assert!(ConnectionMode::Rpc.description().contains("RPC"));
        assert!(
            ConnectionMode::LightClient
                .description()
                .contains("light client")
        );
    }

    #[test]
    fn test_staking_info_default() {
        let info = StakingInfo::default();
        assert_eq!(info.total_balance, 0);
        assert_eq!(info.bonded, 0);
        assert!(!info.is_nominating);
    }
}
