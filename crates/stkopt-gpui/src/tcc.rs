//! TCC (Transparency, Consent, and Control) permission handling for macOS.
//!
//! Handles camera permission checks and requests for desktop apps.

use rusqlite::Connection;
use std::env;
use std::path::PathBuf;

#[cfg(target_os = "macos")]
const TCC_DB_PATH: &str = "Library/Application Support/com.apple.TCC/TCC.db";

const CAMERA_SERVICE: &str = "kTCCServiceCamera";

/// Get the bundle ID for the current app.
/// For GPUI apps, we use a custom bundle ID.
pub fn get_app_bundle_id() -> (&'static str, &'static str) {
    // Check if running from IDE or terminal
    let term_program = env::var("TERM_PROGRAM").unwrap_or_else(|_| String::new());

    if !term_program.is_empty() {
        // Running from terminal - use terminal's bundle ID
        let (bundle_id, display_name) = match term_program.as_str() {
            "Apple_Terminal" => ("com.apple.Terminal", "Terminal"),
            "iTerm.app" => ("com.googlecode.iterm2", "iTerm2"),
            "vscode" => ("com.microsoft.VSCode", "VS Code"),
            "WarpTerminal" => ("dev.warp.Warp-Stable", "Warp"),
            "Hyper" => ("com.zeit.hyper", "Hyper"),
            "Alacritty" => ("io.alacritty", "Alacritty"),
            "WezTerm" => ("com.github.wez.wezterm", "WezTerm"),
            "Tabby" => ("com.tabby", "Tabby"),
            _ => ("com.apple.Terminal", "Terminal"),
        };
        (bundle_id, display_name)
    } else {
        // Running as standalone app
        ("com.stkopt.desktop", "Staking Optimizer")
    }
}

#[cfg(target_os = "macos")]
fn get_tcc_db_path() -> Option<PathBuf> {
    let home = env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(TCC_DB_PATH))
}

#[cfg(not(target_os = "macos"))]
fn get_tcc_db_path() -> Option<PathBuf> {
    None
}

#[cfg(target_os = "macos")]
pub fn check_camera_permission() -> Result<Option<bool>, String> {
    let db_path = get_tcc_db_path().ok_or_else(|| "Could not find HOME directory".to_string())?;

    if !db_path.exists() {
        return Err("TCC.db not found".to_string());
    }

    let conn = Connection::open(db_path).map_err(|e| format!("Failed to open TCC.db: {}", e))?;

    let (bundle_id, display_name) = get_app_bundle_id();

    let mut stmt = conn
        .prepare("SELECT allowed FROM access WHERE client = ?1 AND service = ?2")
        .map_err(|e| format!("Failed to prepare query: {}", e))?;

    let mut rows = stmt
        .query([bundle_id, CAMERA_SERVICE])
        .map_err(|e| format!("Failed to query: {}", e))?;

    if let Some(row) = rows
        .next()
        .map_err(|e| format!("Failed to read row: {}", e))?
    {
        let allowed: i32 = row
            .get(0)
            .map_err(|e| format!("Failed to get allowed value: {}", e))?;
        tracing::info!(
            "Camera permission for {} ({}): {}",
            display_name,
            bundle_id,
            if allowed == 1 { "ALLOWED" } else { "DENIED" }
        );
        Ok(Some(allowed == 1))
    } else {
        tracing::info!(
            "No camera permission record found for {} ({})",
            display_name,
            bundle_id
        );
        Ok(None)
    }
}

#[cfg(not(target_os = "macos"))]
pub fn check_camera_permission() -> Result<Option<bool>, String> {
    Ok(None)
}

#[cfg(target_os = "macos")]
pub fn request_camera_permission() -> Result<bool, String> {
    use std::process::Command;

    tracing::info!("Requesting camera permission via Swift...");

    let output = Command::new("swift")
        .args([
            "-e",
            "import AVFoundation; AVCaptureDevice.requestAccess(for: .video) { granted in exit(granted ? 0 : 1) }",
        ])
        .output()
        .map_err(|e| format!("Failed to execute Swift command: {}", e))?;

    if output.status.success() {
        tracing::info!("Camera permission request succeeded");
        Ok(true)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!("Camera permission request failed: {}", stderr);
        Err(format!("Swift command failed: {}", stderr))
    }
}

#[cfg(not(target_os = "macos"))]
pub fn request_camera_permission() -> Result<bool, String> {
    Err("Camera permission request only available on macOS".to_string())
}

#[cfg(target_os = "macos")]
pub fn ensure_camera_permission() -> Result<bool, String> {
    // First check if permission is already granted
    match check_camera_permission() {
        Ok(Some(true)) => {
            tracing::info!("Camera permission already granted");
            return Ok(true);
        }
        Ok(Some(false)) => {
            tracing::warn!("Camera permission denied in TCC database");
        }
        Ok(None) => {
            tracing::info!("No camera permission record found");
        }
        Err(e) => {
            tracing::warn!("Could not check TCC database: {}", e);
        }
    }

    // Try to request permission
    match request_camera_permission() {
        Ok(true) => {
            tracing::info!("Camera permission granted after request");
            Ok(true)
        }
        Ok(false) => {
            tracing::error!("Camera permission was not granted");
            Err("Camera permission denied by user".to_string())
        }
        Err(e) => {
            tracing::error!("Failed to request camera permission: {}", e);
            Err(e)
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn ensure_camera_permission() -> Result<bool, String> {
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_bundle_id() {
        let (bundle_id, _) = get_app_bundle_id();
        assert!(bundle_id.contains("."));
    }
}
