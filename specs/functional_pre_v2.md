# Functional Specification: ccvv

**Logo Concept:** `[ ccvv ]_`
**Primary Slogan:** "Double the tap, half the mess."
**Secondary Slogan:** "Copy-copy, paste-paste, no-waste."

---

## 1. Product Overview

**ccvv** is a lightweight, background utility designed for developers, technical writers, and power users. It acts as an intelligent clipboard sanitizer and formatter. By utilizing a rapid "double-tap" of standard copy and paste shortcuts, users can instantly strip unwanted artifacts, normalize text, and intelligently format data without interrupting their workflow.

---

## 2. User Interface & Experience

To balance the need for an unobtrusive background tool with the user's need for system status visibility, `ccvv` operates primarily through a System Tray / Menu Bar icon.

### Visual Feedback Loop (The "Blink")
The tray icon provides instant, tactile-like visual confirmation when a transformation occurs, training the user's muscle memory.
* **Idle (Paused):** `[cc]` — distinct icon shape (not just opacity) so the state is unambiguous in both Light and Dark Mode.
* **Active (Listening):** `[cc]` — solid / high contrast. A small dot badge indicates the number of active transformation categories (e.g., `•3` for three feature groups enabled).
* **Success Flash:** `[██]` — changes to a solid block for 300ms upon successful double-tap formatting.
* **Miss Indicator:** `[cc]` briefly shakes or pulses a muted color (~200ms) when a second copy lands *outside* the double-tap window, signaling that the timing was too slow.

### The HUD Toast (Optional)
A brief semi-transparent toast appears near the cursor showing a compressed summary of what changed (e.g., "Stripped 2 tracking params, unwrapped 8 lines → plain text"). Auto-dismissed after ~1.5s.

**Confidence Mode:** The toast is enabled by default for the first 50 transformations (training period), then automatically fades to icon-only feedback. The transition is gradual — toast duration shortens from 1.5s → 1s → 0.5s over time before disappearing. Can be permanently toggled in Preferences.

### Tray Menu Options
Clicking the icon reveals a minimal menu:
* **Pause / Resume:** Temporarily bypasses the listener when the user *wants* to keep rich formatting.
* **History:** Access the local clipboard history (with search). Presented as a scrollable plain list — not a mini-app. Each entry shows a truncated preview, its auto-detected type tag (URL, code, prose, table, JSON), and a timestamp. Click to restore to clipboard.
* **Preferences:** Toggle specific smart features.
* **Quit**

### Preferences Surface
Preferences are a single-pane list of labeled on/off toggles grouped by feature category, with a link to the config file for advanced settings. No tabs, no nested panels, no multi-page wizard. One screen, scroll if needed.

### The "Invisible Default"
For hardcore minimalists, the settings will include an option to "Hide Icon." If hidden, success feedback is delivered via:
* **macOS:** Force Touch trackpad haptic tap (where available), falling back to a brief screen-edge flash.
* **Linux/Windows:** Brief translucent screen-edge flash (~200ms), similar to macOS's screen recording indicator.
* Audio is explicitly avoided — it's too disruptive for open offices and screen-shares.

### First-Run Onboarding
On first launch, a single non-modal overlay appears:

> *Copy something, then tap C again within a beat. Watch the icon flash. That's it.*

The overlay includes a "Try it now" prompt. Once the user completes one successful double-tap (or dismisses), the overlay disappears permanently.

---

## 3. Core Interactions

### Clipboard Hooking Strategy
`ccvv` hooks the **OS-level clipboard** (pasteboard on macOS, clipboard manager on Linux/Windows), *not* individual key events. The double-tap detection listens for two clipboard-write events in rapid succession. This means it works correctly inside terminal emulators (`tmux`, `vim`, apps using `Ctrl+Shift+C`), remote desktop sessions, and any application that writes to the system clipboard — regardless of the specific keybinding used.

Because `ccvv` monitors clipboard writes rather than keystrokes, it does not conflict with `Ctrl+C` (SIGINT) in terminals, Emacs prefix chords, or IDE double-tap bindings. A terminal `Ctrl+C` that sends SIGINT does *not* write to the clipboard and is therefore invisible to `ccvv`.

### Platform Capture Constraints
These are architectural requirements, not implementation details — they determine whether the product can work on each platform.

* **macOS:** Requires Accessibility permission (`AXIsProcessTrusted`). The app prompts on first launch and links directly to System Settings. Clipboard monitoring uses `NSPasteboard` change-count polling (no private API).
* **Linux (X11):** Clipboard monitoring via `XFixes` selection-change events. Global tray via `StatusNotifierItem` (D-Bus) with `XEmbed` fallback.
* **Linux (Wayland):** Wayland's security model restricts global clipboard snooping. On Wayland, `ccvv` operates in **manual mode**: the user triggers transformation via the tray menu, a keyboard shortcut registered through the compositor (e.g., `sway`/`Hyprland` config), or the CLI companion. The double-tap gesture is not available on Wayland compositors that don't expose clipboard-change events. This limitation is documented in the README and surfaced on first launch.
* **Windows:** Low-level clipboard listener via `AddClipboardFormatListener`. Tray icon via shell `NotifyIcon` API. No keyboard hooks needed.

### Double-Tap Mechanics
The detection window defaults to **300ms** (configurable from 150–500ms).

The strategy is **"let the first copy complete, post-process on the second"**:
1. First `Cmd+C` passes through immediately — the OS performs a normal copy. No latency is added to any single copy operation.
2. `ccvv` records the clipboard write timestamp and a hash of the content.
3. If a second clipboard write arrives within the detection window:
   * **Same content** (user re-copied the same selection): this is a deliberate double-tap. Apply sanitization.
   * **Different content** (user selected something new): both copies are left raw. The second becomes the new baseline.
4. If no second write arrives, nothing happens. The copy was a normal single copy.

This means `ccvv` **never delays, blocks, or intercepts** the first copy. Transformation is always a post-processing step after the second write.

**Adaptive timing:** During the first day of use, `ccvv` passively records the interval of confirmed double-taps to learn the user's natural cadence. After ~20 samples, it sets a personalized threshold (clamped to the 150–500ms range). The user can override this with a manual slider in Preferences or a fixed value in the config file.

### Action 1: The Smart Copy (`Cmd/Ctrl + C, C`)
* **Behavior:** After detecting a valid double-tap, applies active sanitization rules and overwrites the clipboard with the cleaned version.
* **Result:** Triggers the visual Success Flash `[██]` and (if enabled) the HUD toast.

### Action 2: The Smart Paste (`Cmd/Ctrl + V, V`)
* **Behavior:** Detects the active destination window and applies context-aware formatting *immediately before* pasting.
* **Detection method:** Reads the frontmost application's bundle identifier (macOS), window class (Linux/X11), or process name (Windows).
* **Fallback:** When detection fails (Electron apps reporting a generic name, unknown window class, Wayland without focus info), `ccvv` **applies no context-specific transformation** and pastes the clipboard as-is. Context-aware formatting is strictly opt-in when the target is ambiguous. Users can define explicit app-name → behavior mappings in the config file.
* **Example:** Flattening a multi-line JSON string into a single line when pasting into a recognized terminal emulator (Terminal.app, iTerm2, Alacritty, Windows Terminal, etc.).

### Action 3: The "Paste As" Chooser (`Shift` + double-tap paste)
Holding `Shift` during the double-tap paste opens a small floating palette near the cursor with format options:
* Paste as plain text
* Paste as Markdown table
* Paste as fenced code block (with language detection)
* Paste as single line
* Paste as raw (undo all transformations)

The palette is keyboard-navigable (arrow keys + Enter) and dismisses on `Escape` or click-away.

### Bypass Modifier
Holding `Option` (macOS) / `Alt` (Windows/Linux) during a copy suppresses `ccvv` for that single operation, even if it would otherwise trigger as a double-tap. This provides an instant escape hatch without needing to Pause/Resume via the tray.

---

## 4. Transform Engine

Transformations are applied as an **ordered pipeline**. Each stage receives the output of the previous stage, and stages can be individually toggled.

### Pipeline Order (Smart Copy)
```
1. Rich text → plain text (Blind Paste)
2. Unicode & encoding normalization
3. Whitespace & line break cleanup
4. Agent artifact stripping (⏺, ANSI, zero-width)
5. Structural detection (JSON, table, code)
6. URL cleaning
7. Auto-wrapper (backtick wrapping)
8. User-defined regex rules
```

### Pipeline Order (Smart Paste)
```
1. All Smart Copy stages (if not already applied)
2. Context detection (identify target app)
3. Context-specific formatting (JSON flatten, etc.)
```

### Design Constraints
* **Idempotency:** Running the pipeline twice on the same input must produce the same output. No stage should introduce artifacts that a later stage would want to strip.
* **Structural detection is conservative:** If the engine isn't confident the content is JSON/table/code, it skips the structural stage and passes raw text through. False negatives are preferable to false positives.
* **Unwrap safeguards:** The line-break unwrapper uses heuristic signals (line length variance, sentence-ending punctuation, indentation patterns) to distinguish hard-wrapped prose from intentional line breaks. Content that resembles log output, poetry, ASCII art, or pre-formatted tables is left untouched. When confidence is below threshold, the unwrapper is skipped entirely for that block.

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

---

## 5. The "Smart" Feature Set

These features are enabled by default but can be toggled individually in Preferences or the config file.

### The "Blind" Paste (Core Pillar)
Guarantees that `Cmd+C, C` actively strips all rich-text MIME types (HTML, CSS, RTF) from the clipboard, leaving only `text/plain`. It works universally, even in applications that do not natively support a "paste without formatting" shortcut.

### Whitespace & Line Break Manager
Fixes the most common layout issues when copying from PDFs, emails, or poorly formatted code.
* **Strip Whitespace:** Automatically removes extraneous trailing and leading whitespace from individual lines.
* **Unwrap Hard Line Breaks:** Detects and rejoins lines that were hard-wrapped (common in PDFs or legacy email clients) back into naturally flowing paragraphs. Uses line-length variance, sentence-ending punctuation, and indentation as heuristic signals. Skips blocks that resemble log output, poetry, ASCII tables, or pre-formatted text.
* **Preserve Structure:** Intelligently ignores the unwrapper to keep deliberate paragraph breaks (double newlines), list items (`-`, `*`, `1.`), and fenced code blocks completely intact.

### Unicode & Encoding Normalization
Silently fixes the invisible characters that break grep patterns, SQL queries, and config files.
* **Smart Quotes → Straight Quotes:** `"` `"` → `"`, `'` `'` → `'`.
* **Typographic Dashes:** `—` (em-dash) → `--`, `–` (en-dash) → `-` (configurable: some users prefer preserving em-dashes).
* **Invisible Characters:** Strips non-breaking spaces (`\u00a0`), zero-width spaces (`\u200b`), zero-width joiners/non-joiners, byte-order marks, and other Unicode control characters that cause silent failures.
* **Encoding Repair:** Detects common mojibake patterns (Latin-1 interpreted as UTF-8) and attempts recovery. Falls back to leaving content unchanged if confidence is low.

### Smart Strip (URLs)
Cleans tracking noise from URLs while preserving usability.
* **Default behavior:** Strips tracking parameters but **preserves the scheme** (`https://`) and subdomain — the result is still a valid, clickable URL.
* **Aggressive mode** (opt-in via config): Also strips `https://` and `www.` for display-only contexts.
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

### Table-to-Markdown
When the clipboard contains tabular data (HTML `<table>`, TSV from Excel/Google Sheets, CSV), automatically converts it into a pipe-delimited Markdown table.
* **Delimiter detection:** Tries tab first (most common from spreadsheets), then comma, then semicolon. Respects quoted fields (RFC 4180).
* **Header inference:** First row is treated as a header if it contains distinct, non-numeric values. Otherwise a generic `Col 1 | Col 2 | ...` header is generated.
* **Column alignment:** Right-aligns columns where >80% of values are numeric; left-aligns everything else.
* **Multi-line cells:** Collapsed to a single line (newlines replaced with `<br>` for Markdown compatibility).

### Code Fence Detection
When the clipboard contains multi-line content that looks like code (based on indentation patterns, syntax keywords, shebang lines, or the source application being a known IDE/terminal), wraps it in a fenced code block with a best-guess language identifier:
~~~
```python
def hello():
    print("world")
```
~~~
Language detection uses a lightweight heuristic (file extension if available from source metadata, keyword frequency otherwise). Falls back to bare ``` fences if confidence is low.

### Platform & Agent-Specific Smarts
Targeted sanitization for modern AI and CLI workflows.
* **macOS Rich Text Inference:** When copying rich text on macOS, it infers inline code by detecting monospace fonts or specific text background colors, converting them into plain-text markdown backticks (`` ` ``) before stripping the rich-text payload.
* **Claude Code / Agent Cleaner:** Explicitly targets and strips CLI agent artifacts, such as the `⏺` markers from Claude Code, ANSI escape codes (terminal colors), and hidden zero-width characters that cause silent syntax errors.

### JSON Flattener
Context-aware JSON formatting.
* **On Copy:** Cleans and *prettifies* messy JSON for readability.
* **On Paste (Terminal):** *Flattens* the JSON into a single minified line to prevent multi-line execution errors in CLI tools.

### Auto-Wrapper
Identifies technical strings and wraps them in backticks (`` ` ``) for standard Markdown formatting.
* Targets absolute/relative file paths, recognized file extensions, and shell prompts (e.g., lines starting with `$ ` or `> `).

---

## 6. User Rules (`.ccvv` Config)

Power users can define custom sanitization rules in a config file (TOML format). Rules are applied as the last stage of the transform pipeline (after all built-in features).

**Config file location** (XDG-compliant):
* **macOS/Linux:** `~/.config/ccvv/config.toml` (also reads `~/.ccvv` for convenience)
* **Windows:** `%APPDATA%\ccvv\config.toml`

```toml
# ~/.config/ccvv/config.toml

[settings]
double_tap_window_ms = 300   # 150–500, or "auto" for adaptive
hud_toast = true
url_strip_scheme = false     # true = aggressive mode (strip https://)

[settings.em_dash]
replace = "--"               # or "—" to preserve

# App-specific Smart Paste overrides
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

---

## 7. Compatibility & Conflicts

### Known Non-Conflicts
Because `ccvv` monitors clipboard writes (not keystrokes), the following do **not** conflict:
* `Ctrl+C` (SIGINT) in terminals — does not write to clipboard
* Emacs `C-c` prefix chord — does not write to clipboard
* IDE "copy reference" on double `Cmd+C` — these typically write *different* content on the second press, which `ccvv` treats as a new baseline (no transform)
* Vim yank in terminal — writes to clipboard like any other copy; double-yank in rapid succession would trigger `ccvv`

### App Exclusion List
Some apps should never be intercepted. The config supports a global exclusion list:
```toml
[exclusions]
apps = [
  "com.vmware.fusion",      # remote desktop — let guest OS handle clipboard
  "com.parallels.desktop",
  "org.keepassxc.keepassxc", # password managers — never touch
]
```
When the frontmost app matches an exclusion, `ccvv` ignores all clipboard writes regardless of double-tap timing.

### Terminal Clipboard Buffers
Some terminal emulators (`tmux`, `screen`, `vim`) maintain internal buffers separate from the system clipboard. `ccvv` only monitors the system clipboard. If a terminal app is configured to sync its internal buffer *to* the system clipboard, that sync event is visible to `ccvv` and will be treated as a normal clipboard write.

---

## 8. Privacy & History

Because `ccvv` forcefully overwrites the clipboard, users need a safety net if they accidentally destroy text they intended to keep raw.
* **Local-Only Database:** SQLite running strictly on the user's machine. Zero cloud telemetry. (SQLite over flat JSON — concurrent clipboard reads/writes would degrade a JSON file, and SQLite enables indexed search over history.)
* **Undo Stack:** Stores the last 50 raw clipboard items, allowing users to quickly revert a "Smart Copy" if the tool's heuristics get too aggressive.
* **Search & Categories:** History entries are automatically tagged by type (URL, code, prose, table, JSON) based on content heuristics. The History panel supports substring search and category filtering.

---

## 9. CLI Companion

Beyond the background daemon, `ccvv` ships a CLI that exposes the same transformation engine for scripting and terminal workflows:

```bash
# Pipe clipboard through ccvv
pbpaste | ccvv | pbcopy

# Apply specific transforms
ccvv --strip-urls < input.txt
ccvv --flatten-json < api-response.json
cat README.md | ccvv --unwrap > clean.md

# Use a named profile
ccvv --profile terminal < input.json

# Preview transforms without modifying clipboard
ccvv preview                     # shows: input → output diff + rules fired
ccvv preview --clipboard         # preview what double-tap would produce
ccvv preview < input.txt         # preview on arbitrary input

# History access
ccvv history                     # list recent entries
ccvv history 5                   # retrieve 5th-most-recent raw entry
ccvv history --search "api"
ccvv history --type json         # filter by auto-detected category

# Diagnostics
ccvv doctor                      # print platform info, permissions status,
                                 # active config, detected conflicts,
                                 # clipboard API availability (X11/Wayland)

# Config validation
ccvv validate                    # check config syntax + regex validity
```

The CLI makes `ccvv` composable with standard Unix tools, shell aliases, and CI scripts. The transformation logic is a shared library between the daemon and the CLI — not a separate implementation.

---

## 10. Platforms & Installation

`ccvv` is a natively compiled binary with low memory overhead, supporting macOS, Windows, and Linux.

### Performance Budget
* **Binary size:** < 5 MB (single statically-linked binary, no runtime dependencies).
* **Memory (RSS):** < 15 MB idle, < 25 MB during transformation. History SQLite DB is memory-mapped on demand, not preloaded.
* **CPU:** Zero polling loops. Clipboard monitoring uses OS-provided event/notification APIs (`NSPasteboard` change-count check on macOS is timer-based at ≤2 Hz; `XFixes` and `AddClipboardFormatListener` are event-driven). The daemon should register **< 1 wakeup/second** when idle.
* **Startup:** < 200ms to tray-icon-visible on a cold start.

### Build Targets
* **macOS:** Swift binary. Uses AppKit for tray/pasteboard and Accessibility framework. Distributed as a universal binary (arm64 + x86_64).
* **Windows:** Native Win32 binary (Rust or C++). Clipboard via `AddClipboardFormatListener`, tray via `Shell_NotifyIcon`.
* **Linux (X11):** Binary with `XFixes` for clipboard events, `StatusNotifierItem` for tray. Minimal system library deps (no GTK/Qt runtime required for the daemon; tray may optionally link `libappindicator`).
* **Linux (Wayland):** Same binary, reduced feature set (see Section 3 — Platform Capture Constraints). CLI companion has full functionality.

### Installation Strategy (One-Liners)
* **macOS:** `brew install ccvv`
* **Windows:** `winget install ccvv`
* **Linux:** `sudo snap install ccvv`
