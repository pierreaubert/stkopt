//! Shared utility functions for the GPUI app.

/// Format a token balance with the given decimals and symbol.
pub fn format_balance(amount: u128, symbol: &str, decimals: u8) -> String {
    let divisor = 10u128.pow(decimals as u32);
    let whole = amount / divisor;
    if whole >= 1_000_000 {
        let m = whole as f64 / 1_000_000.0;
        format!("{:.1}m {}", m, symbol)
    } else if whole >= 10_000 {
        let k = whole as f64 / 1_000.0;
        format!("{:.0}k {}", k, symbol)
    } else {
        let frac_divisor = 10u128.pow(decimals.saturating_sub(4) as u32);
        let frac = (amount % divisor) / frac_divisor;
        format!("{}.{:04} {}", whole, frac, symbol)
    }
}
