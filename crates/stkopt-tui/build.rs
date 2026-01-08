//! Build script to embed Info.plist into the macOS binary.
//!
//! This is required for macOS entitlements (like camera access) to work
//! with signed binaries.

fn main() {
    // Only run on macOS
    #[cfg(target_os = "macos")]
    {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let info_plist_path = format!("{}/../../scripts/Info.plist", manifest_dir);

        // Check if Info.plist exists
        if std::path::Path::new(&info_plist_path).exists() {
            // Embed Info.plist into the binary's __TEXT,__info_plist section
            println!(
                "cargo:rustc-link-arg=-Wl,-sectcreate,__TEXT,__info_plist,{}",
                info_plist_path
            );
            println!("cargo:rerun-if-changed={}", info_plist_path);
        } else {
            println!(
                "cargo:warning=Info.plist not found at {}, camera access may not work",
                info_plist_path
            );
        }
    }
}
