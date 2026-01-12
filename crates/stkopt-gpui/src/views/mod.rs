//! View modules for each section of the app.

mod account;
mod dashboard;
mod help;
mod history;
mod logs;
mod optimization;
mod pools;
mod settings;
mod validators;

pub use account::AccountSection;
pub use dashboard::DashboardSection;
pub use help::HelpOverlay;
pub use history::HistorySection;
pub use logs::LogsView;
pub use optimization::OptimizationSection;
pub use pools::PoolsSection;
pub use settings::SettingsSection;
pub use validators::ValidatorsSection;
