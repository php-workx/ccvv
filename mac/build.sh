#!/bin/bash
set -e

APP_NAME="ccvv"
BUILD_DIR="$(cd "$(dirname "$0")/.." && pwd)/build"
TEAM_ID="4JRN737CHR"
# shellcheck disable=SC2034  # kept for reference; used by codesign/notarize workflows
BUNDLE_ID="com.ccvv.app"
NOTARIZE=0
SKIP_RUST=0
DEPLOYMENT_TARGET="${CCVV_MACOS_DEPLOYMENT_TARGET:-${MACOSX_DEPLOYMENT_TARGET:-13.0}}"
SWIFT_ARCH="$(uname -m)"
SWIFT_TARGET="${SWIFT_ARCH}-apple-macos${DEPLOYMENT_TARGET}"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CORE_DIR="$SCRIPT_DIR/../core"

for arg in "$@"; do
    case "$arg" in
        --notarize) NOTARIZE=1 ;;
        --skip-rust) SKIP_RUST=1 ;;
    esac
done

mkdir -p "$BUILD_DIR"
export MACOSX_DEPLOYMENT_TARGET="$DEPLOYMENT_TARGET"
echo "Using macOS deployment target: $MACOSX_DEPLOYMENT_TARGET ($SWIFT_TARGET)"

# --- Step 1: Build Rust static library ---
if [[ "$SKIP_RUST" -eq 0 ]]; then
    echo "Building Rust core library..."
    if ! command -v cargo &>/dev/null; then
        echo "Error: cargo not found. Install Rust from https://rustup.rs"
        exit 1
    fi
    (cd "$CORE_DIR" && cargo build --package ccvv-lib --package ccvv-cli --release)

    # Find the generated header
    RUST_OUT_DIR=$(cd "$CORE_DIR" && cargo metadata --format-version 1 2>/dev/null \
        | python3 -c "import sys,json; print(json.load(sys.stdin)['target_directory'])" 2>/dev/null \
        || echo "$CORE_DIR/target")
    HEADER_DIR=$(find "$RUST_OUT_DIR/release/build" -name "ccvv-bridge.h" -print -quit 2>/dev/null)
    if [[ -n "$HEADER_DIR" ]]; then
        HEADER_DIR=$(dirname "$HEADER_DIR")
    fi
    LIB_PATH="$RUST_OUT_DIR/release/libccvv_lib.a"

    if [[ ! -f "$LIB_PATH" ]]; then
        echo "Error: static library not found at $LIB_PATH"
        exit 1
    fi
    if [[ -z "$HEADER_DIR" ]]; then
        echo "Error: ccvv-bridge.h not found in build output"
        exit 1
    fi

    # Copy header to a stable location
    cp "$HEADER_DIR/ccvv-bridge.h" "$BUILD_DIR/ccvv-bridge.h"
    echo "  Library: $LIB_PATH"
    echo "  Header:  $BUILD_DIR/ccvv-bridge.h"
else
    LIB_PATH="$CORE_DIR/target/release/libccvv_lib.a"
    if [[ ! -f "$LIB_PATH" ]]; then
        echo "Error: --skip-rust specified but no pre-built library found at $LIB_PATH"
        exit 1
    fi
    echo "Skipping Rust build (using pre-built library)"
fi

# --- Step 2: Compile Swift with Rust library linked ---
echo "Compiling Swift..."
swiftc -o "$BUILD_DIR/$APP_NAME" main.swift \
    -framework Cocoa \
    -framework ApplicationServices \
    -target "$SWIFT_TARGET" \
    -import-objc-header "$BUILD_DIR/ccvv-bridge.h" \
    -L "$(dirname "$LIB_PATH")" \
    -lccvv_lib \
    -lsqlite3 \
    -O

echo "Creating app bundle..."
APP_BUNDLE="$BUILD_DIR/$APP_NAME.app"
rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources"
cp "$BUILD_DIR/$APP_NAME" "$APP_BUNDLE/Contents/MacOS/"
cp Info.plist "$APP_BUNDLE/Contents/"

# Stamp beta version for local dev builds (skip for --notarize release builds)
if [[ "$NOTARIZE" -eq 0 ]]; then
    COUNTER_FILE="$SCRIPT_DIR/../.beta-counter"
    BETA_NUM=1
    if [[ -f "$COUNTER_FILE" ]]; then
        BETA_NUM=$(( $(cat "$COUNTER_FILE") + 1 ))
    fi
    echo "$BETA_NUM" > "$COUNTER_FILE"
    BASE_VER=$(/usr/libexec/PlistBuddy -c "Print CFBundleShortVersionString" "$APP_BUNDLE/Contents/Info.plist")
    # CFBundleVersion must be digits-and-dots per Apple docs; store beta label in custom key
    /usr/libexec/PlistBuddy -c "Set :CFBundleVersion ${BASE_VER}.${BETA_NUM}" "$APP_BUNDLE/Contents/Info.plist"
    /usr/libexec/PlistBuddy -c "Add :CCVVBetaLabel string beta${BETA_NUM}" "$APP_BUNDLE/Contents/Info.plist" 2>/dev/null \
        || /usr/libexec/PlistBuddy -c "Set :CCVVBetaLabel beta${BETA_NUM}" "$APP_BUNDLE/Contents/Info.plist"
    echo "  Version: ${BASE_VER} (build ${BASE_VER}.${BETA_NUM})"
fi
if [[ -f "$SCRIPT_DIR/assets/ccvv.icns" ]]; then
    cp "$SCRIPT_DIR/assets/ccvv.icns" "$APP_BUNDLE/Contents/Resources/ccvv.icns"
else
    echo "Warning: icon asset missing at $SCRIPT_DIR/assets/ccvv.icns"
fi

# Copy CLI binary into bundle if it was built
CLI_PATH="${RUST_OUT_DIR:-$CORE_DIR/target}/release/ccvv"
if [[ -f "$CLI_PATH" ]]; then
    cp "$CLI_PATH" "$APP_BUNDLE/Contents/MacOS/ccvv-cli"
    echo "  CLI binary: $APP_BUNDLE/Contents/MacOS/ccvv-cli"
fi

echo "Signing..."
# Prefer Developer ID (distributable), fall back to Apple Development, then ad-hoc
SIGN_ID=$(security find-identity -v -p codesigning | grep "Developer ID Application" | head -1 | sed 's/.*"\(.*\)"/\1/' || true)
if [[ -z "$SIGN_ID" ]]; then
    SIGN_ID=$(security find-identity -v -p codesigning | head -1 | sed 's/.*"\(.*\)"/\1/' || true)
fi

if [[ -n "$SIGN_ID" ]]; then
    # Sign the CLI binary separately (must be signed before the bundle)
    if [[ -f "$APP_BUNDLE/Contents/MacOS/ccvv-cli" ]]; then
        codesign --force --options runtime \
            --sign "$SIGN_ID" "$APP_BUNDLE/Contents/MacOS/ccvv-cli"
    fi
    codesign --force --options runtime \
        --entitlements ccvv.entitlements \
        --sign "$SIGN_ID" "$APP_BUNDLE"
    echo "Signed with: $SIGN_ID"
else
    codesign --force --sign - "$APP_BUNDLE"
    echo "Warning: no signing identity found, using ad-hoc"
fi

# Notarize only when --notarize is passed
if [[ "$NOTARIZE" -eq 1 && "$SIGN_ID" == *"Developer ID"* ]]; then
    echo ""
    echo "Notarizing..."
    ZIP_PATH="$BUILD_DIR/$APP_NAME-notarize.zip"
    ditto -c -k --sequesterRsrc --keepParent "$APP_BUNDLE" "$ZIP_PATH"
    if xcrun notarytool submit "$ZIP_PATH" --keychain-profile "notarytool" --wait 2>&1; then
        echo "Stapling notarization ticket..."
        xcrun stapler staple "$APP_BUNDLE"
        rm -f "$ZIP_PATH"
    else
        echo ""
        echo "Notarization failed. To set up credentials:"
        echo "  xcrun notarytool store-credentials notarytool --apple-id YOUR_APPLE_ID --team-id $TEAM_ID"
        rm -f "$ZIP_PATH"
        exit 1
    fi
fi

echo ""
echo "Done: $APP_BUNDLE"
echo ""
echo "To install:  cp -r $APP_BUNDLE /Applications/"
echo "To run now:  open $APP_BUNDLE"
