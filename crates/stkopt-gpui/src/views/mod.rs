//! View modules for each section of the app.

mod account;
mod dashboard;
mod help;
mod history;
mod logs;
mod optimization;
mod pool_modal;
mod pools;
mod qr_modal;
mod settings;
mod staking_modal;
mod validators;

pub use account::AccountSection;
pub use dashboard::DashboardSection;
pub use help::HelpOverlay;
pub use history::HistorySection;
pub use logs::LogsView;
pub use optimization::OptimizationSection;
pub use pool_modal::PoolModal;
pub use pools::PoolsSection;
pub use qr_modal::QrModal;
pub use settings::SettingsSection;
pub use staking_modal::StakingModal;
pub use validators::ValidatorsSection;
