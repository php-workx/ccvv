#!/bin/bash
set -euo pipefail

usage() {
    cat <<'EOF'
Usage:
  ./mac/brew-local-test.sh [install|uninstall|cycle] [--no-open]

Modes:
  install     Build, package, tap, and install cask (default)
  uninstall   Uninstall the local test cask
  cycle       Install then uninstall

Options:
  --no-open   Do not launch /Applications/ccvv.app after install
EOF
}

MODE="install"
OPEN_APP=1

for arg in "$@"; do
    case "$arg" in
        install|uninstall|cycle)
            MODE="$arg"
            ;;
        --no-open)
            OPEN_APP=0
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown argument: $arg" >&2
            usage
            exit 1
            ;;
    esac
done

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MAC_DIR="$REPO_ROOT/mac"
BUILD_DIR="$MAC_DIR/build"
SWIFT_FILE="$MAC_DIR/main.swift"
CASK_FILE="$REPO_ROOT/Casks/ccvv.rb"
BREW_REPO="$(brew --repository)"
LOCAL_TAP="runger/local"
LOCAL_TAP_DIR="$BREW_REPO/Library/Taps/runger/homebrew-local"
LOCAL_CASK_DIR="$LOCAL_TAP_DIR/Casks"
LOCAL_CASK_FILE="$LOCAL_CASK_DIR/ccvv.rb"
LOCAL_ZIP_FILE="$LOCAL_CASK_DIR/ccvv.zip"
SOURCE_ZIP_FILE="$BUILD_DIR/ccvv.zip"

brew_cmd() {
    HOMEBREW_NO_AUTO_UPDATE=1 "$@"
}

bump_patch_version() {
    local current
    current=$(grep -o 'let appVersion = "[^"]*"' "$SWIFT_FILE" | grep -o '[0-9][0-9.]*')
    if [[ -z "$current" ]]; then
        echo "Could not read version from main.swift" >&2
        exit 1
    fi

    local major minor patch
    IFS='.' read -r major minor patch <<< "$current"
    patch=$(( ${patch:-0} + 1 ))
    local new_version="$major.$minor.$patch"

    sed -i '' "s/let appVersion = \"$current\"/let appVersion = \"$new_version\"/" "$SWIFT_FILE"
    sed -i '' "s/version \"$current\"/version \"$new_version\"/" "$CASK_FILE"

    echo "$new_version"
}

ensure_local_tap() {
    if ! brew tap | grep -qx "$LOCAL_TAP"; then
        brew_cmd brew tap-new "$LOCAL_TAP"
    fi
    mkdir -p "$LOCAL_CASK_DIR"
}

build_zip() {
    (
        cd "$MAC_DIR"
        ./build.sh
    )
    ditto -c -k --sequesterRsrc --keepParent \
        "$BUILD_DIR/ccvv.app" \
        "$SOURCE_ZIP_FILE"
}

sync_cask_artifacts() {
    cp "$CASK_FILE" "$LOCAL_CASK_FILE"
    cp "$SOURCE_ZIP_FILE" "$LOCAL_ZIP_FILE"
}

install_cask() {
    pkill -x ccvv >/dev/null 2>&1 || true
    if brew list --cask | grep -qx ccvv; then
        brew_cmd brew uninstall --cask ccvv
    fi
    find "$(brew --cache)" -name '*ccvv*' -delete 2>/dev/null || true
    brew_cmd brew install --cask --force --no-quarantine "$LOCAL_TAP/ccvv"
    if [[ "$OPEN_APP" -eq 1 ]]; then
        open /Applications/ccvv.app
    fi
    echo ""
    echo "If the menu bar icon shows ⚠, grant Accessibility permission:"
    echo "  System Settings → Privacy & Security → Accessibility → enable ccvv"
}

uninstall_cask() {
    pkill -x ccvv >/dev/null 2>&1 || true
    if brew list --cask | grep -qx ccvv; then
        brew_cmd brew uninstall --cask ccvv
    else
        echo "ccvv is not installed."
    fi
}

case "$MODE" in
    install)
        NEW_VERSION=$(bump_patch_version)
        echo "Version bumped to $NEW_VERSION"
        ensure_local_tap
        build_zip
        sync_cask_artifacts
        install_cask
        ;;
    uninstall)
        uninstall_cask
        ;;
    cycle)
        NEW_VERSION=$(bump_patch_version)
        echo "Version bumped to $NEW_VERSION"
        ensure_local_tap
        build_zip
        sync_cask_artifacts
        install_cask
        uninstall_cask
        ;;
esac
