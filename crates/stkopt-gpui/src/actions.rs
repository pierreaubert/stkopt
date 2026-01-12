//! Actions for state updates in the GPUI app.
//!
//! Actions are events that can update application state. They are sent from
//! background tasks (chain operations) to the UI thread.

use crate::app::{ConnectionMode, ConnectionStatus, Network};

/// Chain info received after connection.
#[derive(Debug, Clone)]
pub struct ChainInfoUpdate {
    /// Chain name as reported by system_chain RPC.
    pub chain_name: String,
    /// Runtime spec version.
    pub spec_version: u32,
    /// Whether the chain matches expected network.
    pub validated: bool,
}

/// Actions that can update application state.
#[derive(Debug, Clone)]
pub enum Action {
    /// Update connection status.
    UpdateConnectionStatus(ConnectionStatus),

    /// Set chain info after successful connection.
    SetChainInfo(ChainInfoUpdate),

    /// Connection failed with error message.
    ConnectionFailed(String),

    /// Request to connect to a network.
    Connect {
        network: Network,
        mode: ConnectionMode,
    },

    /// Request to disconnect from the network.
    Disconnect,

    /// Set the watched account address.
    SetWatchedAccount(String),

    /// Clear the watched account.
    ClearWatchedAccount,

    /// Account validation failed.
    AccountValidationError(String),

    /// Clear account validation error.
    ClearAccountValidationError,

    /// Switch to a specific section/tab.
    SwitchSection(crate::app::Section),

    /// Set validators list.
    SetValidators(Vec<crate::app::ValidatorInfo>),

    /// Toggle validator selection.
    ToggleValidatorSelection(usize),

    /// Clear all validator selections.
    ClearValidatorSelections,

    /// Set validator sort column.
    SetValidatorSort(ValidatorSortColumn),

    /// Set validator search query.
    SetValidatorSearch(String),

    /// Set pools list.
    SetPools(Vec<crate::app::PoolInfo>),
}

/// Column to sort validators by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ValidatorSortColumn {
    #[default]
    Name,
    Commission,
    TotalStake,
    OwnStake,
    NominatorCount,
    Apy,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_debug() {
        let action = Action::UpdateConnectionStatus(ConnectionStatus::Connecting);
        let debug_str = format!("{:?}", action);
        assert!(debug_str.contains("UpdateConnectionStatus"));
        assert!(debug_str.contains("Connecting"));
    }

    #[test]
    fn test_action_clone() {
        let action = Action::ConnectionFailed("test error".to_string());
        let cloned = action.clone();
        match cloned {
            Action::ConnectionFailed(msg) => assert_eq!(msg, "test error"),
            _ => panic!("Expected ConnectionFailed"),
        }
    }

    #[test]
    fn test_connect_action() {
        let action = Action::Connect {
            network: Network::Polkadot,
            mode: ConnectionMode::default(),
        };
        match action {
            Action::Connect { network, .. } => assert_eq!(network, Network::Polkadot),
            _ => panic!("Expected Connect"),
        }
    }

    #[test]
    fn test_chain_info_update() {
        let info = ChainInfoUpdate {
            chain_name: "Polkadot".to_string(),
            spec_version: 1000000,
            validated: true,
        };
        assert_eq!(info.chain_name, "Polkadot");
        assert!(info.validated);
    }

    #[test]
    fn test_disconnect_action() {
        let action = Action::Disconnect;
        assert!(matches!(action, Action::Disconnect));
    }

    #[test]
    fn test_set_watched_account() {
        let action = Action::SetWatchedAccount("15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5".to_string());
        match action {
            Action::SetWatchedAccount(addr) => assert!(addr.starts_with("15oF4u")),
            _ => panic!("Expected SetWatchedAccount"),
        }
    }

    #[test]
    fn test_clear_watched_account() {
        let action = Action::ClearWatchedAccount;
        assert!(matches!(action, Action::ClearWatchedAccount));
    }

    #[test]
    fn test_account_validation_error() {
        let action = Action::AccountValidationError("Invalid address".to_string());
        match action {
            Action::AccountValidationError(msg) => assert_eq!(msg, "Invalid address"),
            _ => panic!("Expected AccountValidationError"),
        }
    }

    #[test]
    fn test_switch_section() {
        use crate::app::Section;
        let action = Action::SwitchSection(Section::Dashboard);
        match action {
            Action::SwitchSection(section) => assert_eq!(section, Section::Dashboard),
            _ => panic!("Expected SwitchSection"),
        }
    }
}
