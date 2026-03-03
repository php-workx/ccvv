# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**ccvv** ("copy-copy, paste-paste") is a clipboard sanitizer that cleans text when users double-tap Cmd+C. It strips formatting, ANSI codes, tracking URLs, and normalizes whitespace while preserving Markdown structure. Menu bar app on macOS; simple scripts on Linux/Windows.

Current version: **1.2.0** (defined in `mac/main.swift` as `appVersion`)

## Architecture

### Current State (v1.0 — shipped)

The macOS app is a **single-file Swift app** (`mac/main.swift`, ~700 lines) with no Rust code yet. It handles:
- Text cleaning pipeline (the `ccvv()` function)
- Rich text → plain text with inline code inference from `NSAttributedString`
- `CGEventTap` for Cmd+C double-tap detection
- `NSStatusItem` menu bar UI with `[cc]` icon

Linux (`linux/ccvv`) is a Python script using xclip/wl-clipboard.
Windows (`windows/ccvv.ps1`) is a PowerShell script.

### Target State (v1.1 — in specs, not yet built)

Per `specs/technical_v1.md`, the intended architecture is:

```
ccvv-lib (Rust)          ← transform engine, config, history, FFI
  ├── macOS App (Swift)  ← links via C FFI (staticlib)
  └── ccvv CLI (Rust)    ← links directly (native Rust API)
```

The Rust workspace lives under `core/` with `ccvv-lib` and `ccvv-cli` crates. The transform pipeline has 8 ordered stages (see specs). Stage 1 (rich text extraction) stays in Swift; stages 2-8 run in Rust.

## Build & Run

### macOS App
```bash
cd mac
./build.sh              # compile + sign + bundle → mac/build/ccvv.app
./build.sh --notarize   # also notarize via Apple notarytool
```

Build output: `mac/build/ccvv.app` and `mac/build/ccvv` (bare binary).

The build script uses `swiftc` directly (no Xcode project, no SPM). Frameworks: Cocoa, ApplicationServices. Signing prefers Developer ID, falls back to ad-hoc.

### Running
```bash
open mac/build/ccvv.app        # launch menu bar app
# or directly:
mac/build/ccvv                 # bare binary (no app bundle)
```

Requires macOS Accessibility permission for double-tap detection. Without it, the app still works via left-click "Clean Clipboard Now" on the `[cc]` menu bar icon.

### Linux / Windows
```bash
linux/ccvv                     # requires python3 + xclip or wl-clipboard
windows/ccvv.ps1               # PowerShell, reads/writes system clipboard
```

### Homebrew (local testing)
```bash
cd mac && ./brew-local-test.sh
```
Cask definition: `Casks/ccvv.rb`

## Key Files

| File | Purpose |
|------|---------|
| `mac/main.swift` | Entire macOS app — text cleaning, rich text inference, event tap, UI |
| `mac/build.sh` | Build + sign + optional notarize |
| `mac/Info.plist` | App bundle metadata (LSUIElement=true hides dock icon) |
| `linux/ccvv` | Python clipboard cleaner for Linux |
| `windows/ccvv.ps1` | PowerShell clipboard cleaner for Windows |
| `specs/functional_v1.md` | Product spec (features, UX, config format) |
| `specs/technical_v1.md` | Technical spec (Rust architecture, security model, stage specs) |

## Code Conventions

- The Swift code is a flat single-file design — no modules, no SPM packages. Keep it that way until the Rust migration.
- Text cleaning functions are pure (input string → output string). The `ccvv()` function is the main pipeline entry point.
- All three platform implementations (Swift, Python, PowerShell) share the same algorithm: normalize line endings → detect terminal width → process blocks (code fences verbatim, paragraphs compacted) → join with double newlines.
- The `⏺` character (Claude Code artifact) is explicitly stripped at line starts.
- Bullet markers `•`, `◦`, `▪` are normalized to `- `.
- The app version string appears in both `mac/main.swift` (`appVersion`) and `Casks/ccvv.rb` — keep them in sync.
- Bundle ID: `com.ccvv.app`, Team ID: `4JRN737CHR`.

## Config (planned, not yet implemented)

Config file: `~/.ccvv/config.toml` (TOML format). See `specs/functional_v1.md` §6 for schema.
History DB: `~/.ccvv/history.db` (SQLite). Not yet implemented.

## Security Constraints

- **No network access ever.** This is a hard invariant enforced by dependency bans (`deny.toml` bans reqwest, hyper, tokio, etc.).
- **No `fancy-regex`.** Only the `regex` crate (linear-time guarantee) to prevent ReDoS from user rules.
- Clipboard content is untrusted input — no unbounded allocations, graceful pass-through on unexpected input.
- Sensitive content filter runs before transforms (PEM keys, API tokens, JWTs → skip and notify).
- Auto-wrapper (backtick wrapping) is disabled by default due to shell command substitution risk.

## Logs

macOS app logs to `~/Library/Logs/ccvv.log` (timestamped, includes clipboard diagnostics).
