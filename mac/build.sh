#!/bin/bash
set -e

APP_NAME="ccvv"
BUILD_DIR="build"
TEAM_ID="4JRN737CHR"
BUNDLE_ID="com.ccvv.app"
NOTARIZE=0

for arg in "$@"; do
    case "$arg" in
        --notarize) NOTARIZE=1 ;;
    esac
done

mkdir -p "$BUILD_DIR"

echo "Compiling..."
swiftc -o "$BUILD_DIR/$APP_NAME" main.swift \
    -framework Cocoa \
    -framework ApplicationServices \
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
    codesign --force --options runtime --sign "$SIGN_ID" "$APP_BUNDLE"
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
