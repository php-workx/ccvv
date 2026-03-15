# [ ccvv ]_

**Double the tap, half the mess.**

[![macOS](https://img.shields.io/badge/macOS-Ventura+-black?logo=apple)](https://github.com/php-workx/ccvv/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](#license)

You copy text from a terminal, a PDF, or an AI agent. You paste it. It's a disaster of ANSI escape codes, hard line breaks, phantom spaces, and tracking URLs.

`ccvv` is a macOS menu bar utility that sanitizes and formats your clipboard when you double-tap `Cmd+C`. It strips the garbage, applies smart formatting, and saves the clean result back to your clipboard.

---

## Installation

**Homebrew (recommended):**
```bash
brew tap php-workx/ccvv
brew install --cask ccvv
```

**From source:**
Requires macOS Ventura+, Xcode Command Line Tools, and Rust 1.82+.
```bash
mac/build.sh
open build/ccvv.app
```

---

## How It Works

It lives in your menu bar as `[cc]` and stays out of your way until you need it.

**Double-tap `Cmd+C`** to clean the clipboard in place. The transform pipeline runs instantly and the `[cc]` icon flashes to `[██]` for 300ms as confirmation.

If you don't have Accessibility permission (or prefer manual control), click the `[cc]` icon and choose **Clean Clipboard Now**.

---

## Features

`ccvv` isn't just a plain-text stripper. It understands context:

* **Rich-Text Stripping:** Nukes HTML, CSS, and RTF MIME types. Guarantees a clean paste even in apps that ignore `Cmd+Shift+V`.
* **Agent & CLI Cleaner:** Strips ANSI terminal color codes, removes hidden zero-width characters, and deletes artifact markers (like Claude Code's `⏺`).
* **Whitespace & Line Break Manager:** Fixes copied PDF and email text. Unwraps hard line breaks into flowing paragraphs, but **preserves** intentional structure (double newlines, lists, and fenced code blocks).
* **Smart URL Stripper:** Transforms `https://www.github.com/repo?utm_source=slack` into `github.com/repo`. Strips tracking parameters (`utm_*`, `gclid`, `fbclid`, etc.) and optionally removes `www.` and the scheme.
* **Rich-Text Inference:** Infers inline code from monospace fonts/colors in the copied rich text and converts them to Markdown backticks before stripping the rich payload.
* **JSON Prettify/Flatten:** Detects JSON in the clipboard and prettifies it. Use the CLI `--flatten-json` flag to compact it to a single line instead.
* **Table Detection:** Detects pipe-delimited, box-drawing, and TSV/CSV tables and converts them to Markdown table format.
* **Sensitive Content Filter:** Detects PEM keys, API tokens, JWTs, and AWS keys. Skips transformation and notifies you so secrets aren't accidentally modified.

---

## CLI

The `ccvv` CLI reads from stdin and writes the cleaned result to stdout.

```bash
# Clean text through the full pipeline
echo "some   messy   text" | ccvv

# Preview changes as a unified diff
echo "visit https://example.com?utm_source=slack" | ccvv preview

# Use individual transforms
echo "text" | ccvv --strip-urls
echo '{"a":1}' | ccvv --prettify-json
echo "text" | ccvv --normalize
echo "text" | ccvv --unwrap

# Use a specific config profile
echo "text" | ccvv --profile terminal

# System diagnostics
ccvv doctor

# Validate your config file
ccvv validate

# Browse clipboard history
ccvv history --limit 5
ccvv history --search "github"
ccvv history --type url
```

---

## Configuration

Configuration is optional. Without a config file, sensible defaults apply.

**Location:** `~/.ccvv/config.toml`

```toml
[settings]
normalize_unicode = true
whitespace_cleanup = true
agent_strip = true
url_cleaning = true
url_strip_scheme = false
auto_wrapper = false          # disabled by default (shell substitution risk)
sensitive_filter = true
max_input_bytes = 1048576     # 1 MB; larger content passes through unchanged

[[rules]]
name = "Strip internal tracker IDs"
pattern = 'INTERNAL-\d{4,}'
replace = ""
```

You can also toggle features from the menu bar: click `[cc]` > **Preferences**.

**Profiles** let you define context-specific overrides (e.g., a `terminal` profile that disables URL stripping):

```toml
[profiles.terminal]
url_cleaning = false
auto_wrapper = false
```

Use profiles via the CLI with `--profile terminal` or assign them to apps in the config.

---

## Privacy & Security

* **No network access.** This is a hard invariant enforced at the dependency level. `ccvv` never phones home, sends telemetry, or makes any network connections.
* **Accessibility permission** is required for `Cmd+C` double-tap detection via `CGEventTap`. The tap is listen-only: it observes key-down/key-up events for `Cmd+C` timing and never logs, modifies, or blocks any keystrokes. If permission is denied, the app degrades to manual mode (menu click only).
* **Clipboard history** is stored locally in `~/.ccvv/history.db` (SQLite). It keeps the last 50 cleaned entries, auto-prunes older ones, and is excluded from Time Machine and Spotlight. Raw clipboard content is not stored unless you opt in with `history_store_raw = true`.
* **Sensitive content filter** runs before any transforms. If the clipboard looks like it contains a secret (PEM key, API token, JWT, etc.), `ccvv` skips processing and notifies you.

---

## Menu Bar

Click the `[cc]` icon to:

1. **Clean Clipboard Now** — run the pipeline manually (no double-tap needed).
2. **Pause** — temporarily disable the double-tap listener.
3. **History** — browse your last 50 clipboard entries.
4. **Preferences** — toggle individual features on or off.

---

## Contributing

See `specs/` for the full [functional spec](specs/functional_v1.md) and [technical spec](specs/technical_v1.md).

## Linux

`ccvv-linux` is the native Linux daemon in `core/ccvv-linux`. It uses the same transform pipeline and local history store as the macOS app, but runtime behavior depends on the desktop session.

Linux rollout status: `technical beta` for Linux power users.

Broad external rollout stays blocked until the Linux stability gate is complete:

1. Regression coverage for the high-risk copy/detect/format flows is in place.
2. Linux CI is green, including the headless wlroots Wayland stability harness.
3. The manual validation matrix is filled for X11, wlroots Wayland, and GNOME Wayland limited mode.
4. A short internal soak completes without copy/content/formatting regressions.

The current validation template lives in `docs/plans/2026-03-11-linux-stability-validation-matrix.md`.

Linux runtime modes:

1. `Automatic` - X11 and supported Wayland compositor clipboard-manager protocols. Clipboard changes are monitored automatically and `Clean Clipboard Now` is available.
2. `Limited` - Wayland sessions without native automatic monitoring but with an explicit manual-clean path. Automatic cleaning is disabled.
3. `DiagnosticsOnly` - no usable desktop clipboard backend. The daemon still exposes status and local control, but manual clean is unavailable.

Linux limitations:

1. GNOME/Mutter Wayland remains limited/manual rather than ordinary automatic double-copy mode.
2. Sanitizing a mixed-content clipboard copy republishes text targets only.
3. Tray sidecars are optional socket clients. The daemon keeps running without a tray host or without a sidecar.

Linux packaging artifacts live under `linux/` and the Linux release tarball currently ships:

1. `ccvv-linux`
2. `ccvv-tray-sni`
3. `ccvv-linux.desktop`
4. `ccvv-linux.service`

The legacy `ccvv-indicator-legacy` helper remains optional and is only built when `--features xembed` is enabled.

## License

MIT
