#!/bin/bash
#
# Build script for stkopt macOS application
# Creates a signed and notarized DMG for distribution
#
# Usage:
#   ./build-dmg.sh                    # Build unsigned DMG (for local testing)
#   ./build-dmg.sh --sign             # Build signed DMG (requires Developer ID)
#   ./build-dmg.sh --sign --notarize  # Build, sign, and notarize (for distribution)
#
# Environment variables:
#   DEVELOPER_ID         - Developer ID Application certificate name
#                          Example: "Developer ID Application: Your Name (TEAMID)"
#   APPLE_ID             - Apple ID email for notarization
#   APPLE_APP_PASSWORD   - App-specific password for notarization
#   APPLE_TEAM_ID        - Apple Developer Team ID
#
# Prerequisites:
#   - Xcode Command Line Tools
#   - Rust toolchain
#   - create-dmg (optional, for prettier DMG): brew install create-dmg
#

set -euo pipefail

# Configuration
APP_NAME="stkopt"
BUNDLE_ID="xyz.dotidx.stkopt"
BINARY_NAME="stkopt"
BUILD_NUMBER="1"

# Paths
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Extract version from root Cargo.toml
VERSION=$(grep -m1 '^version = ' "$PROJECT_ROOT/Cargo.toml" | sed 's/version = "\(.*\)"/\1/')
if [ -z "$VERSION" ]; then
    echo "ERROR: Could not extract version from Cargo.toml"
    exit 1
fi
BUILD_DIR="$PROJECT_ROOT/target/release"
DMG_DIR="$PROJECT_ROOT/target/dmg"
APP_BUNDLE="$DMG_DIR/$APP_NAME.app"

# Command line options (defaults: unsigned build for local testing)
SIGN=false
NOTARIZE=false
UNIVERSAL=false
CLEAN=false

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --sign)
            SIGN=true
            shift
            ;;
        --notarize)
            NOTARIZE=true
            SIGN=true  # Notarization requires signing
            shift
            ;;
        --universal)
            UNIVERSAL=true
            shift
            ;;
        --clean)
            CLEAN=true
            shift
            ;;
        --help|-h)
            echo "Usage: $0 [options]"
            echo ""
            echo "Options:"
            echo "  --sign        Sign the application with Developer ID"
            echo "  --notarize    Notarize the application (implies --sign)"
            echo "  --universal   Build universal binary (Intel + Apple Silicon)"
            echo "  --clean       Clean build directory before building"
            echo "  --help        Show this help message"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Color output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check prerequisites
check_prerequisites() {
    log_info "Checking prerequisites..."

    if ! command -v cargo &> /dev/null; then
        log_error "Rust/Cargo is not installed"
        exit 1
    fi

    if ! command -v codesign &> /dev/null; then
        log_error "Xcode Command Line Tools not installed"
        exit 1
    fi

    # DEVELOPER_ID is optional - if not set, we use ad-hoc signing
    if $SIGN && [ -z "${DEVELOPER_ID:-}" ]; then
        log_warning "DEVELOPER_ID not set, will use ad-hoc signing (local use only)"
        log_info "For distribution, set DEVELOPER_ID='Developer ID Application: Your Name (TEAMID)'"
    fi

    if $NOTARIZE; then
        if [ -z "${DEVELOPER_ID:-}" ]; then
            log_error "DEVELOPER_ID required for notarization"
            exit 1
        fi
        if [ -z "${APPLE_ID:-}" ]; then
            log_error "APPLE_ID environment variable not set"
            exit 1
        fi
    fi

    log_success "Prerequisites check passed"
}

# Clean build artifacts
clean_build() {
    if $CLEAN; then
        log_info "Cleaning build directory..."
        rm -rf "$DMG_DIR"
        cargo clean -p stkopt-tui
    fi
}

# Build the binary
build_binary() {
    log_info "Building release binary..."

    cd "$PROJECT_ROOT"

    if $UNIVERSAL; then
        log_info "Building universal binary (x86_64 + arm64)..."

        # Ensure targets are installed
        rustup target add x86_64-apple-darwin aarch64-apple-darwin

        # Build for both architectures
        cargo build --release --package stkopt-tui --target x86_64-apple-darwin
        cargo build --release --package stkopt-tui --target aarch64-apple-darwin

        # Create universal binary
        mkdir -p "$BUILD_DIR"
        lipo -create \
            "$PROJECT_ROOT/target/x86_64-apple-darwin/release/$BINARY_NAME" \
            "$PROJECT_ROOT/target/aarch64-apple-darwin/release/$BINARY_NAME" \
            -output "$BUILD_DIR/$BINARY_NAME"

        log_success "Universal binary created"
    else
        # Standard build - Info.plist is embedded via build.rs
        cargo build --release --package stkopt-tui
    fi

    if [ ! -f "$BUILD_DIR/$BINARY_NAME" ]; then
        log_error "Binary not found at $BUILD_DIR/$BINARY_NAME"
        exit 1
    fi

    # Verify Info.plist is embedded
    if otool -s __TEXT __info_plist "$BUILD_DIR/$BINARY_NAME" | grep -q "Contents"; then
        log_success "Binary built with embedded Info.plist"
    else
        log_warning "Info.plist may not be embedded - camera access might not work"
    fi
}

# Create app bundle structure
create_app_bundle() {
    log_info "Creating app bundle..."

    # Clean and create directories
    rm -rf "$APP_BUNDLE"
    mkdir -p "$APP_BUNDLE/Contents/MacOS"
    mkdir -p "$APP_BUNDLE/Contents/Resources"

    # Copy binary to Resources (the launcher will run it)
    cp "$BUILD_DIR/$BINARY_NAME" "$APP_BUNDLE/Contents/Resources/$BINARY_NAME-bin"

    # Create launcher script that opens Terminal with the TUI app
    cat > "$APP_BUNDLE/Contents/MacOS/$BINARY_NAME" << 'LAUNCHER_EOF'
#!/bin/bash
# Launcher script for stkopt TUI application
# Opens Terminal.app and runs the actual binary

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BINARY="$SCRIPT_DIR/../Resources/stkopt-bin"

# Use osascript to open Terminal and run the app
osascript << EOF
tell application "Terminal"
    activate
    do script "clear && '$BINARY'; exit"
end tell
EOF
LAUNCHER_EOF
    chmod +x "$APP_BUNDLE/Contents/MacOS/$BINARY_NAME"

    # Copy Info.plist and update version
    sed -e "s/STKOPT_VERSION/$VERSION/" \
        -e "s/STKOPT_BUILD/$BUILD_NUMBER/" \
        "$SCRIPT_DIR/Info.plist" > "$APP_BUNDLE/Contents/Info.plist"

    # Create PkgInfo
    echo -n "APPL????" > "$APP_BUNDLE/Contents/PkgInfo"

    # Copy icon if it exists (convert from jpg/png to icns if needed)
    if [ -f "$SCRIPT_DIR/icon.png" ]; then
        create_icns "$SCRIPT_DIR/icon.png" "$APP_BUNDLE/Contents/Resources/AppIcon.icns"
    elif [ -f "$SCRIPT_DIR/icon.icns" ]; then
        cp "$SCRIPT_DIR/icon.icns" "$APP_BUNDLE/Contents/Resources/AppIcon.icns"
    else
        log_warning "No icon found, app will use default icon"
    fi

    log_success "App bundle created at $APP_BUNDLE"
}

# Bundle dynamic libraries from Homebrew and other non-system locations
bundle_dylibs() {
    log_info "Bundling dynamic libraries..."

    local frameworks_dir="$APP_BUNDLE/Contents/Frameworks"
    mkdir -p "$frameworks_dir"

    # The actual binary is in Resources (MacOS contains the launcher script)
    local binary="$APP_BUNDLE/Contents/Resources/$BINARY_NAME-bin"

    # Get list of non-system dylibs
    local dylibs
    dylibs=$(otool -L "$binary" | grep -v "^$binary" | awk '{print $1}' | grep -v "^/System" | grep -v "^/usr/lib" | grep -v "@rpath" | grep -v "@executable_path" || true)

    if [ -z "$dylibs" ]; then
        log_info "No external dylibs to bundle"
        return
    fi

    # Process each dylib
    for dylib in $dylibs; do
        if [ ! -f "$dylib" ]; then
            log_warning "Dylib not found: $dylib"
            continue
        fi

        local dylib_name
        dylib_name=$(basename "$dylib")
        local dest="$frameworks_dir/$dylib_name"

        log_info "Bundling: $dylib_name"

        # Copy the dylib
        cp "$dylib" "$dest"
        chmod 755 "$dest"

        # Remove existing signature to avoid warnings from install_name_tool
        codesign --remove-signature "$dest" 2>/dev/null || true

        # Fix the dylib's own install name
        install_name_tool -id "@executable_path/../Frameworks/$dylib_name" "$dest"

        # Update the reference in the main binary
        install_name_tool -change "$dylib" "@executable_path/../Frameworks/$dylib_name" "$binary"

        # Recursively process dependencies FIRST (before fixing references)
        bundle_dylib_deps "$dest" "$frameworks_dir" "$binary"

        # Fix all internal references within this dylib AFTER bundling deps
        fix_dylib_references "$dest"
    done

    log_success "Dynamic libraries bundled"
}

# Fix all non-system library references within a dylib to use @executable_path
fix_dylib_references() {
    local dylib="$1"

    # Get all non-system dependencies (including @rpath references)
    local deps
    deps=$(otool -L "$dylib" | tail -n +2 | awk '{print $1}' | grep -v "^/System" | grep -v "^/usr/lib" | grep -v "@executable_path" || true)

    for dep in $deps; do
        local dep_name
        dep_name=$(basename "$dep")
        install_name_tool -change "$dep" "@executable_path/../Frameworks/$dep_name" "$dylib"
    done
}

# Recursively bundle dependencies of a dylib
bundle_dylib_deps() {
    local dylib="$1"
    local frameworks_dir="$2"
    local main_binary="$3"

    local deps
    deps=$(otool -L "$dylib" | grep -v "^$dylib" | awk '{print $1}' | grep -v "^/System" | grep -v "^/usr/lib" | grep -v "@rpath" | grep -v "@executable_path" || true)

    for dep in $deps; do
        if [ ! -f "$dep" ]; then
            continue
        fi

        local dep_name
        dep_name=$(basename "$dep")
        local dest="$frameworks_dir/$dep_name"

        # Skip if already bundled
        if [ -f "$dest" ]; then
            # Just update the reference
            install_name_tool -change "$dep" "@executable_path/../Frameworks/$dep_name" "$dylib"
            continue
        fi

        log_info "  Bundling dependency: $dep_name"
        cp "$dep" "$dest"
        chmod 755 "$dest"

        # Remove existing signature to avoid warnings from install_name_tool
        codesign --remove-signature "$dest" 2>/dev/null || true

        # Fix the dylib's own install name
        install_name_tool -id "@executable_path/../Frameworks/$dep_name" "$dest"

        # Update reference in the dylib being processed
        install_name_tool -change "$dep" "@executable_path/../Frameworks/$dep_name" "$dylib"

        # Recurse FIRST (before fixing references)
        bundle_dylib_deps "$dest" "$frameworks_dir" "$main_binary"

        # Fix all internal references within this dylib AFTER bundling deps
        fix_dylib_references "$dest"
    done
}

# Create icns from image file
create_icns() {
    local input_image="$1"
    local output_icns="$2"

    log_info "Creating app icon..."

    local iconset_dir="$DMG_DIR/AppIcon.iconset"
    mkdir -p "$iconset_dir"

    # Generate all required sizes
    local sizes=(16 32 64 128 256 512 1024)
    for size in "${sizes[@]}"; do
        sips -z $size $size "$input_image" --out "$iconset_dir/icon_${size}x${size}.png" 2>/dev/null || true
    done

    # Create @2x versions
    sips -s format png -z 32 32 "$input_image" --out "$iconset_dir/icon_16x16@2x.png" 2>/dev/null || true
    sips -s format png -z 64 64 "$input_image" --out "$iconset_dir/icon_32x32@2x.png" 2>/dev/null || true
    sips -s format png -z 128 128 "$input_image" --out "$iconset_dir/icon_64x64@2x.png" 2>/dev/null || true
    sips -s format png -z 256 256 "$input_image" --out "$iconset_dir/icon_128x128@2x.png" 2>/dev/null || true
    sips -s format png -z 512 512 "$input_image" --out "$iconset_dir/icon_256x256@2x.png" 2>/dev/null || true
    sips -s format png -z 1024 1024 "$input_image" --out "$iconset_dir/icon_512x512@2x.png" 2>/dev/null || true

    # Convert to icns
    iconutil -c icns "$iconset_dir" -o "$output_icns" 2>/dev/null || {
        log_warning "Failed to create icns, app will use default icon"
        return
    }

    rm -rf "$iconset_dir"
    log_success "App icon created"
}

# Sign the standalone binary (for direct use without .app bundle)
sign_binary() {
    log_info "Signing standalone binary..."

    local binary="$BUILD_DIR/$BINARY_NAME"

    if [ -n "${DEVELOPER_ID:-}" ]; then
        codesign --force --options runtime \
            --entitlements "$PROJECT_ROOT/scripts/entitlements.plist" \
            --sign "$DEVELOPER_ID" \
            --timestamp \
            "$binary"
    else
        # Ad-hoc signing for local use
        codesign --force --deep \
            --entitlements "$PROJECT_ROOT/scripts/entitlements.plist" \
            -fs - \
            "$binary"
    fi

    log_success "Standalone binary signed: $binary"
}

# Sign the application
sign_app() {
    log_info "Signing application..."

    # Determine signing identity
    local sign_identity
    if [ -n "${DEVELOPER_ID:-}" ]; then
        sign_identity="$DEVELOPER_ID"
        log_info "Using Developer ID: $sign_identity"
    else
        sign_identity="-"  # Ad-hoc signing
        log_info "Using ad-hoc signing (local use only)"
    fi

    # Sign all frameworks first (must sign inside-out)
    if [ -d "$APP_BUNDLE/Contents/Frameworks" ]; then
        for lib in "$APP_BUNDLE/Contents/Frameworks"/*.dylib; do
            if [ -f "$lib" ]; then
                log_info "Signing framework: $(basename "$lib")"
                if [ "$sign_identity" = "-" ]; then
                    codesign --force --deep -fs "$sign_identity" "$lib"
                else
                    codesign --force --options runtime \
                        --sign "$sign_identity" \
                        --timestamp \
                        "$lib"
                fi
            fi
        done
    fi

    # Sign the actual binary (in Resources) with entitlements
    local actual_binary="$APP_BUNDLE/Contents/Resources/$BINARY_NAME-bin"

    if [ "$sign_identity" = "-" ]; then
        # Ad-hoc signing
        codesign --force --deep \
            --entitlements "$PROJECT_ROOT/scripts/entitlements.plist" \
            -fs "$sign_identity" \
            "$actual_binary"

        # Sign the launcher script
        codesign --force --deep \
            -fs "$sign_identity" \
            "$APP_BUNDLE/Contents/MacOS/$BINARY_NAME"

        # Sign the entire bundle
        codesign --force --deep \
            --entitlements "$PROJECT_ROOT/scripts/entitlements.plist" \
            -fs "$sign_identity" \
            "$APP_BUNDLE"
    else
        # Developer ID signing with hardened runtime
        codesign --force --options runtime \
            --entitlements "$PROJECT_ROOT/scripts/entitlements.plist" \
            --sign "$sign_identity" \
            --timestamp \
            "$actual_binary"

        # Sign the launcher script
        codesign --force --options runtime \
            --sign "$sign_identity" \
            --timestamp \
            "$APP_BUNDLE/Contents/MacOS/$BINARY_NAME"

        # Sign the entire bundle
        codesign --force --options runtime \
            --entitlements "$PROJECT_ROOT/scripts/entitlements.plist" \
            --sign "$sign_identity" \
            --timestamp \
            "$APP_BUNDLE"
    fi

    # Verify signature
    codesign --verify --deep --strict --verbose=2 "$APP_BUNDLE"

    log_success "Application signed successfully"
}

# Create DMG
create_dmg() {
    log_info "Creating DMG..."

    local dmg_path="$DMG_DIR/$APP_NAME-$VERSION.dmg"
    local dmg_temp="$DMG_DIR/temp.dmg"

    rm -f "$dmg_path" "$dmg_temp"

    # Check if create-dmg is available (prettier DMG)
    if command -v create-dmg &> /dev/null; then
        log_info "Using create-dmg for styled DMG..."

        # create-dmg can fail with AppleScript timeout when Finder is unresponsive
        # or when running headless. We handle this gracefully with hdiutil fallback.
        if create-dmg \
            --volname "$APP_NAME" \
            --volicon "$APP_BUNDLE/Contents/Resources/AppIcon.icns" \
            --window-pos 200 120 \
            --window-size 600 400 \
            --icon-size 100 \
            --icon "$APP_NAME.app" 150 190 \
            --hide-extension "$APP_NAME.app" \
            --app-drop-link 450 185 \
            --no-internet-enable \
            "$dmg_path" \
            "$APP_BUNDLE" 2>&1; then
            log_success "DMG created (with create-dmg)"
        else
            # Clean up any temp DMG files left by create-dmg
            rm -f "$DMG_DIR"/rw.*.dmg 2>/dev/null || true

            if [ -f "$dmg_path" ]; then
                # create-dmg sometimes returns non-zero even on success
                log_success "DMG created (with create-dmg)"
            else
                log_warning "create-dmg failed, falling back to hdiutil"
                create_dmg_hdiutil "$dmg_path"
            fi
        fi
    else
        create_dmg_hdiutil "$dmg_path"
    fi

    if [ -f "$dmg_path" ] && [ -n "${DEVELOPER_ID:-}" ]; then
        log_info "Signing DMG..."
        codesign --force --sign "$DEVELOPER_ID" --timestamp "$dmg_path"
        log_success "DMG signed"
    fi

    log_success "DMG created at $dmg_path"
    echo "$dmg_path"
}

# Create DMG using hdiutil (fallback)
create_dmg_hdiutil() {
    local dmg_path="$1"
    local staging_dir="$DMG_DIR/staging"

    rm -rf "$staging_dir"
    mkdir -p "$staging_dir"

    # Copy app to staging
    cp -R "$APP_BUNDLE" "$staging_dir/"

    # Create symlink to /Applications
    ln -s /Applications "$staging_dir/Applications"

    # Create DMG
    hdiutil create -volname "$APP_NAME" \
        -srcfolder "$staging_dir" \
        -ov -format UDZO \
        "$dmg_path"

    rm -rf "$staging_dir"
    log_success "DMG created (with hdiutil)"
}

# Notarize the DMG
notarize_dmg() {
    if ! $NOTARIZE; then
        log_warning "Skipping notarization (use --notarize to enable)"
        return
    fi

    local dmg_path="$DMG_DIR/$APP_NAME-$VERSION.dmg"

    if [ ! -f "$dmg_path" ]; then
        log_error "DMG not found at $dmg_path"
        exit 1
    fi

    log_info "Submitting for notarization..."

    # Submit for notarization
    local submission_output
    submission_output=$(xcrun notarytool submit "$dmg_path" \
        --apple-id "$APPLE_ID" \
        --keychain-profile "stkopt-notarization" \
        --wait 2>&1)

    echo "$submission_output"

    if echo "$submission_output" | grep -q "status: Accepted"; then
        log_success "Notarization accepted"

        # Staple the notarization ticket
        log_info "Stapling notarization ticket..."
        xcrun stapler staple "$dmg_path"
        log_success "Notarization ticket stapled"

        # Verify
        xcrun stapler validate "$dmg_path"
        log_success "Notarization verified"
    else
        log_error "Notarization failed"
        log_info "Check the submission output above for details"

        # Extract submission ID for log retrieval
        local submission_id
        submission_id=$(echo "$submission_output" | grep -o 'id: [a-f0-9-]*' | head -1 | cut -d' ' -f2)
        if [ -n "$submission_id" ]; then
            log_info "To get detailed logs, run:"
            log_info "  xcrun notarytool log $submission_id --apple-id $APPLE_ID --password <password> --team-id $APPLE_TEAM_ID"
        fi
        exit 1
    fi
}

# Main build process
main() {
    log_info "=========================================="
    log_info "Building $APP_NAME v$VERSION"
    log_info "=========================================="

    check_prerequisites
    clean_build
    build_binary
    sign_binary          # Sign standalone binary for direct use
    create_app_bundle
    bundle_dylibs
    sign_app             # Sign .app bundle
    create_dmg
    notarize_dmg

    log_info "=========================================="
    log_success "Build complete!"
    log_info "=========================================="

    log_info ""
    log_info "Standalone binary: $BUILD_DIR/$BINARY_NAME"

    local dmg_path="$DMG_DIR/$APP_NAME-$VERSION.dmg"
    if [ -f "$dmg_path" ]; then
        log_info "DMG: $dmg_path"
        log_info "Size: $(du -h "$dmg_path" | cut -f1)"

        if [ -n "${DEVELOPER_ID:-}" ]; then
            log_info "Signed: Yes (Developer ID)"
        else
            log_info "Signed: Ad-hoc (local use only)"
        fi

        if $NOTARIZE; then
            log_info "Notarized: Yes"
        else
            log_warning "Notarized: No (use --notarize for Gatekeeper)"
        fi
    fi
}

main "$@"
