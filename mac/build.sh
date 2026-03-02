#!/bin/bash
set -e

APP_NAME="ccvv"
BUILD_DIR="build"
TEAM_ID="4JRN737CHR"
BUNDLE_ID="com.ccvv.app"
NOTARIZE=0
SKIP_RUST=0

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CORE_DIR="$SCRIPT_DIR/../core"

for arg in "$@"; do
    case "$arg" in
        --notarize) NOTARIZE=1 ;;
        --skip-rust) SKIP_RUST=1 ;;
    esac
done

mkdir -p "$BUILD_DIR"

# --- Step 1: Build Rust static library ---
if [[ "$SKIP_RUST" -eq 0 ]]; then
    echo "Building Rust core library..."
    if ! command -v cargo &>/dev/null; then
        echo "Error: cargo not found. Install Rust from https://rustup.rs"
        exit 1
    fi
    (cd "$CORE_DIR" && cargo build --package ccvv-lib --release)

    # Find the generated header
    RUST_OUT_DIR=$(cd "$CORE_DIR" && cargo metadata --format-version 1 2>/dev/null \
        | python3 -c "import sys,json; print(json.load(sys.stdin)['target_directory'])" 2>/dev/null \
        || echo "$CORE_DIR/target")
    HEADER_DIR=$(find "$RUST_OUT_DIR/release/build" -name "ccvv-bridge.h" -print -quit 2>/dev/null | xargs dirname 2>/dev/null || true)
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

echo "Signing..."
# Prefer Developer ID (distributable), fall back to Apple Development, then ad-hoc
SIGN_ID=$(security find-identity -v -p codesigning | grep "Developer ID Application" | head -1 | sed 's/.*"\(.*\)"/\1/' || true)
if [[ -z "$SIGN_ID" ]]; then
    SIGN_ID=$(security find-identity -v -p codesigning | head -1 | sed 's/.*"\(.*\)"/\1/' || true)
fi

if [[ -n "$SIGN_ID" ]]; then
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
    xcrun notarytool submit "$ZIP_PATH" --keychain-profile "notarytool" --wait 2>&1 && {
        echo "Stapling notarization ticket..."
        xcrun stapler staple "$APP_BUNDLE"
        rm -f "$ZIP_PATH"
    } || {
        echo ""
        echo "Notarization failed. To set up credentials:"
        echo "  xcrun notarytool store-credentials notarytool --apple-id YOUR_APPLE_ID --team-id $TEAM_ID"
        rm -f "$ZIP_PATH"
    }
fi

echo ""
echo "Done: $APP_BUNDLE"
echo ""
echo "To install:  cp -r $APP_BUNDLE /Applications/"
echo "To run now:  open $APP_BUNDLE"
