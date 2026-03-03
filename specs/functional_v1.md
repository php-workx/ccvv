# Functional Specification: ccvv v1.1

**Logo Concept:** `[ ccvv ]_`
**Primary Slogan:** "Double the tap, half the mess."
**Secondary Slogan:** "Copy-copy, paste-paste, no-waste."

**Scope:** macOS-deep v1 — Rust core library, Swift macOS shell, CLI companion
**Deferred:** Smart Paste, "Paste As" chooser, Linux/Windows daemon, Invisible mode

---

## 1. Product Overview

**ccvv** is a lightweight macOS menu bar utility designed for developers, technical writers, and power users. It acts as an intelligent clipboard sanitizer and formatter. By utilizing a rapid "double-tap" of the `Cmd+C` shortcut, users can instantly strip unwanted artifacts, normalize text, and intelligently format data without interrupting their workflow.

---

## 2. User Interface & Experience

`ccvv` operates as a background menu bar utility, providing status and control via a system tray icon.

### Visual Feedback Loop (The "Blink")
The menu bar icon provides instant visual confirmation when a transformation occurs, training the user's muscle memory.
* **Active (Listening):** Solid / high-contrast template image — two overlapping "C" letters.
* **Paused:** Distinct pause-bar template image, clearly different from active state.
* **Success Flash:** Checkmark template image for 300ms upon successful double-tap cleaning.
* **Miss Indicator:** The status item button briefly highlights for ~200ms when a second copy lands *outside* the double-tap window but within 2× the window, signaling that the timing was too slow.
* **No Accessibility Permission:** Warning/exclamation template image, shown when Accessibility permission is denied or revoked.

All icons are `NSImage` template images (18×18pt, PDF or SF Symbols), ensuring automatic adaptation to Light and Dark mode. Text-based status items are not used, as they are fragile across macOS versions.

### The HUD Toast
A brief semi-transparent toast window appears near the cursor showing a compressed summary of what changed (e.g., "Unwrapped 8 lines, stripped 2 params"). Positioned 20pt below and right of the cursor, clamped to the active screen's visible frame. Auto-dismissed after a duration determined by Confidence Mode.

When content is skipped, the toast shows the reason: "Skipped: looks like a secret" or "Skipped: content too large".

**Confidence Mode:** Toast duration decreases as the user becomes familiar with the tool:

| Transform count | Toast duration |
|-----------------|----------------|
| 0–29 | 1.5s |
| 30–39 | 1.0s |
| 40–49 | 0.5s |
| 50+ | Toast disabled (icon flash only) |

The user can override this with "Always show toast" or "Never show toast" in Preferences.

### Menu Bar Menu
Clicking the icon reveals a minimal menu:
* **Pause / Resume:** Temporarily disables the double-tap listener when the user *wants* to keep rich formatting. Pause state is transient and resets on app launch.
* **Clean Clipboard Now:** Manual one-click clipboard cleaning — always available, including when Accessibility permission is denied.
* **History:** Opens a popover showing the local clipboard history (with search). Each entry shows a truncated preview, its auto-detected type badge (URL, Code, Prose, Table, JSON), and a relative timestamp. Click to restore to clipboard.
* **Preferences:** Toggle specific smart features.
* **Quit**

When Accessibility permission is denied, the menu also shows an "Open System Settings" button for convenient re-granting.

### Preferences Surface
Preferences are a single `NSWindow` with a scrollable list of labeled on/off toggles grouped by feature category, with a link to the config file for advanced settings. No tabs, no nested panels.

Toggle groups:
* **Text Cleanup:** Whitespace & line break cleanup, Unicode normalization, Agent artifact stripping
* **Formatting:** JSON detect & prettify, Table-to-Markdown, Code fence wrapping, Backtick auto-wrapper (off by default, with shell safety warning)
* **URLs:** Strip tracking parameters, Strip URL scheme (aggressive, off by default)
* **Privacy:** Sensitive content filter (on by default), Store raw clipboard in history (off by default)
* **Feedback:** Show HUD toast

Bottom row: `[Open Config File]` and `[Reset to Defaults]` buttons.

Toggle changes are written back to the TOML config file using an atomic write protocol (temp file → fsync → rename) with `toml_edit` to preserve comments and formatting.

### First-Run Onboarding
On first launch, a single non-modal overlay appears after the Accessibility permission system dialog is dismissed.

If Accessibility permission IS granted:
> *Copy something, then tap ⌘C again within a beat. Watch the icon flash. That's it.*

If Accessibility permission is NOT yet granted:
> *Grant Accessibility access, then copy something and tap ⌘C again within a beat.*

The overlay includes "Try it now" and "Dismiss" buttons. Once the user completes one successful double-tap (or dismisses), the overlay disappears permanently.

The onboarding also communicates two product decisions:
* "ccvv cleans your clipboard to plain text. Rich formatting will be removed."
* "ccvv never connects to the internet. Your clipboard stays on your device."

---

## 3. Core Interactions

### Detection Strategy
`ccvv` uses a `CGEventTap` (macOS Accessibility API) to detect rapid repeated `Cmd+C` keystrokes. It monitors keystroke cadence — not clipboard writes — to identify the double-tap gesture. After detecting the cadence, it reads and processes the system pasteboard (`NSPasteboard`).

This means `ccvv` detects `Cmd+C` double-taps specifically. It does **not** detect clipboard writes from right-click → Copy, Edit menu → Copy, programmatic copies, tmux yank, or any non-keyboard copy mechanism. These can instead be cleaned via the "Clean Clipboard Now" menu item or the CLI companion.

Because `ccvv` monitors `Cmd+C` keystrokes (not arbitrary clipboard writes), it does not conflict with `Ctrl+C` (SIGINT) in terminals, Emacs `C-c` prefix chords, or IDE double-tap bindings that write *different* content on the second press.

### Accessibility Permission
ccvv requires macOS Accessibility permission to install the `CGEventTap`.

* **On first launch:** The app prompts for Accessibility access via the system dialog and links directly to System Settings.
* **If denied or revoked:** ccvv degrades to **manual mode** — the menu bar icon shows a warning badge, and the user can clean the clipboard via the "Clean Clipboard Now" menu item. All transform and history functionality works normally; only automatic double-tap detection is disabled.
* **Periodic re-check:** Every 30 seconds, `AXIsProcessTrusted()` is called (without re-prompting). If permission is newly granted, the event tap is registered automatically. If revoked at runtime, the app enters manual mode and shows a one-time notification.

### Double-Tap Mechanics
The detection window defaults to **300ms** (configurable from 150–500ms).

The strategy is **"detect cadence via keystrokes, confirm via pasteboard"**:
1. First `Cmd+C` detected via `CGEventTap`. Record the timestamp and begin polling `NSPasteboard.changeCount` (every 10ms, up to 200ms) to confirm the clipboard actually updated. Snapshot the content hash (SHA-256).
2. If a second `Cmd+C` arrives within the detection window:
   * Poll for a new `changeCount` increment, then hash the new content.
   * **Same content** (hashes match): this is a deliberate double-tap. Apply sanitization.
   * **Different content** (hashes differ): user selected something new. Update baseline, do NOT transform.
3. If no second `Cmd+C` arrives within the window, nothing happens. The copy was a normal single copy.

`ccvv` **never delays, blocks, or intercepts** the first copy. Transformation is always a post-processing step after the second keystroke. A write-back guard prevents ccvv's own pasteboard write from being misdetected as a new user copy.

**Adaptive timing:** During initial use, `ccvv` passively records the interval of confirmed double-taps. After ~20 samples, it computes a personalized threshold (median + 1.5× IQR, clamped to 150–500ms). The user can override this with a fixed value in Preferences or the config file (`"auto"` for adaptive, or a fixed integer).

### Action: The Smart Copy (`Cmd+C, C`)
* **Behavior:** After detecting a valid double-tap, applies active sanitization rules and overwrites the clipboard with the cleaned plain text.
* **Pasteboard type check:** Before reading, ccvv confirms the pasteboard contains a `.string` type. If the pasteboard contains only files, images, or other non-text types, it is left untouched (preventing destruction of Finder file copies, image pastes, etc.).
* **Result:** Triggers the visual Success Flash (checkmark icon for 300ms) and (if enabled) the HUD toast with a summary of changes applied.

### Bypass Modifier
Holding `Option` during a `Cmd+C` suppresses `ccvv` for that single operation, even if it would otherwise trigger as a double-tap. This provides an instant escape hatch without needing to Pause/Resume via the menu.

---

## 4. Transform Engine

Transformations are applied as an **ordered pipeline**. Each stage receives the output of the previous stage, and stages can be individually toggled.

### Pipeline Order (Smart Copy)
```
1. Rich text → plain text (Swift — strips RTF/HTML, infers inline code from fonts)
2. Unicode & encoding normalization
3. Whitespace & line break cleanup
4. Agent artifact stripping (⏺, ANSI, zero-width)
5. Structural detection (JSON, table, code)
6. URL cleaning
7. Auto-wrapper (backtick wrapping) — disabled by default
8. User-defined regex rules
```

Stage 1 runs in Swift (reads `NSAttributedString` from `NSPasteboard`, infers inline code from monospace fonts/background colors, inserts backtick markers, then strips to plain text). Stages 2–8 run in the Rust core library via FFI.

### Design Constraints
* **Idempotency:** Running the pipeline twice on the same input must produce the same output. No stage introduces artifacts that a later stage would want to strip.
* **Structural detection is conservative:** If the engine isn't confident the content is JSON/table/code, it skips the structural stage and passes raw text through. False negatives are preferable to false positives.
* **Unwrap safeguards:** The line-break unwrapper uses heuristic signals (line length variance, sentence-ending punctuation, indentation patterns) to distinguish hard-wrapped prose from intentional line breaks. Content that resembles log output, columnar data, ASCII art, or pre-formatted tables is left untouched. When confidence is below threshold, the unwrapper is skipped entirely for that block.
* **Input size limit:** Content larger than 1 MB (configurable) passes through untransformed. The user is notified via the HUD toast.
* **Output size guard:** If any stage produces output larger than 2× the input, the pipeline aborts and returns the original input untransformed.

### Sensitive Content Filter
Before any transform stage runs, the pipeline checks input against a set of secret-detection patterns (PEM private keys, API key prefixes, JWTs, AWS access keys, GitHub tokens, Slack tokens, long base64 strings). If any match, the content is returned untouched and the user is notified: "Skipped: looks like a secret."

The filter is enabled by default and can be disabled in Preferences or the config file. This is a best-effort heuristic, not a security boundary.

---

## 5. The "Smart" Feature Set

These features are enabled by default (except Auto-Wrapper) and can be toggled individually in Preferences or the config file.

### The "Blind" Paste (Core Pillar)
The `Cmd+C, C` double-tap actively strips all rich-text MIME types (HTML, RTF) from the clipboard, leaving only plain text. This works universally for any `Cmd+C` copy, even in applications that do not natively support a "paste without formatting" shortcut. The result is always plain text written back to `NSPasteboard` as `.string` type only.

### Whitespace & Line Break Manager
Fixes the most common layout issues when copying from PDFs, emails, or poorly formatted code.
* **Strip Whitespace:** Removes extraneous trailing whitespace from individual lines. Collapses interior runs of ≥ 3 spaces to a single space.
* **Unwrap Hard Line Breaks:** Detects and rejoins lines that were hard-wrapped (common in PDFs or legacy email clients) back into naturally flowing paragraphs. Uses line-length variance, sentence-ending punctuation, and indentation as heuristic signals. Skips blocks that resemble log output, columnar data, ASCII tables, or pre-formatted text.
* **Preserve Structure:** Keeps deliberate paragraph breaks (double newlines), list items (`-`, `*`, `1.`), and fenced code blocks intact.
* **Bullet normalization:** `•`, `◦`, `▪` → `-`.

### Unicode & Encoding Normalization
Silently fixes the invisible characters that break grep patterns, SQL queries, and config files.
* **Smart Quotes → Straight Quotes:** `"` `"` → `"`, `'` `'` → `'`.
* **Typographic Dashes:** `—` (em-dash) → `--`, `–` (en-dash) → `-` (configurable: some users prefer preserving em-dashes).
* **Invisible Characters:** Strips non-breaking spaces (`\u00a0`), zero-width spaces (`\u200b`), zero-width joiners/non-joiners, byte-order marks, and other Unicode control characters that cause silent failures.
* **Encoding Repair:** Detects common mojibake patterns (Latin-1 interpreted as UTF-8) using a lookup table of ~30 common sequences. Applies repair only at high confidence (≥3 matching patterns in the same paragraph). Falls back to leaving content unchanged.
* **NFC Normalization:** Ensures composed Unicode characters (e.g., `é` as a single codepoint rather than `e` + combining accent).

### Smart Strip (URLs)
Cleans tracking noise from URLs while preserving usability.
* **Default behavior:** Strips known tracking parameters (`utm_*`, `gclid`, `fbclid`, `mc_*`, `si`, `ref_src`, `_ga`, `_gl`) and strips `www.` from the host. Preserves the scheme (`https://`) — the result is still a valid, clickable URL.
* **Aggressive mode** (opt-in via config): Also strips `https://` for display-only contexts.
* *Result (default):* `https://www.github.com/repo?utm_source=slack` → `https://github.com/repo`

**Parameter policy** is configurable via allowlist/denylist:
```toml
[url_params]
# Global denylist (stripped from all URLs)
deny = ["utm_*", "gclid", "fbclid", "mc_*", "si", "ref_src"]

# Per-domain overrides
[url_params.overrides."youtube.com"]
keep = ["v", "t", "list"]     # always preserve these
deny = ["si", "pp", "feature"] # strip these

[url_params.overrides."github.com"]
keep = ["q", "tab", "type"]   # preserve search params
```

URLs inside code fences or backtick-wrapped spans are not modified.

**Known risk:** Stripping `www.` can break URLs where `www.example.com` and `example.com` resolve to different hosts. Users can add per-domain overrides or disable URL cleaning for specific domains.

### Table-to-Markdown
When the clipboard contains tabular data (HTML `<table>`, TSV from Excel/Google Sheets, CSV), automatically converts it into a pipe-delimited Markdown table.
* **Delimiter detection:** Tries tab first (most common from spreadsheets), then comma, then semicolon. Respects quoted fields (RFC 4180).
* **Header inference:** First row is treated as a header if it contains distinct, non-numeric values. Otherwise a generic `Col 1 | Col 2 | ...` header is generated.
* **Column alignment:** Right-aligns columns where >80% of values are numeric; left-aligns everything else.
* **Multi-line cells:** Collapsed to a single line (newlines replaced with `<br>` for Markdown compatibility).
* **Output size guard:** If the Markdown table exceeds 2× the input size, the content passes through untransformed.

### Code Fence Detection
When the clipboard contains multi-line content that looks like code (≥40% indented lines, shebang line, or high keyword density for known languages), wraps it in a fenced code block with a best-guess language identifier:
~~~
```python
def hello():
    print("world")
```
~~~
Language detection uses lightweight keyword-frequency heuristics (`def`/`import`/`print(` → Python, `fn`/`let mut` → Rust, `function`/`const`/`=>` → JavaScript, etc.). Falls back to bare ``` fences if confidence is low. Already-fenced content is skipped.

### Platform & Agent-Specific Smarts
Targeted sanitization for modern AI and CLI workflows.
* **macOS Rich Text Inference:** When copying rich text on macOS, ccvv infers inline code by detecting monospace fonts or specific text background colors in the `NSAttributedString`, converting them into plain-text markdown backticks (`` ` ``) before stripping the rich-text payload. This happens in Swift (Stage 1) before the Rust pipeline.
* **Claude Code / Agent Cleaner:** Explicitly targets and strips CLI agent artifacts, such as the `⏺` markers from Claude Code, ANSI escape codes (terminal colors), and hidden zero-width characters that cause silent syntax errors.

### JSON Prettifier
When the entire clipboard content parses as valid JSON, re-serializes it with `serde_json::to_string_pretty()` for readability. Partial JSON embedded in prose does not trigger this feature. Deep nesting (>128 levels) passes through untransformed.

### Auto-Wrapper (Disabled by Default)
Identifies technical strings and wraps them in backticks (`` ` ``) for standard Markdown formatting.
* Targets: CLI flags (`--flag`), snake_case identifiers, file paths, filenames, host:port patterns, camelCase and SCREAMING_CASE tokens.
* Lines starting with `$` or `>` (shell prompts) have the entire line content wrapped.
* Tokens inside existing backtick spans, code fences, or URLs are skipped.

**Disabled by default** due to shell safety risk: backticks are command substitution delimiters in bash/zsh. Content wrapped in backticks and pasted into a terminal could be executed. Users who primarily paste into Markdown editors or chat apps can enable it in Preferences.

---

## 6. User Rules (`.ccvv` Config)

Power users can define custom sanitization rules in a config file (TOML format). Rules are applied as the last stage of the transform pipeline (after all built-in features). Maximum 50 user rules; the `regex` crate guarantees linear-time matching (no ReDoS risk). Per-rule timeout of 50ms.

### Profiles
The config file supports named profiles that override feature toggles and pipeline behavior per context:

```toml
[profiles.terminal]
json = "flatten"
code_fence = false
url_strip_scheme = true

[profiles.markdown]
json = "prettify"
code_fence = true
table_to_markdown = true

[profiles.raw]
# Disables all transforms — useful for "I just want plain text"
blind_paste = true
all_smart_features = false
```

Profiles can be bound to specific apps in `[paste_targets]` or invoked explicitly via the CLI: `ccvv --profile terminal`.

**Config file location** (checked in order):
1. Explicit path (CLI `--config` or FFI)
2. `~/.ccvv/config.toml`
3. If none found, all defaults apply. No error — the config file is optional.

If the config file exists but fails TOML parsing, ccvv does **not** silently fall back to defaults. Instead: the app shows a warning badge on the menu bar icon with the parse error in the menu, and keeps the previously loaded valid config (if any).

```toml
# ~/.ccvv/config.toml

[settings]
double_tap_window_ms = 300   # 150–500, or "auto" for adaptive
hud_toast = true
url_strip_scheme = false     # true = aggressive mode (strip https://)
sensitive_filter = true      # skip content that looks like secrets
history_store_raw = false    # only store cleaned text in history

[settings.em_dash]
replace = "--"               # or "—" to preserve

# App-specific overrides (used by profiles)
[paste_targets]
"com.googlecode.iterm2" = "terminal"
"com.microsoft.VSCode" = "editor"
"com.slack.Slack" = "raw"    # never transform when pasting to Slack

[[rules]]
name = "Strip internal tracker IDs"
pattern = 'INTERNAL-\d{4,}'
replace = ""

[[rules]]
name = "Clean Jira prefix"
pattern = '^(PROJ-\d+)\s*[-:]\s*'
replace = '$1 '

[[rules]]
name = "Redact API keys"
pattern = 'sk-[A-Za-z0-9]{20,}'
replace = "sk-***REDACTED***"
```

Rules support full regex with capture groups.

### Config Integrity
* **File permissions:** On load, ccvv warns if the config file is not owned by the current user or is group/world-writable.
* **Atomic writes:** Preferences UI writes use temp file → fsync → rename to prevent partial corruption.
* **Merge semantics:** Default exclusions (password managers, VM apps) are always present. User-defined exclusions are appended, not replaced.

---

## 7. Compatibility & Conflicts

### Known Non-Conflicts
Because `ccvv` monitors `Cmd+C` keystrokes via `CGEventTap` (not clipboard writes), the following do **not** conflict:
* `Ctrl+C` (SIGINT) in terminals — not a `Cmd+C` keystroke
* Emacs `C-c` prefix chord — not `Cmd+C`
* IDE "copy reference" on double `Cmd+C` — these typically write *different* content on the second press, which `ccvv` detects via hash comparison and treats as a new baseline (no transform)
* Right-click copy, Edit menu copy, programmatic copies — invisible to `CGEventTap` and therefore ignored (use "Clean Clipboard Now" or the CLI for these)

### Known Limitation: Cmd+C Only
v1 detects `Cmd+C` keystrokes only. Non-keyboard copy methods (right-click → Copy, Edit menu → Copy, programmatic `NSPasteboard` writes, tmux yank) are not detected. These can be cleaned manually via the "Clean Clipboard Now" menu item or the CLI companion.

### App Exclusion List
Some apps should never be intercepted. The config supports a global exclusion list:
```toml
[exclusions]
apps = [
  "com.vmware.fusion",       # remote desktop — let guest OS handle clipboard
  "com.parallels.desktop",
  "org.keepassxc.keepassxc",  # password managers — never touch
]
```
When the frontmost app matches an exclusion, `ccvv` ignores all double-tap events regardless of timing. Default exclusions for common password managers and VM apps are built in and cannot be accidentally removed by user config (merge semantics).

---

## 8. Privacy & History

Because `ccvv` overwrites the clipboard with cleaned text, users need a safety net if they accidentally destroy text they intended to keep raw.

* **Local-Only Database:** SQLite on the user's machine. Zero cloud telemetry. Zero network connections (enforced technically, not just by policy).
* **Privacy-First Storage:** By default, only the SHA-256 hash of the raw text and the cleaned text are stored. Raw text is never persisted to disk unless explicitly opted in (`history_store_raw = true`). Cleaned text that matches sensitive-content patterns is redacted in the database.
* **Undo Stack:** An in-memory ring buffer holds the last 10 raw clipboard entries for quick undo. Lost on process termination (intentional — reduces persistence of sensitive data). Entries are securely zeroed on eviction.
* **History Retention:** Stores the last 50 committed entries. Older entries are pruned automatically.
* **Search & Categories:** History entries are automatically tagged by type (URL, Code, Prose, Table, JSON) based on content heuristics. The History panel supports substring search and category filtering.
* **Protection:** The data directory is excluded from Time Machine backups and Spotlight indexing. Database and directory permissions are set to owner-only access (0600/0700).
* **Crash Safety:** A two-phase commit protocol ensures the clipboard and history database are always consistent — either both reflect the cleaned state, or neither does.

---

## 9. CLI Companion

Beyond the menu bar app, `ccvv` ships a CLI binary that exposes the same Rust transformation engine (linked directly, not via FFI) for scripting and terminal workflows:

```bash
# Pipe clipboard through ccvv
pbpaste | ccvv | pbcopy

# Apply specific transforms
ccvv --strip-urls < input.txt
ccvv --flatten-json < api-response.json
ccvv --prettify-json < api-response.json
cat README.md | ccvv --unwrap > clean.md
ccvv --normalize < input.txt

# Use a named profile
ccvv --profile terminal < input.json

# Preview transforms without modifying clipboard
ccvv preview                     # shows: input → output diff + rules fired
ccvv preview --clipboard         # preview what double-tap would produce

# History access
ccvv history                     # list 10 most recent entries
ccvv history 5                   # retrieve 5th-most-recent entry
ccvv history --search "api"
ccvv history --type json         # filter by auto-detected category

# Diagnostics
ccvv doctor                      # print platform info, permissions status,
                                 # config path & validation, feature states,
                                 # sensitive filter status, history DB info,
                                 # backup exclusion status

# Config validation
ccvv validate                    # check config syntax + regex validity
```

The CLI shares the identical Rust transformation library with the menu bar app — not a separate implementation. History access from the CLI reads the same SQLite database (WAL mode enables concurrent reads).

---

## 10. Platform & Installation

`ccvv` v1 targets macOS exclusively, with a natively compiled Rust core and Swift/AppKit shell.

### Performance Budget
* **Memory (RSS):** < 15 MB idle, < 25 MB during transformation. History SQLite DB is memory-mapped on demand, not preloaded.
* **CPU:** The `CGEventTap` fires only on `Cmd+C` keystrokes. The daemon should register **< 1 wakeup/second** when idle. Pasteboard `changeCount` polling is short-lived (up to 200ms per detected keystroke, at 10ms intervals).
* **Startup:** < 200ms to tray-icon-visible on a cold start.

### Build Targets

| Platform | Artifact | Status |
|----------|----------|--------|
| macOS (native arch) | App bundle + CLI binary | Full implementation |
| Linux | — | Deferred to v2 |
| Windows | — | Deferred to v2 |

The macOS app builds for the native architecture. Universal binary (arm64 + x86_64 via `lipo`) is deferred.

### Distribution
* **macOS:** Homebrew cask (pre-built, signed, notarized binary): `brew install --cask ccvv`
* Developer ID code signing with Hardened Runtime, notarized via Apple's `notarytool`.

---

## 11. v1 Scope Exclusions (Deferred to v2)

| Feature | Reason for deferral |
|---------|---------------------|
| Smart Paste (context-aware destination formatting) | Architecturally complex, fragile window detection |
| "Paste As" chooser palette | Depends on Smart Paste infrastructure |
| Linux daemon (X11/Wayland tray app) | Platform scope is macOS-deep for v1 |
| Windows daemon (Win32 tray app) | Platform scope is macOS-deep for v1 |
| Invisible mode (hide icon, haptic/screen-edge feedback) | Nice-to-have, not core |
| Universal binary (arm64 + x86_64) | Build for native arch in v1, add lipo later |
| History at-rest encryption (SQLCipher) | Raw text not stored by default, reducing need |
| Non-keyboard copy detection (right-click, Edit menu, programmatic) | Would require high-frequency pasteboard polling exceeding idle CPU budget |
