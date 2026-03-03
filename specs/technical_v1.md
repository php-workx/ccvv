# Technical Specification: ccvv v1.1

**Status:** Draft
**Scope:** macOS-deep v1 — Rust core library, Swift macOS shell, CLI companion
**Deferred:** Smart Paste (v2), Linux/Windows daemon (v2)
**Reference:** [`specs/functional.md`](functional_pre_v2.md)
**Previous:** [`specs/technical_v1.0.md`](technical_v1.0.md)

---

## 1. Architecture Overview

ccvv v1 is composed of three compiled artifacts sharing a single transform engine:

```
┌──────────────────────────────────────────────────────┐
│                    ccvv-lib (Rust)                    │
│                                                      │
│  ┌────────────┐  ┌──────────┐  ┌──────────────────┐ │
│  │  Transform  │  │  Config  │  │  History (SQLite) │ │
│  │  Pipeline   │  │  Parser  │  │                   │ │
│  └────────────┘  └──────────┘  └──────────────────┘ │
│                                                      │
│  ┌──────────────────────────────────────────────────┐│
│  │              C FFI Surface (ffi.rs)              ││
│  └──────────────────────────────────────────────────┘│
└────────────────┬─────────────────────┬───────────────┘
                 │ staticlib           │ direct link
        ┌────────┴────────┐   ┌───────┴────────┐
        │  macOS App      │   │  ccvv CLI      │
        │  (Swift/AppKit) │   │  (Rust/clap)   │
        └─────────────────┘   └────────────────┘
```

- **ccvv-lib**: Rust library crate. Contains all text transformation logic, TOML config parsing, SQLite history, and content classification. Compiled as `staticlib` (for linking into Swift) and `cdylib` (for future dynamic loading). Exposes a C-ABI FFI surface.
- **macOS App**: Swift/AppKit menu bar application. Handles platform-specific concerns: `NSPasteboard` access, `NSAttributedString` rich-text extraction, `CGEventTap` keyboard monitoring, `NSStatusItem` tray icon, and all UI. Calls ccvv-lib via C FFI for text transformation, config, and history.
- **ccvv CLI**: Rust binary crate. Links ccvv-lib directly (native Rust API, no FFI). Provides stdin/stdout pipe transformation, history access, diagnostics, and config validation.

### v1 Platform Scope

| Platform | Artifact | Status              |
|----------|----------|---------------------|
| macOS (arm64 + x86_64) | App bundle + CLI binary | Full implementation |
| Linux | Python script (`linux/ccvv`) | No implementation in v1 |
| Windows | PowerShell script (`windows/ccvv.ps1`) | No implementation in v1 |

---

## 2. Threat Model & Security Architecture

### 2.1 Assets

| Asset | Location                                                | Sensitivity | Threat |
|-------|---------------------------------------------------------|-------------|--------|
| Raw clipboard content | Memory (transient), History DB (persistent if opted in) | **Critical** — may contain passwords, tokens, secrets, PII | Exfiltration, unintended persistence, backup leakage |
| Cleaned clipboard content | System pasteboard, History DB                           | **High** — still user data, may retain partial secrets | Same-user process snooping, clipboard manager leakage |
| History database | `~/.ccvv/history.db`                                    | **High** — aggregates clipboard over time | Same-user read, backup propagation, index exposure |
| Config file | `~/.ccvv/config.toml`                                   | **High** — regex rules can silently rewrite clipboard content | Integrity attack: modified rules rewrite crypto addresses, URLs, commands |
| Timing data | `~/.ccvv/timing.json`                                   | **Low** — cadence samples, no content | Minimal |
| Diagnostic output | `ccvv doctor` stdout                                    | **Medium** — prints paths, feature states, permission status | Information disclosure to co-located observer |

### 2.2 Adversary Model

| Adversary | Capability | Relevant attacks |
|-----------|------------|------------------|
| Same-user malware | Read/write any user-owned file, read system pasteboard, install launch agents | Read history DB, modify config to rewrite pasted content, inject regex rules |
| Supply-chain compromise | Malicious dependency update, tampered Homebrew formula | Exfiltrate clipboard via injected network call, backdoor transform stage. Inherits Accessibility permission — can keylog and synthesize input. |
| Crafted clipboard content | Adversarial text placed on pasteboard by a malicious app or website | Trigger pathological parsing (deeply nested JSON, regex edge cases), exploit parser bugs, produce subtly corrupted output (homoglyph characters surviving the pipeline) |
| Backup/sync leakage | Time Machine, iCloud Drive sync, enterprise MDM backup | Copies of history DB propagate to external drives, NAS, cloud |
| Curious coworker | Physical access to unlocked machine or backups | Browse history DB with sqlite3, read config |
| Accidental misconfiguration | User creates partial config overriding safe defaults | Wipes app exclusion list, disables sensitive-content filter |

**Clipboard content is untrusted input.** ccvv processes text from `NSPasteboard` which can be set by any application. The transform pipeline MUST treat all input as potentially adversarial: no unbounded allocations, no assumptions about well-formedness, and graceful degradation (pass-through) on any unexpected input. See §5.3 for size limits and §14.7 for fuzz testing.

### 2.3 Trust Boundary — Accessibility Scope

ccvv requires macOS Accessibility permission to install a `CGEventTap`. **This is the most dangerous permission in the application.** On macOS, Accessibility permission is binary — there is no way to grant "only CGEventTap" — and it confers the ability to:

- Read *all* keyboard input system-wide (not just Cmd+C).
- Synthesize keyboard and mouse events.
- Read the contents of any text field in any application via the AX API.
- Observe and manipulate the UI hierarchy of all running applications.

ccvv uses none of these capabilities beyond listen-only CGEventTap for Cmd+C cadence detection. However, **if ccvv's binary is compromised** (via supply-chain attack, config injection escalation, or a memory safety bug in a C dependency), the attacker inherits all of these capabilities. This makes ccvv a high-value privilege escalation target. Users deserve to know the trust they are extending.

The following constraints are **invariants** — the app MUST NOT violate them:

1. **ccvv only observes `kCGEventKeyDown` and `kCGEventKeyUp` events.** No mouse events, no other event types.
2. **ccvv only inspects the keycode and modifier flags** of each event to detect Cmd+C cadence. It does NOT log, persist, or transmit individual keystrokes.
3. **ccvv never records which application is frontmost** except to check the app exclusion list (`bundleIdentifier` compared against a static set). It does NOT store or correlate app identity with clipboard content.
4. **The event tap is passive (listen-only).** ccvv never modifies, blocks, or delays keyboard events.
5. **If Accessibility permission is denied or revoked**, ccvv degrades to a menu-bar-only manual mode (user clicks "Clean" in the menu). It does NOT re-prompt or escalate.

**Privilege justification:** CGEventTap is required because macOS provides no lighter-weight API for detecting rapid repeated keystrokes. NSPasteboard change-count polling alone cannot reliably distinguish a double-tap Cmd+C within a 300ms window (polling at 2 Hz gives 500ms granularity; higher polling rates exceed the idle CPU budget). The functional spec's double-tap UX requires keystroke cadence detection, which necessitates Accessibility.

**Accessibility vs Input Monitoring (macOS Ventura+):** On macOS 13 and later, Apple exposes **Input Monitoring** as a distinct TCC (Transparency, Consent, and Control) privacy category. CGEventTap for keyboard monitoring may require Input Monitoring consent in addition to (or instead of) Accessibility consent, depending on how the tap is created and what events it observes. This MUST be tested on clean macOS 13/14/15 installs where neither permission is pre-granted. The app must handle all possible states:

| Accessibility | Input Monitoring | Behavior |
|---------------|-----------------|----------|
| Granted | Granted (or not required) | Full functionality — double-tap detection active |
| Granted | Denied | Test-dependent — if CGEventTap still receives events, full functionality. If not, fall back to manual mode. |
| Denied | Any | Manual mode |

**Important:** TCC permissions are granted at runtime by the user through System Settings, NOT by entitlements in the code signature. Entitlements control Hardened Runtime restrictions (e.g., `com.apple.security.cs.disable-library-validation`), not TCC consent. The entitlements file (§9.4) does NOT grant Accessibility or Input Monitoring access — it only configures Hardened Runtime.

**v1 scope clarification:** The CGEventTap approach detects `Cmd+C` keystrokes only. It does NOT detect clipboard writes from: right-click → Copy, Edit menu → Copy, programmatic copies, tmux yank, or any non-keyboard copy mechanism. The functional spec's claim that ccvv works with "any application that writes to the system clipboard regardless of keybinding" is **incorrect** under this architecture and must be corrected. v1 supports `Cmd+C` double-tap only.

**Fallback behavior when permission denied or revoked:**
- Menu bar icon shows with a warning badge.
- Menu includes a "Clean Clipboard Now" manual action.
- The "Open System Settings" button is available but no aggressive re-prompting.
- All transform and history functionality works normally — only auto-detection of double-tap is disabled.
- **Periodic re-check:** Every 30 seconds, call `AXIsProcessTrusted()` (without the prompt flag). If permission is lost at runtime, update the icon to error state, show a one-time notification with "Open System Settings" deep link, and disable the event tap. If permission is re-granted, re-enable the tap and restore the icon.

**Runtime self-integrity (defense-in-depth):** On launch, verify the application's own code signature via `SecStaticCodeCheckValidity`. If the signature is invalid or missing, log a warning. This does not prevent a sophisticated attacker (who could patch out the check) but raises the bar against casual binary modification.

### 2.4 Privacy Invariants

The following are **hard requirements** — they override any convenience or feature consideration:

1. **ccvv MUST NOT make network connections.** Enforced via Hardened Runtime and dependency auditing (see §2.6).
2. **ccvv MUST NOT persist raw clipboard content by default.** The history database stores only a SHA-256 hash of raw text (for undo lookup) and the cleaned text. Raw text storage requires explicit opt-in (`settings.history_store_raw = true`). See §7.
3. **ccvv MUST NOT inspect clipboard content from excluded apps.** When the frontmost app's `bundleIdentifier` matches the exclusion list, the double-tap handler returns immediately without reading `NSPasteboard`.
4. **ccvv MUST NOT process content that matches secret-detection patterns** unless the user explicitly disables the filter. See §5.5.
5. **ccvv MUST exclude its data directory from Time Machine and Spotlight indexing.** See §7.5.

### 2.5 Crash Consistency Invariants

After any ccvv operation (including crashes, SIGKILL, OOM jetsam), exactly one of the following holds:

- **(a)** The clipboard contains the cleaned text AND the history database contains the corresponding entry (committed state).
- **(b)** The clipboard contains the original raw text AND the history database is unchanged (no-op state — crash before commit).

It is NEVER the case that cleaned text is on the clipboard but the history entry (needed for undo) is missing. See §9.2 for the two-phase commit protocol.

### 2.6 Network Isolation Enforcement

The claim "zero network access" is enforced with technical controls, not just policy:

1. **No network-capable code:** ccvv does not use App Sandbox (which would conflict with CGEventTap and Accessibility TCC requirements for Developer ID distribution). Instead, network isolation is enforced by ensuring no code in the binary makes network calls. This is verified by `cargo deny` (banning network-capable crates) and runtime assertion (see below).
2. **Dependency audit:** `cargo deny` checks that no dependency in the tree has features enabling network I/O (`reqwest`, `hyper`, `tokio-net`, etc.). This is enforced in CI.
3. **Runtime assertion (debug builds):** A debug-only check confirms no socket file descriptors are open after initialization.
4. **Known platform-initiated connections:** macOS may make OCSP calls during code signature verification at launch. These are initiated by the OS, not by ccvv, and occur outside the process sandbox. Notarization checks are also OS-initiated. These are documented but not counted as violations.
5. **Homebrew analytics:** `brew install ccvv` transmits installation analytics to Homebrew's servers unless the user has set `HOMEBREW_NO_ANALYTICS=1`. This is Homebrew's behavior, not ccvv's. Documented in installation instructions.

### 2.7 DoS Model

| Attack vector | Mitigation |
|---------------|------------|
| Multi-MB clipboard content (logs, JSON dumps, base64 blobs) | Input size ceiling: 1 MB (configurable). Content above ceiling passes through untransformed with a diagnostic entry. See §5.3. |
| Rapid repeated triggers (automation, stuck keys) | Rate limit: max 1 transform per 100ms. Subsequent triggers within the cooldown are dropped. |
| Pathological regex in user rules | `regex` crate guarantees linear-time matching. Additionally, per-rule timeout of 50ms; rule is skipped and logged if exceeded. Max 50 user rules. |
| Deep JSON nesting | `serde_json` default recursion limit (128). Content exceeding limit passes through untransformed. |
| Massive CSV/table → Markdown expansion | Output size limit: 2× input size. If Markdown table output exceeds this, pass through untransformed. |
| Memory pressure in menu-bar app | Per-transform memory budget: 10× input size. Monitored via allocation tracking; if exceeded, abort transform and pass through. |

---

## 3. Repository Layout

```
ccvv/
├── core/                              # Rust workspace
│   ├── Cargo.toml                     # [workspace] members
│   ├── deny.toml                      # cargo-deny config (license, advisory, ban)
│   ├── ccvv-lib/
│   │   ├── Cargo.toml                 # lib crate: staticlib + cdylib
│   │   ├── cbindgen.toml              # C header generation config
│   │   └── src/
│   │       ├── lib.rs                 # Public Rust API, re-exports
│   │       ├── ffi.rs                 # extern "C" functions
│   │       ├── error.rs               # CcvvError enum
│   │       ├── config.rs              # TOML parsing, resolution, validation
│   │       ├── pipeline.rs            # Pipeline struct, stage orchestration
│   │       ├── classify.rs            # Content type heuristics
│   │       ├── history.rs             # SQLite operations
│   │       ├── secrets.rs             # Secret-pattern detection
│   │       └── transforms/
│   │           ├── mod.rs             # Transform trait, ContentType, TransformContext
│   │           ├── normalize.rs       # Stage 2: Unicode & encoding
│   │           ├── whitespace.rs      # Stage 3: Line cleanup & unwrap
│   │           ├── agent.rs           # Stage 4: Artifact stripping
│   │           ├── structural.rs      # Stage 5: JSON, table, code fence
│   │           ├── url.rs             # Stage 6: URL cleaning
│   │           ├── autowrap.rs        # Stage 7: Backtick wrapping
│   │           └── userrules.rs       # Stage 8: User regex rules
│   └── ccvv-cli/
│       ├── Cargo.toml                 # bin crate
│       └── src/
│           └── main.rs               # clap argument parser, subcommands
├── mac/
│   ├── main.swift                     # macOS app (refactored)
│   ├── ccvv-bridge.h                  # Generated C header
│   ├── ccvv.entitlements              # Hardened Runtime entitlements
│   ├── build.sh                       # Build script (Rust + Swift)
│   ├── brew-local-test.sh             # Homebrew testing
│   └── Info.plist                     # App bundle metadata
```

---

## 4. Rust Workspace

### 4.1 Workspace Cargo.toml

```toml
[workspace]
members = ["ccvv-lib", "ccvv-cli"]
resolver = "2"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
toml = "0.8"
regex = "1"
rusqlite = { version = "0.32", features = ["bundled"] }
url = "2"
serde_json = "1"
unicode-normalization = "0.1"
thiserror = "2"
sha2 = "0.10"
zeroize = { version = "1", features = ["derive"] }
toml_edit = "0.22"
```

### 4.2 ccvv-lib Cargo.toml

```toml
[package]
name = "ccvv-lib"
version = "1.1.0"
edition = "2021"

[lib]
name = "ccvv_lib"
crate-type = ["staticlib", "lib"]

[dependencies]
serde.workspace = true
toml.workspace = true
regex.workspace = true
rusqlite.workspace = true
url.workspace = true
serde_json.workspace = true
unicode-normalization.workspace = true
thiserror.workspace = true
sha2.workspace = true
zeroize.workspace = true
toml_edit.workspace = true

[build-dependencies]
cbindgen = "0.27"
```

The `lib` crate type enables normal Rust linking for ccvv-cli. The `staticlib` produces `libccvv_lib.a` for Swift. `cdylib` is omitted in v1 — it adds build time for an unused artifact. Add it when a dynamic-linking use case materializes.

### 4.3 ccvv-cli Cargo.toml

```toml
[package]
name = "ccvv-cli"
version = "1.1.0"
edition = "2021"

[[bin]]
name = "ccvv"
path = "src/main.rs"

[dependencies]
ccvv-lib = { path = "../ccvv-lib" }
clap = { version = "4", features = ["derive"] }
similar = "2"
```

### 4.4 Dependency Rationale

| Crate | Purpose | Why this crate |
|-------|---------|----------------|
| `serde` + `toml` | Config deserialization | De-facto standard; derives eliminate boilerplate |
| `regex` | ANSI stripping, URL detection, user rules, token heuristics | Mature, performant, guaranteed linear-time (no ReDoS) |
| `rusqlite` (bundled) | History database | `bundled` embeds SQLite — zero system dependency, no version skew |
| `url` | URL parsing in the URL cleaner stage | RFC-compliant parser; avoids hand-rolled regex for URLs |
| `serde_json` | JSON detection and prettification | Validates JSON before attempting structural transforms |
| `unicode-normalization` | NFC normalization | Unicode Consortium reference implementation |
| `thiserror` | Error type derivation | Zero-cost, derive macro only |
| `clap` (derive) | CLI argument parsing | Standard, derive-based, auto-generates `--help` |
| `similar` | Text diffing for `ccvv preview` | Patience diff algorithm, line-level granularity |
| `sha2` | Content hashing for history dedup and raw-text lookup | NIST standard, pure Rust, no FFI |
| `zeroize` | Secure memory clearing for pruned history entries | Standard crate for sensitive data handling; zeroes memory on drop rather than leaving it for the allocator |
| `toml_edit` | Preserve-formatting config writes from Preferences UI | Round-trips TOML without losing comments or formatting; `toml` crate loses both on deserialization |

### 4.5 Dependency Policy

`deny.toml` (cargo-deny configuration):

```toml
[advisories]
vulnerability = "deny"
unmaintained = "warn"

[licenses]
allow = ["MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib"]
unlicensed = "deny"

[bans]
# No network-capable crates allowed in the dependency tree
deny = [
    { name = "reqwest" },
    { name = "hyper" },
    { name = "tokio" },
    { name = "async-std" },
    { name = "surf" },
    { name = "ureq" },
    { name = "attohttpc" },
    { name = "curl" },
    # SECURITY: fancy-regex supports backreferences and lookahead, which are
    # vulnerable to ReDoS. The `regex` crate's linear-time guarantee is a
    # security property that prevents user-defined rules from causing DoS.
    # Do NOT replace `regex` with `fancy-regex` anywhere in the codebase.
    { name = "fancy-regex" },
]

[sources]
unknown-registry = "deny"
unknown-git = "deny"
```

This is enforced in CI via `cargo deny check`.

**Additional CI checks:**
- `cargo audit` — checks for known vulnerabilities in the dependency tree (via RustSec advisory database). Run on every CI build.
- `Cargo.lock` is committed to the repository. Dependency versions are pinned. Updates require explicit `cargo update` and review.

---

## 5. Transform Engine

### 5.1 Transform Trait

```rust
/// Metadata that flows through the pipeline alongside the text.
/// Stages read and annotate this to communicate downstream.
#[derive(Debug, Clone, Default)]
pub struct TransformContext {
    pub content_type: Option<ContentType>,
    pub rules_fired: Vec<RuleFired>,
    pub profile: Option<String>,
    pub input_size_bytes: usize,
    pub skipped_sensitive: bool,
    pub skipped_oversize: bool,
}

#[derive(Debug, Clone)]
pub struct RuleFired {
    pub stage: &'static str,
    pub description: String,
    pub chars_changed: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    Url,
    Code,
    Prose,
    Table,
    Json,
    Mixed,
}

/// Every pipeline stage implements this trait.
pub trait Transform: Send + Sync {
    /// Human-readable name used for config toggles and diagnostics.
    fn name(&self) -> &'static str;

    /// Apply the transformation. Returns modified text.
    /// CONTRACT: apply(apply(input)) == apply(input) — must be idempotent.
    fn apply(&self, input: &str, ctx: &mut TransformContext) -> String;
}
```

Design decisions:
- `TransformContext` is mutable and flows forward. Stage 4 (agent stripping) can annotate that ANSI codes were found, which Stage 5 (structural detection) uses as a signal the text originated from a terminal.
- `skipped_sensitive` and `skipped_oversize` flags allow the UI to inform the user why content was not transformed.
- Each stage returns a new `String`. No shared mutable buffer.
- `Send + Sync` because the FFI layer may invoke from a non-Rust thread.

### 5.2 Pipeline Orchestrator

```rust
pub struct Pipeline {
    stages: Vec<Box<dyn Transform>>,
    max_input_bytes: usize,        // default: 1_048_576 (1 MB)
    max_output_ratio: f64,         // default: 2.0
    sensitive_filter: SecretFilter, // see §5.5
}

impl Pipeline {
    /// Build from a resolved config. Stages are instantiated in
    /// canonical order, but only included if enabled.
    pub fn from_config(config: &ResolvedConfig) -> Self;

    /// Run all stages. Returns cleaned text and context.
    /// If input exceeds max_input_bytes, returns input unchanged
    /// with ctx.skipped_oversize = true.
    /// If input matches sensitive patterns and filter is enabled,
    /// returns input unchanged with ctx.skipped_sensitive = true.
    pub fn run(&self, input: &str) -> (String, TransformContext);

    /// Run only named stages. Used by CLI --strip-urls, --unwrap, etc.
    pub fn run_selective(
        &self,
        input: &str,
        stage_names: &[&str],
    ) -> (String, TransformContext);
}
```

The pipeline enforces canonical stage ordering from the functional spec. Config can only toggle stages on/off, not reorder them. The ordering is:

| Order | Stage | Module | Config key |
|-------|-------|--------|------------|
| 2 | Unicode & encoding normalization | `normalize.rs` | `normalize_unicode` |
| 3 | Whitespace & line break cleanup | `whitespace.rs` | `whitespace_cleanup` |
| 4 | Agent artifact stripping | `agent.rs` | `agent_strip` |
| 5 | Structural detection (JSON, table, code) | `structural.rs` | `structural_detection` |
| 6 | URL cleaning | `url.rs` | `url_cleaning` |
| 7 | Auto-wrapper (backtick) — **opt-in, disabled by default** | `autowrap.rs` | `auto_wrapper` |
| 8 | User-defined regex rules | `userrules.rs` | `user_rules` |

Stage 1 (rich text → plain text) is macOS-specific and remains in Swift. The Rust pipeline receives text that has already been stripped to plain text with inline code markers inserted.

**Inter-stage output size check:** After each stage, if `output.len() > input.len() * max_output_ratio`, the pipeline aborts, returns the original input, and records a diagnostic entry. This prevents structural detection (e.g., table → Markdown) from producing runaway expansion.

### 5.3 Input Size Limits

| Limit | Default | Config key | Behavior when exceeded |
|-------|---------|------------|----------------------|
| Max input size | 1 MB | `settings.max_input_bytes` | Pass through unchanged, set `ctx.skipped_oversize = true` |
| Max output expansion ratio | 2.0× | (not configurable) | Abort transform, return original input |
| Max user rules | 50 | (not configurable) | Config validation rejects files with > 50 rules |
| Per-rule execution timeout | 50ms | (not configurable) | Rule is skipped, logged in `ctx.rules_fired` as timeout |

### 5.4 Stage Specifications

#### Stage 2: Unicode & Encoding Normalization (`normalize.rs`)

Character-by-character scan with lookup tables. No regex.

| Input | Output | Notes |
|-------|--------|-------|
| `\u{201C}` `\u{201D}` (curly double quotes) | `"` | Always |
| `\u{2018}` `\u{2019}` (curly single quotes) | `'` | Always |
| `\u{2014}` (em-dash) | Configurable: `"--"` (default) or preserve | `settings.em_dash.replace` |
| `\u{2013}` (en-dash) | `-` | Always |
| `\u{00A0}` (non-breaking space) | ` ` (regular space) | Always |
| `\u{200B}` (zero-width space) | `` (removed) | Always |
| `\u{200C}` (zero-width non-joiner) | `` (removed) | Always |
| `\u{200D}` (zero-width joiner) | `` (removed) | Always |
| `\u{FEFF}` (byte-order mark) | `` (removed) | Always |
| `\u{2022}` (bullet) | Already handled by Stage 3 | Skip here |

**Mojibake repair:** Scans for common Latin-1-as-UTF-8 double-encoding patterns (e.g., `Ã©` → `é`, `Ã¼` → `ü`). Uses a lookup table of the ~30 most common mojibake sequences. Applies repair only when confidence is high (≥3 matching patterns in the same **paragraph**). A "paragraph" is defined as text between blank lines (consistent with Stage 3's block accumulation). Falls back to leaving content unchanged. When repair triggers, a `RuleFired` entry is recorded with `stage = "normalize"` and `description = "repaired N mojibake sequences"` so the HUD toast shows "repaired encoding" and the user can catch false positives.

**Corruption risk:** This stage modifies exact bytes. Content where exact characters matter (base64, JWTs, cryptographic signatures, hashes) is protected by the sensitive content filter (§5.5) which runs before any stage. If the filter is disabled by the user, this stage may corrupt such content. This is documented in the config file comments and the `ccvv doctor` output.

**NFC normalization:** Applied via `unicode-normalization` crate as a final pass. Ensures composed characters (e.g., `é` as a single codepoint rather than `e` + combining acute accent).

#### Stage 3: Whitespace & Line Break Cleanup (`whitespace.rs`)

This is the largest and most nuanced module. It is a direct port of the existing Swift `ccvv()` function (`mac/main.swift` lines 11–209) with identical behavior.

**Algorithm:**

1. **Line normalization:** `\r\n` → `\n`, lone `\r` → `\n`.
2. **Terminal width detection:** Count the length of all raw lines > 40 characters. Find the most frequent length. If ≥ 3 lines share that length, treat it as the terminal width. Otherwise, terminal width is 0 (disabled).
3. **Per-line processing:**
   - Strip trailing whitespace (regex: `\s+$`).
   - Collapse terminal padding: interior runs of ≥ 3 spaces → single space.
   - Lines with > 20 leading spaces are treated as paragraph breaks.
4. **Block accumulation:** Lines are accumulated into paragraph blocks, delimited by:
   - Blank lines (explicit paragraph breaks).
   - Code fence markers (`` ``` ``).
   - Lines that begin a new list item.
   - "Implicit breaks" when terminal width is detected and a line is significantly shorter than the terminal width (< width − 5), signaling end-of-paragraph.
5. **Code fence handling:** Lines between matching `` ``` `` markers are collected verbatim. Fences are canonicalized to exactly `` ``` `` (no extra backticks). Content inside fences is dedented to the minimum indentation of the block.
6. **Bullet normalization:** `•`, `◦`, `▪` → `-`.  `⏺` at line start is stripped (overlaps with Stage 4 — intentional for idempotency).
7. **Paragraph compaction (`compactParagraph`):** Joins continuation lines within a paragraph block. A line is a continuation if:
   - It does not start a list item.
   - It does not start a code fence.
   - Its indentation does not change significantly from the previous line.
   - If the previous line ends with sentence-ending punctuation (`.`, `!`, `?`) and the current line starts with a capital letter, it is treated as a new sentence within the same paragraph (joined).
8. **Output assembly:** Blocks are joined with `\n\n`. Trailing whitespace is stripped from the final output.

**Unwrap safeguards:** The unwrapper skips a block entirely if:
- All lines have identical length (likely pre-formatted / ASCII table).
- Line-length standard deviation is very low AND lines don't end with sentence punctuation (likely log output or columnar data).
- More than 50% of lines start with a non-alphabetic character (likely code or data).

**Data structure:**
```rust
struct ParagraphLine {
    text: String,
    indent: usize,
}
```

#### Stage 4: Agent Artifact Stripping (`agent.rs`)

| Pattern | Action |
|---------|--------|
| `⏺` at line start (with optional trailing whitespace) | Remove character and collapse whitespace |
| ANSI escape sequences: `\x1B\[[0-9;]*[A-Za-z]` | Remove entirely (compiled regex) |
| Zero-width characters: `\u{200B}`, `\u{200C}`, `\u{200D}`, `\u{FEFF}` | Remove (overlaps Stage 2 — idempotent) |

The ANSI regex is compiled once at stage construction (not per invocation).

#### Stage 5: Structural Detection (`structural.rs`)

Three sub-detectors, tried in order. If any matches, the others are skipped.

**JSON detection:**
1. Attempt `serde_json::from_str::<serde_json::Value>(input)`.
2. If the entire input parses as valid JSON, re-serialize with `serde_json::to_string_pretty()`.
3. If it does not parse, pass through unchanged.
4. Conservative: only triggers on content that is entirely valid JSON. A JSON snippet embedded in prose does not trigger.
5. Recursion limit: uses serde_json default (128 levels). Content exceeding this limit passes through untransformed.

**Table detection:**
1. Split input into lines.
2. For each candidate delimiter (tab, comma, semicolon — tried in that order):
   - Split each line by the delimiter, respecting RFC 4180 quoted fields.
   - If ≥ 80% of lines have the same column count AND column count ≥ 2 AND line count ≥ 2, treat as tabular.
3. **Output size guard:** If the generated Markdown table exceeds 2× the input size, pass through untransformed.
4. Emit Markdown pipe table:
   - **Header inference:** First row is a header if its values are distinct and < 80% numeric. Otherwise generate `Col 1 | Col 2 | ...`.
   - **Alignment:** Right-align columns where > 80% of non-header values match `^\s*-?[\d,.]+%?\s*$`. Left-align everything else.
   - **Multi-line cells:** Replace embedded newlines with `<br>`.

**Code fence wrapping:**
1. If input is > 3 lines and looks like code:
   - ≥ 40% of lines start with whitespace (indentation).
   - OR first line matches a shebang (`#!`).
   - OR keyword frequency matches a known language (see below).
2. Wrap in `` ``` `` fences with a language hint.
3. **Language heuristic** (keyword frequency):
   - `def`, `import`, `print(` → `python`
   - `fn`, `let mut`, `impl`, `use std::` → `rust`
   - `function`, `const`, `=>`, `console.` → `javascript`
   - `func`, `var`, `import Foundation` → `swift`
   - `package`, `func`, `fmt.` → `go`
   - `public class`, `System.out` → `java`
   - Falls back to bare `` ``` `` fences if no language scores above threshold.
4. Already-fenced content (starts and ends with `` ``` ``) is skipped.

#### Stage 6: URL Cleaning (`url.rs`)

1. Scan text for URLs using regex: `https?://[^\s<>"'\)]+`.
2. For each URL found, parse with `url::Url`.
3. Apply global deny list. Deny patterns support trailing glob (`utm_*` matches `utm_source`, `utm_medium`, etc.).
4. Apply per-domain overrides: if the URL's effective domain (after stripping `www.`) matches an override key, apply that domain's `keep` list first (these params are never stripped), then its `deny` list.
5. Strip `www.` from the host.
6. If `url_strip_scheme` is `true` (aggressive mode, opt-in), also strip the scheme (`https://`).
7. Reassemble URL and replace in-place in the text.

**Default deny list** (compiled into the binary, active when no config is present):
```
utm_source, utm_medium, utm_campaign, utm_term, utm_content,
gclid, fbclid, mc_cid, mc_eid, si, ref_src, _ga, _gl
```

**Corruption risk — parameter stripping:** Parameters that look "tracking-like" may be functional in internal tools and APIs. The deny list is deliberately conservative (only well-known tracking params). Per-domain overrides with `keep` lists provide an escape hatch. The `ccvv preview` command allows users to inspect URL changes before relying on them.

**Corruption risk — `www.` stripping:** `www.example.com` and `example.com` are not guaranteed to resolve to the same host. Some servers only have a DNS record for the `www.` subdomain. Stripping `www.` can produce a URL that does not resolve. This stripping is always applied during URL cleaning (Step 5). Users who encounter broken URLs can add per-domain overrides or disable URL cleaning for specific domains. `ccvv doctor` will document this behavior.

URL cleaning preserves non-URL text surrounding the URL unchanged. It does not modify URLs inside code fences or backtick-wrapped spans.

#### Stage 7: Auto-Wrapper (`autowrap.rs`)

Direct port of the Swift `addInlineCodeMarkersToPlainText` pipeline (`mac/main.swift` lines 320–516).

**Algorithm:**
1. Split each line by backtick (`` ` ``) boundaries. Only process segments outside existing backtick spans (even-indexed segments in a 0-indexed split).
2. Within each segment, tokenize by whitespace (`\S+`).
3. For each token, strip leading/trailing punctuation. Test the core token against heuristics.
4. **Heuristic set** (from `shouldWrapCodeToken`):
   - Starts with `--` (CLI flags)
   - Contains `_` (snake_case identifiers)
   - Path-like: starts with `/`, `./`, `../`, or contains `/` with `.` or `-`
   - Filename: matches `\w+\.\w{1,5}` (e.g., `file.txt`, `config.yaml`)
   - Host:port: matches `\w+:\d+`
   - camelCase: matches `[a-z]+[A-Z][A-Za-z0-9]*`
   - SCREAMING_CASE: matches `[A-Z][A-Z0-9_]{2,}`
5. Tokens matching any heuristic are wrapped in backticks.
6. Lines starting with `$` or `>` (shell prompts) have the entire line content wrapped.
7. Lines inside code fences are skipped entirely.
8. Tokens that are URLs (contain `://`) are skipped.

**Shell safety risk:** Backticks are command substitution delimiters in many shells (`sh`, `bash`, `zsh`). Content wrapped in backticks and pasted into a terminal could be executed as a command. The auto-wrapper's heuristics target exactly the kind of tokens that appear in shell commands: file paths, CLI flags, snake_case identifiers. ccvv's core audience — developers — pastes these into terminals constantly.

**For this reason, auto-wrapper is disabled by default (`auto_wrapper = false`).** Users who primarily paste into Markdown editors, chat apps, or documentation tools can enable it via Preferences or config. When enabled, the risk is mitigated by: (a) bypass modifier (Option key), (b) app exclusion list (which can include terminal apps like `com.apple.Terminal`, `com.googlecode.iterm2`), and (c) documentation in first-run onboarding if the feature is enabled.

#### Stage 8: User-Defined Regex Rules (`userrules.rs`)

Applies compiled `Vec<CompiledUserRule>` from the resolved config, in declaration order.

```rust
pub struct CompiledUserRule {
    pub name: String,
    pub regex: regex::Regex,
    pub replacement: String,
}
```

For each rule: `regex.replace_all(text, replacement)`. Capture groups (`$1`, `$2`) are supported in the replacement string per the `regex` crate's syntax. Each rule that modifies the text is recorded in `TransformContext.rules_fired`.

**Safety limits:**
- Maximum 50 user rules. Config validation rejects files with more.
- Per-rule execution: if `replace_all` takes longer than 50ms (wall clock), the rule is aborted, the input for that rule is returned unchanged, and a timeout entry is logged in `ctx.rules_fired`.
- The `regex` crate guarantees linear-time matching (no backreferences, no lookahead), eliminating ReDoS. However, `replace_all` can produce large output if the replacement string is much longer than the match. Output size is bounded by the inter-stage expansion check (§5.2).

### 5.5 Sensitive Content Filter (`secrets.rs`)

Before any transform stage runs, the pipeline checks the input against a set of secret-detection patterns. If any match, the input is returned unchanged and `ctx.skipped_sensitive = true`.

**Patterns** (compiled regex, checked in order — short-circuit on first match):

| Pattern | Description |
|---------|-------------|
| `-----BEGIN (RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----` | PEM private key header |
| `-----BEGIN CERTIFICATE-----` | X.509 certificate |
| `^[A-Za-z0-9+/]{40,}={0,2}$` (single-line, > 40 chars) | Long base64 (likely key/token) |
| `(sk|pk|rk|ak)[-_][a-zA-Z0-9]{20,}` | API key prefixes (OpenAI, Stripe, etc.) |
| `ghp_[a-zA-Z0-9]{36}` | GitHub personal access token |
| `xox[bpsar]-[a-zA-Z0-9-]{10,}` | Slack tokens |
| `eyJ[a-zA-Z0-9_-]{10,}\.[a-zA-Z0-9_-]{10,}` | JWT (starts with base64 `{"`) |
| `AKIA[A-Z0-9]{16}` | AWS access key ID |

**Config:** `settings.sensitive_filter = true` (default). Can be disabled by the user. When disabled, `ccvv doctor` prints a warning.

**Design note:** This is a best-effort heuristic, not a security boundary. It will not catch all secrets. Its purpose is to reduce the most common cases of accidentally transforming (and potentially corrupting or persisting) sensitive content.

### 5.6 Double-Tap Detection & Pasteboard Synchronization

The double-tap mechanism uses **CGEventTap for keystroke timing** combined with **NSPasteboard.changeCount for clipboard confirmation**. This hybrid approach provides sub-millisecond keystroke timing accuracy while ensuring the pasteboard has actually updated before reading content.

**Critical timing issue:** The CGEventTap callback fires on `keyDown`, but `NSPasteboard.general` is NOT updated synchronously with the keystroke. The foreground app's copy handler runs asynchronously, and the pasteboard typically updates *after* the key event has been delivered and processed. Under load (JetBrains IDEs, Electron apps, VM/remote desktop), this delay can be tens or hundreds of milliseconds. Reading the pasteboard immediately will intermittently get stale content.

**Algorithm:**

1. **First Cmd+C detected:**
   - Record `timestamp_1` and `changeCount_1 = NSPasteboard.general.changeCount`.
   - Start a short-lived poll loop: check `changeCount` every 10ms for up to 200ms.
   - When `changeCount` increments, snapshot: `hash_1 = SHA-256(pasteboard.string)`, `changeCount_after_1 = changeCount`.
   - If `changeCount` does not increment within 200ms, the keystroke did not result in a copy (e.g., `Cmd+C` used for something else). Record no snapshot.

2. **Second Cmd+C detected within double-tap window:**
   - Same poll loop: wait for `changeCount` to increment beyond `changeCount_after_1`.
   - When it does, compute `hash_2 = SHA-256(pasteboard.string)`.
   - If `hash_1 == hash_2`, this is a "same content double-tap" — trigger transform.
   - If `hash_1 != hash_2`, the user copied new content — update snapshot, do NOT transform.
   - If `changeCount` does not increment within 200ms, abort (no transform).

3. **Write-back guard (`isWritingBack`):**
   - When ccvv writes cleaned text back to the pasteboard, it sets `isWritingBack = true` and records the expected `changeCount` after write.
   - The poll loop ignores `changeCount` increments that match the write-back `changeCount`.
   - `isWritingBack` is cleared after the write completes.
   - This prevents the write-back from being mistaken for a new user copy.

**Hash specification:** SHA-256 (via the `sha2` crate) of the `.string` pasteboard type, UTF-8 encoded, no normalization. Collision probability is negligible (2^-128). For clipboard text < 100 KB, SHA-256 computation is < 1ms.

**changeCount as cheap first-pass:** Before any content read, compare `changeCount` values. If `changeCount` hasn't changed between taps, the clipboard hasn't been written to — skip the expensive content read and hash entirely.

**TOCTOU mitigation:** Store a "pasteboard snapshot" (changeCount + content hash) at first-copy time rather than re-reading the pasteboard for comparison on the second tap. This bounds the race window: if another app writes to the pasteboard between the first snapshot and the second tap, the changed `changeCount` detects the interference.

### 5.7 Idempotency Contract

**Requirement:** For any input string `s` and any config `c`:
```
let (out1, _) = pipeline.run(s);
let (out2, _) = pipeline.run(&out1);
assert_eq!(out1, out2);
```

This is enforced by design:
- Stage 2 replaces characters with ASCII equivalents that Stage 2 does not further modify.
- Stage 3 strips whitespace and joins lines; running again on joined lines produces the same output.
- Stage 4 removes markers; running again on text with no markers is a no-op.
- Stage 5 outputs valid JSON / Markdown tables / fenced code; running again detects these formats and leaves them intact.
- Stage 6 strips params from URLs; URLs without params pass through.
- Stage 7 skips tokens already inside backticks.
- Stage 8 applies regex replace; well-formed replacements are idempotent if they don't produce new matches.

**Testing:** An integration test runs the full pipeline twice on every fixture and asserts equality.

---

## 6. Configuration

### 6.1 Config File Location

Checked in order:
1. Path passed explicitly (FFI `ccvv_load_config(path)` or CLI `--config`).
3. `~/.ccvv/config.toml`
4. If none found, all defaults apply. No error — the config file is optional.

**Config file found but unparseable:** This is distinct from "not found." If the config file exists but fails TOML parsing, ccvv does NOT silently fall back to defaults (which would cause silent loss of user preferences). Instead:
- **App:** Keep the previously loaded valid config (if any). Show a warning badge on the menu bar icon. Menu includes "Config error: {message}" as a disabled item. Log the parse error.
- **CLI:** Print the parse error to stderr and exit with code 1 (for `ccvv transform`), or include the error in `ccvv doctor` output.
- **First launch (no previous config):** Use defaults but show the warning badge. The user's config file is not deleted or overwritten.

### 6.2 Config Schema

```rust
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct CcvvConfig {
    pub settings: Settings,
    pub url_params: UrlParamConfig,
    pub paste_targets: HashMap<String, String>,
    pub exclusions: Exclusions,
    pub profiles: HashMap<String, ProfileOverrides>,
    pub rules: Vec<UserRule>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub double_tap_window_ms: DoubleTapSetting,  // u32 or "auto"
    pub hud_toast: bool,                          // default: true
    pub url_strip_scheme: bool,                   // default: false
    pub em_dash: EmDashConfig,
    pub max_input_bytes: usize,                   // default: 1_048_576
    pub sensitive_filter: bool,                    // default: true
    pub history_store_raw: bool,                   // default: false

    // Feature toggles — all default true except auto_wrapper
    pub normalize_unicode: bool,
    pub whitespace_cleanup: bool,
    pub agent_strip: bool,
    pub structural_detection: bool,
    pub url_cleaning: bool,
    pub auto_wrapper: bool,       // default: FALSE — opt-in due to shell safety risk (see §5.4 Stage 7)
    pub user_rules: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum DoubleTapSetting {
    Fixed(u32),       // 150–500
    Adaptive(String), // "auto"
}

#[derive(Debug, Clone, Deserialize)]
pub struct EmDashConfig {
    pub replace: String, // default: "--"
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UrlParamConfig {
    pub deny: Vec<String>,
    pub overrides: HashMap<String, DomainOverride>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DomainOverride {
    pub keep: Vec<String>,
    pub deny: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Exclusions {
    pub apps: Vec<String>,  // bundle identifiers
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ProfileOverrides {
    pub json: Option<String>,            // "flatten" | "prettify" | "off"
    pub code_fence: Option<bool>,
    pub url_strip_scheme: Option<bool>,
    pub table_to_markdown: Option<bool>,
    pub blind_paste: Option<bool>,
    pub all_smart_features: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserRule {
    pub name: String,
    pub pattern: String,
    pub replace: String,
}
```

### 6.3 Config Resolution

```rust
/// Fully resolved config — no Option fields, all regexes compiled.
pub struct ResolvedConfig {
    pub double_tap_window_ms: Option<u32>, // None = adaptive
    pub hud_toast: bool,
    pub stages_enabled: StagesEnabled,
    pub em_dash_replacement: String,
    pub url_params: ResolvedUrlParams,
    pub url_strip_scheme: bool,
    pub json_mode: JsonMode,
    pub exclusions: Vec<String>,
    pub paste_targets: HashMap<String, String>,
    pub user_rules: Vec<CompiledUserRule>,
    pub max_input_bytes: usize,
    pub sensitive_filter: bool,
    pub history_store_raw: bool,
}

pub struct StagesEnabled {
    pub normalize_unicode: bool,
    pub whitespace_cleanup: bool,
    pub agent_strip: bool,
    pub structural_detection: bool,
    pub url_cleaning: bool,
    pub auto_wrapper: bool,
    pub user_rules: bool,
}

pub enum JsonMode { Prettify, Flatten, Off }
```

**Merge semantics:** When a user config is loaded, it is **overlaid** onto defaults, not replaced. Specifically:

- `exclusions.apps`: User list is **appended** to the compiled-in default list (password managers). To remove a default entry, the user specifies `exclusions.remove_apps = ["com.example.app"]`.
- `url_params.deny`: User list is **appended** to the compiled-in default deny list. To clear the default list entirely, the user specifies `url_params.deny_replace = true` alongside their custom list.
- All other fields: User values replace defaults. Missing fields inherit defaults via `#[serde(default)]`.

This prevents the most dangerous misconfiguration: a user creating a config for one setting and accidentally wiping the app exclusion list or URL deny list.

Resolution process:
1. Parse TOML into `CcvvConfig`.
2. If a profile name is given, overlay `ProfileOverrides` onto `Settings`. Fields that are `None` inherit from base settings. `all_smart_features = false` disables all stages except blind paste.
3. Compile all `UserRule.pattern` strings into `regex::Regex`. If any pattern is invalid, return `CcvvError::Regex` with the rule name and source error.
4. Validate `double_tap_window_ms` range (150–500 for fixed, or "auto").
5. Enforce user rules limit (max 50).
6. Return `ResolvedConfig`.

```rust
pub fn load_config(path: Option<&Path>) -> Result<CcvvConfig, CcvvError>;
pub fn resolve_config(
    config: &CcvvConfig,
    profile: Option<&str>,
) -> Result<ResolvedConfig, CcvvError>;
```

Regex compilation happens at config load time, not at transform time. The hot path never compiles regexes.

### 6.4 Config Validation

`ccvv validate` (CLI) and `ccvv_validate_config` (FFI) check:
- TOML syntax.
- All `pattern` fields compile as valid regex.
- `double_tap_window_ms` is in range or "auto".
- All profile names referenced in `paste_targets` exist in `profiles`.
- All app identifiers in `exclusions.apps` are plausible bundle identifiers (contain at least one `.`).
- User rules count ≤ 50.
- No replacement string exceeds 1000 characters.

Returns a list of errors/warnings, not just the first failure.

### 6.5 Config Integrity

**File permissions:** On load, check that the config file is:
- Owned by the current user (`stat().st_uid == getuid()`).
- Not group-writable or world-writable (mode `& 0o022 == 0`).
- If either check fails, log a warning to stderr (CLI) or show a one-time alert (app). The config is still loaded — this is a warning, not a hard block — but the warning is prominent.

**Atomic writes (Preferences UI):** When the macOS app writes config changes:
1. Serialize the updated config to a `String`.
2. Write to a temporary file in the same directory: `config.toml.tmp.XXXXXX` (random suffix).
3. `fsync()` the temporary file.
4. `rename()` the temporary file to `config.toml` (atomic on POSIX).

This ensures a crash mid-write cannot produce a partial config file.

**Config change detection:** On each transform, compare the config file's `mtime` with the last-loaded `mtime`. If changed, reload. If reload fails (parse error), keep the previous valid config and log a warning.

---

## 7. History Database

### 7.1 SQLite Schema

```sql
CREATE TABLE IF NOT EXISTS history (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    raw_hash     TEXT    NOT NULL,     -- SHA-256 hex of raw text (always stored)
    raw_text     TEXT,                 -- raw text (NULL unless history_store_raw = true)
    cleaned_text TEXT    NOT NULL,
    content_type TEXT    NOT NULL,     -- 'url' | 'code' | 'prose' | 'table' | 'json' | 'mixed'
    preview      TEXT    NOT NULL,     -- first 100 chars of cleaned_text, truncated
    committed    INTEGER NOT NULL DEFAULT 0,  -- 0 = pending, 1 = committed (see §9.2)
    created_at   INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_history_created ON history(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_history_type ON history(content_type);
CREATE INDEX IF NOT EXISTS idx_history_raw_hash ON history(raw_hash);
```

**Key change from v1.0:** `raw_text` is now nullable and only populated when `settings.history_store_raw = true`. By default, only the SHA-256 hash of the raw text is stored. This dramatically reduces the sensitivity of the history database — it can confirm that specific content was cleaned (via hash comparison) but cannot reconstruct the original content.

**Undo behavior with `history_store_raw = false`:** The undo stack uses the in-memory ring buffer (§7.7) for the most recent N transforms. Undo beyond the ring buffer size is unavailable. This is an explicit trade-off: privacy over convenience.

### 7.2 History API

```rust
pub struct HistoryDb {
    conn: Mutex<rusqlite::Connection>,
    ring_buffer: Mutex<VecDeque<UndoEntry>>,  // in-memory emergency undo
}

pub struct HistoryEntry {
    pub id: i64,
    pub raw_hash: String,
    pub raw_text: Option<String>,
    pub cleaned_text: String,
    pub content_type: ContentType,
    pub preview: String,
    pub created_at: i64,
}

/// UndoEntry fields are zeroized on drop to prevent sensitive clipboard
/// content from lingering in the process's address space after pruning.
#[derive(zeroize::ZeroizeOnDrop)]
pub struct UndoEntry {
    pub raw_text: String,
    pub cleaned_text: String,
    #[zeroize(skip)]
    pub timestamp: i64,
}

impl HistoryDb {
    /// Open or create database. Creates schema if absent.
    /// Sets PRAGMA journal_mode=WAL, busy_timeout=5000.
    pub fn open(path: &Path) -> Result<Self, CcvvError>;

    /// Phase 1 of two-phase commit: insert entry with committed=0.
    /// Returns the row ID for later commit/rollback.
    pub fn prepare(&self, raw: &str, cleaned: &str, store_raw: bool) -> Result<i64, CcvvError>;

    /// Phase 2: set committed=1. Also pushes to in-memory ring buffer.
    pub fn commit_entry(&self, id: i64) -> Result<(), CcvvError>;

    /// Rollback: delete uncommitted entry.
    pub fn rollback_entry(&self, id: i64) -> Result<(), CcvvError>;

    /// Startup cleanup: delete all entries with committed=0
    /// (these represent interrupted operations from a previous crash).
    pub fn cleanup_uncommitted(&self) -> Result<usize, CcvvError>;

    /// Most recent N committed entries, newest first.
    pub fn recent(&self, limit: usize) -> Result<Vec<HistoryEntry>, CcvvError>;

    /// Substring search across cleaned_text (and raw_text if stored).
    pub fn search(&self, query: &str) -> Result<Vec<HistoryEntry>, CcvvError>;

    /// Filter by content type.
    pub fn by_type(&self, ct: ContentType) -> Result<Vec<HistoryEntry>, CcvvError>;

    /// Get most recent raw text from ring buffer (for undo).
    /// Returns None if ring buffer is empty or index out of range.
    pub fn undo_raw(&self, n: usize) -> Option<String>;

    /// Prune old entries beyond retention limit.
    fn prune(&self) -> Result<(), CcvvError>;
}
```

**Pruning:** After each successful `commit_entry()`, execute:
```sql
DELETE FROM history WHERE committed = 1 AND id NOT IN (
    SELECT id FROM history WHERE committed = 1 ORDER BY created_at DESC LIMIT 50
);
```

### 7.3 SQLite Operational Configuration

Set on every `Connection::open`:

```rust
conn.pragma_update(None, "journal_mode", "WAL")?;
conn.pragma_update(None, "busy_timeout", 5000)?;   // 5 seconds
conn.pragma_update(None, "synchronous", "NORMAL")?; // safe with WAL
conn.pragma_update(None, "foreign_keys", "ON")?;
```

**WAL mode** allows concurrent reads (CLI reading while app writes). The `Mutex` serializes writes within the same process; `busy_timeout` handles cross-process contention (app + CLI).

**VACUUM:** Run `VACUUM` on startup if the database file size exceeds 10 MB. This reclaims space from deleted entries. Large clipboard entries can bloat the WAL/DB file; VACUUM compacts it.

**Corruption handling:** If any SQLite operation returns `SQLITE_CORRUPT`, `SQLITE_NOTADB`, or `SQLITE_FULL`:
1. Close the connection.
2. Rename the corrupted file to `history.db.corrupt.{timestamp}`.
3. Open a fresh database.
4. Log a warning visible to the user (HUD toast: "History database was reset due to corruption").
5. The in-memory ring buffer is unaffected, preserving recent undo capability.

### 7.4 Content Classifier (`classify.rs`)

```rust
pub fn classify(text: &str) -> ContentType {
    if serde_json::from_str::<serde_json::Value>(text).is_ok() {
        return ContentType::Json;
    }
    if is_single_url(text) {
        return ContentType::Url;
    }
    if looks_like_table(text) {
        return ContentType::Table;
    }
    if looks_like_code(text) {
        return ContentType::Code;
    }
    ContentType::Prose
}
```

- `is_single_url`: trimmed text matches `^https?://\S+$`.
- `looks_like_table`: consistent tab/comma column counts across ≥ 80% of lines, ≥ 2 columns.
- `looks_like_code`: ≥ 40% of lines start with whitespace, or contains shebang, or high keyword density for known languages.

### 7.5 Database Location & Protection

- **macOS:** `~/.ccvv/history.db`

The parent directory is created on first `HistoryDb::open()` if it does not exist.

**File permissions:** The data directory and database file are created with mode `0700` (directory) and `0600` (file). On each open, permissions are verified and corrected if looser than expected.

**Time Machine exclusion (macOS):** On directory creation, set the `NSURLIsExcludedFromBackupKey` resource value:

```swift
var url = URL(fileURLWithPath: supportDir)
var resourceValues = URLResourceValues()
resourceValues.isExcludedFromBackup = true
try url.setResourceValues(resourceValues)
```

This is set from Swift after `HistoryDb::open()` returns the path. The Rust side does not handle macOS-specific resource values.

**Spotlight exclusion (macOS):** Place a `.metadata_never_index` file in the data directory on creation.

### 7.6 Sensitive Content in History

Even with `history_store_raw = false` (default), the `cleaned_text` column may contain sensitive data if the sensitive filter is disabled or if the content didn't match any pattern.

**Additional protection:** If the cleaned text matches any sensitive pattern (§5.5), the history entry stores `preview = "[sensitive content]"` and `cleaned_text = "[redacted — undo via ring buffer only]"` instead of the actual cleaned text. The in-memory ring buffer still holds the real values for undo.

### 7.7 In-Memory Ring Buffer (Emergency Undo)

```rust
const RING_BUFFER_SIZE: usize = 10;

struct RingBuffer {
    entries: VecDeque<UndoEntry>,
}
```

The ring buffer holds the last 10 `(raw_text, cleaned_text)` pairs in memory. It is:
- Populated on every successful `commit_entry()`.
- Unaffected by database corruption or failure.
- Lost on process termination (intentional — reduces persistence of sensitive data).
- Queried by the undo function before falling back to the database.
- Entries evicted from the ring buffer are zeroized (via `ZeroizeOnDrop` on `UndoEntry`) so sensitive content does not linger in the process's address space.

---

## 8. C FFI Surface

### 8.1 Opaque Types

Every Rust object exposed across the FFI boundary is an opaque pointer. The caller never sees the struct layout.

```c
typedef struct CcvvConfig CcvvConfig;
typedef struct CcvvHistory CcvvHistory;
```

### 8.2 Transform Result

```c
typedef struct {
    char *cleaned_text;    // heap-allocated; free with ccvv_string_free()
    char *summary;         // heap-allocated; free with ccvv_string_free()
                           // e.g., "unwrapped 8 lines, stripped 2 params"
    uint32_t rules_count;  // number of rules that fired
    bool skipped_sensitive; // true if sensitive filter triggered
    bool skipped_oversize;  // true if input exceeded size limit
} CcvvTransformResult;
```

### 8.3 Function Signatures

```c
// ── Transform ──────────────────────────────────────────────────

// Apply the full pipeline. Config may be NULL (uses all defaults).
// On error, returns result with cleaned_text = NULL.
// Check error_out for details.
// IMPORTANT: input must be null-terminated. For non-null-terminated
// buffers, use ccvv_transform_n instead.
// IMPORTANT: caller MUST call ccvv_transform_result_free on the returned
// result in ALL cases (even when cleaned_text is NULL) to avoid leaking
// the summary string.
CcvvTransformResult ccvv_transform(
    const char *input,
    const CcvvConfig *config,
    char **error_out          // set to heap-allocated error string on failure;
                              // NULL on success. Caller frees with ccvv_string_free().
);

// Length-delimited variant. Preferred for non-Rust FFI consumers.
// Does not require null-terminated input.
CcvvTransformResult ccvv_transform_n(
    const char *input,
    size_t input_len,
    const CcvvConfig *config,
    char **error_out
);

void ccvv_transform_result_free(CcvvTransformResult result);

// ── Config ─────────────────────────────────────────────────────

// Load config. path may be NULL (searches default locations).
// Returns NULL on file-not-found (uses defaults in that case — not an error).
// Returns NULL with error_out set on parse failure.
CcvvConfig *ccvv_load_config(
    const char *path,
    char **error_out
);

void ccvv_config_free(CcvvConfig *config);

uint32_t ccvv_get_double_tap_window_ms(const CcvvConfig *config);

bool ccvv_is_feature_enabled(
    const CcvvConfig *config,
    const char *feature_name
);

bool ccvv_is_app_excluded(
    const CcvvConfig *config,
    const char *bundle_id
);

// Validate config file. Returns NULL on success.
// Returns heap-allocated error string on failure (free with ccvv_string_free).
char *ccvv_validate_config(const char *path);

// ── History ────────────────────────────────────────────────────

CcvvHistory *ccvv_history_open(
    const char *db_path,
    char **error_out
);

// Phase 1: prepare entry (uncommitted). Returns row ID, or -1 on error.
int64_t ccvv_history_prepare(
    CcvvHistory *history,
    const char *raw_text,
    const char *cleaned_text,
    bool store_raw,
    char **error_out
);

// Phase 2: commit entry.
bool ccvv_history_commit(
    CcvvHistory *history,
    int64_t entry_id,
    char **error_out
);

// Rollback: delete uncommitted entry.
bool ccvv_history_rollback(
    CcvvHistory *history,
    int64_t entry_id,
    char **error_out
);

// Get raw text for undo from ring buffer. Returns NULL if unavailable.
char *ccvv_history_undo_raw(
    const CcvvHistory *history,
    uint32_t n
);

// Returns JSON array of entries. Caller frees with ccvv_string_free().
char *ccvv_history_get_recent_json(
    const CcvvHistory *history,
    uint32_t count
);

char *ccvv_history_search_json(
    const CcvvHistory *history,
    const char *query
);

void ccvv_history_free(CcvvHistory *history);

// ── Memory ─────────────────────────────────────────────────────

void ccvv_string_free(char *s);
```

### 8.4 Memory Management Contract

| Returned by | Ownership | Free with |
|-------------|-----------|-----------|
| `ccvv_transform` → `cleaned_text`, `summary` | Caller owns | `ccvv_string_free()` |
| `ccvv_load_config` | Caller owns | `ccvv_config_free()` |
| `ccvv_history_open` | Caller owns | `ccvv_history_free()` |
| `ccvv_history_get_recent_json` | Caller owns | `ccvv_string_free()` |
| `ccvv_history_search_json` | Caller owns | `ccvv_string_free()` |
| `ccvv_history_undo_raw` | Caller owns | `ccvv_string_free()` |
| `ccvv_validate_config` | Caller owns | `ccvv_string_free()` |
| `error_out` (all functions) | Caller owns | `ccvv_string_free()` |

**Null safety:** Every FFI function handles NULL input pointers gracefully (returns NULL or no-ops). No function will crash on NULL input. `error_out` may be NULL if the caller does not want error details.

**Length-delimited vs null-terminated:** `ccvv_transform` scans for a null terminator with no length limit (via `CStr::from_ptr`). Swift's `withCString` guarantees null termination, but other FFI consumers (Python, C) may not. The `ccvv_transform_n` variant is the recommended API for non-Swift consumers; the null-terminated version is a convenience wrapper around it.

### 8.5 Thread Safety

- `CcvvConfig` is immutable after creation. Safe to share across threads without synchronization.
- `CcvvHistory` uses internal `Mutex<rusqlite::Connection>`. Safe to call from any thread, but calls are serialized.
- `ccvv_transform` is stateless given a config. Safe to call concurrently with the same config pointer.
- **Error reporting uses `error_out` parameters (not thread-local storage).** This is safe with Swift concurrency — each call site gets its own error string regardless of which queue or thread the call runs on. This replaces the v1.0 `ccvv_last_error()` thread-local approach, which was unsafe with Swift's cooperative threading model where multiple async tasks may share a thread.

### 8.6 C Header Generation

`cbindgen` generates `ccvv-bridge.h` from the Rust source:

```toml
# ccvv-lib/cbindgen.toml
language = "C"
include_guard = "CCVV_LIB_H"
no_includes = true
sys_includes = ["stdint.h", "stdbool.h", "stddef.h"]

[export]
include = ["CcvvTransformResult"]

[fn]
prefix = ""
```

The header is regenerated as part of `build.sh`. It is committed to the repository for visibility but the build script is the source of truth.

---

## 9. Swift ↔ Rust Integration

### 9.1 Boundary Definition

| Concern | Implemented in | Rationale |
|---------|----------------|-----------|
| NSPasteboard read/write | Swift | Requires AppKit |
| RTF/HTML → NSAttributedString | Swift | Requires AppKit |
| Monospace font detection | Swift | Requires NSFont |
| Syntax color detection | Swift | Requires NSColor |
| Rich-text inline code inference | Swift | Requires NSAttributedString enumeration |
| CGEventTap / keyboard monitoring | Swift | Requires ApplicationServices |
| Status item / menu bar | Swift | Requires AppKit |
| HUD toast window | Swift | Requires NSWindow |
| Data directory protection (backup/index exclusion) | Swift | Requires Foundation URL resource values |
| Text transform pipeline (stages 2–8) | Rust (via FFI) | Shared with CLI |
| Config loading and parsing | Rust (via FFI) | Shared with CLI |
| History database | Rust (via FFI) | Shared with CLI |
| Secret pattern detection | Rust (via FFI) | Shared with CLI |
| Plain-text token wrapping heuristics | Rust (autowrap stage) | Shared with CLI |

### 9.2 Call Flow (Two-Phase Commit)

```
User double-taps Cmd+C
    │
    ▼
AppDelegate.handleCGKeyEvent()     [Swift — event tap callback, background runloop]
    │ detects double-tap within window (see §5.6 for timing)
    │ dispatches to main thread:
    ▼
DispatchQueue.main.async {         [Swift — main thread]
    │
    ├── CHECK: frontmost app excluded?
    │   let bundleId = NSWorkspace.shared.frontmostApplication?.bundleIdentifier
    │   if ccvvCore.isAppExcluded(bundleId) → return
    │
    ├── CHECK: pasteboard has .string type?
    │   if !pb.types?.contains(.string) → return
    │   (Skip if pasteboard contains only files, images, or other non-text types.
    │    This prevents destroying Finder file copies or image pastes.)
    │
    ├── WAIT: pasteboard sync (see §5.6)
    │   Poll changeCount every 10ms for up to 200ms.
    │   If changeCount does not increment → abort (keystroke didn't produce a copy)
    │
    ├── extractClipboardTextWithStyleHints(pb)    [Swift — reads NSPasteboard,
    │       │                                      extracts rich text, infers
    │       │                                      inline code from fonts/colors]
    │       ▼
    │   text: String (plain text with backtick-wrapped code markers)
    │
    ├── ccvv_transform(text, config, &error)      [Rust via FFI — stages 2-8]
    │       │
    │       ▼
    │   CcvvTransformResult { cleaned_text, summary, rules_count,
    │                         skipped_sensitive, skipped_oversize }
    │
    │   if skipped_sensitive || skipped_oversize → show feedback, return
    │   if cleaned_text == text → no changes, return
    │
    │── PHASE 1: Persist raw for undo ────────────────────────────
    │   entry_id = ccvv_history_prepare(history, raw, cleaned, store_raw, &error)
    │       │   [Rust via FFI — inserts with committed=0]
    │       │   if error → log, skip history, still write clipboard
    │       ▼
    │
    │── PHASE 2: Write clipboard ─────────────────────────────────
    │   isWritingBack = true
    │   pb.clearContents()
    │   pb.setString(cleaned_text, forType: .string)
    │   expectedChangeCount = pb.changeCount
    │   isWritingBack = false
    │       │   [Swift — write to system pasteboard]
    │       ▼
    │
    │── PHASE 3: Commit ──────────────────────────────────────────
    │   ccvv_history_commit(history, entry_id, &error)
    │       │   [Rust via FFI — sets committed=1, pushes ring buffer]
    │       ▼
    │
    └── showFeedback(summary, rules_count)         [Swift — icon flash + HUD toast]
}
```

**Key design notes:**

- **Main thread dispatch:** The event tap callback fires on a background runloop source. `NSWorkspace.shared.frontmostApplication` and `NSPasteboard` access must happen on the main thread. The callback dispatches to `DispatchQueue.main.async` immediately.
- **Pasteboard type check:** `pb.setString` calls `clearContents()` internally, destroying ALL pasteboard types (RTF, HTML, images, file promises). This is intentional for text-to-text transformation ("Blind Paste" — §5 of functional spec), but MUST NOT destroy non-text content. The type check at the top prevents corrupting Finder file copies, image pastes, etc.
- **Write-back guard:** `isWritingBack` prevents the pasteboard write from being detected as a new user copy by the changeCount monitoring in §5.6.
- **Sanitize = plain text:** Writing back as `.string` only means all rich formatting (RTF, HTML) is lost. This is an **explicit product decision** — ccvv produces sanitized plain text. This must be communicated in onboarding: "ccvv cleans your clipboard to plain text."

**Crash window analysis:**
- Crash after Phase 1 but before Phase 2: Clipboard still has raw text. Uncommitted history entry is cleaned up on next launch (`cleanup_uncommitted()`). **Invariant (b) holds.**
- Crash after Phase 2 but before Phase 3: Clipboard has cleaned text. History entry exists but is uncommitted. On next launch, `cleanup_uncommitted()` deletes the entry. However, the in-memory ring buffer was not yet populated, so undo is unavailable for this specific entry. **This is the narrow remaining risk.** Mitigation: the cleaned text is on the clipboard (user's most likely intent), and the loss is limited to the undo capability for one entry.
- Crash after Phase 3: Everything committed. **Invariant (a) holds.**

### 9.3 Swift Wrapper

A thin Swift struct isolates all FFI calls:

```swift
final class CcvvCore {
    private let config: OpaquePointer?
    private let history: OpaquePointer?

    init() {
        var configError: UnsafeMutablePointer<CChar>?
        config = ccvv_load_config(nil, &configError)
        if let err = configError {
            NSLog("ccvv: config load warning: %@", String(cString: err))
            ccvv_string_free(err)
        }

        let dbPath = CcvvCore.historyDbPath()
        var histError: UnsafeMutablePointer<CChar>?
        history = dbPath.withCString { ccvv_history_open($0, &histError) }
        if let err = histError {
            NSLog("ccvv: history open error: %@", String(cString: err))
            ccvv_string_free(err)
        }

        // Exclude data directory from Time Machine and Spotlight
        CcvvCore.protectDataDirectory()
    }

    func transform(_ input: String) -> TransformResult? {
        var error: UnsafeMutablePointer<CChar>?
        let result = input.withCString { ccvv_transform($0, config, &error) }
        if let err = error {
            NSLog("ccvv: transform error: %@", String(cString: err))
            ccvv_string_free(err)
            return nil
        }
        guard let cleanedPtr = result.cleaned_text else { return nil }
        let cleaned = String(cString: cleanedPtr)
        let summary = result.summary.map { String(cString: $0) } ?? ""
        let skippedSensitive = result.skipped_sensitive
        let skippedOversize = result.skipped_oversize
        ccvv_transform_result_free(result)
        return TransformResult(
            cleaned: cleaned, summary: summary,
            skippedSensitive: skippedSensitive,
            skippedOversize: skippedOversize
        )
    }

    /// Two-phase commit: prepare → write clipboard → commit.
    func prepareHistory(raw: String, cleaned: String) -> Int64? {
        guard let h = history else { return nil }
        var error: UnsafeMutablePointer<CChar>?
        let storeRaw = ccvv_is_feature_enabled(config, "history_store_raw")
        let entryId = raw.withCString { rawPtr in
            cleaned.withCString { cleanPtr in
                ccvv_history_prepare(h, rawPtr, cleanPtr, storeRaw, &error)
            }
        }
        if let err = error {
            NSLog("ccvv: history prepare error: %@", String(cString: err))
            ccvv_string_free(err)
            return nil
        }
        return entryId >= 0 ? entryId : nil
    }

    func commitHistory(entryId: Int64) {
        guard let h = history else { return }
        var error: UnsafeMutablePointer<CChar>?
        ccvv_history_commit(h, entryId, &error)
        if let err = error {
            NSLog("ccvv: history commit error: %@", String(cString: err))
            ccvv_string_free(err)
        }
    }

    func undoRaw(n: UInt32 = 1) -> String? {
        guard let h = history else { return nil }
        guard let ptr = ccvv_history_undo_raw(h, n) else { return nil }
        defer { ccvv_string_free(ptr) }
        return String(cString: ptr)
    }

    func recentHistory(count: UInt32) -> String? {
        guard let h = history else { return nil }
        guard let json = ccvv_history_get_recent_json(h, count) else { return nil }
        defer { ccvv_string_free(json) }
        return String(cString: json)
    }

    var doubleTapWindowSeconds: TimeInterval {
        guard let c = config else { return 0.3 }
        return TimeInterval(ccvv_get_double_tap_window_ms(c)) / 1000.0
    }

    func isAppExcluded(_ bundleId: String) -> Bool {
        guard let c = config else { return false }
        return bundleId.withCString { ccvv_is_app_excluded(c, $0) }
    }

    deinit {
        if let c = config { ccvv_config_free(c) }
        if let h = history { ccvv_history_free(h) }
    }

    private static func historyDbPath() -> String {
        let support = FileManager.default.urls(
            for: .applicationSupportDirectory, in: .userDomainMask
        ).first!.appendingPathComponent("ccvv")
        try? FileManager.default.createDirectory(
            at: support, withIntermediateDirectories: true,
            attributes: [.posixPermissions: 0o700]
        )
        return support.appendingPathComponent("history.db").path
    }

    private static func protectDataDirectory() {
        let support = FileManager.default.urls(
            for: .applicationSupportDirectory, in: .userDomainMask
        ).first!.appendingPathComponent("ccvv")

        // Exclude from Time Machine
        var url = support
        var resourceValues = URLResourceValues()
        resourceValues.isExcludedFromBackup = true
        try? url.setResourceValues(resourceValues)

        // Exclude from Spotlight
        let noIndex = support.appendingPathComponent(".metadata_never_index")
        if !FileManager.default.fileExists(atPath: noIndex.path) {
            FileManager.default.createFile(atPath: noIndex.path, contents: nil)
        }
    }
}

struct TransformResult {
    let cleaned: String
    let summary: String
    let skippedSensitive: Bool
    let skippedOversize: Bool
}
```

### 9.4 Build System

Updated `mac/build.sh`:

```bash
#!/bin/bash
set -e

APP_NAME="ccvv"
BUILD_DIR="build"
TEAM_ID="4JRN737CHR"
BUNDLE_ID="com.ccvv.app"
RUST_WORKSPACE="../core"
NOTARIZE=0

for arg in "$@"; do
    case "$arg" in
        --notarize) NOTARIZE=1 ;;
    esac
done

mkdir -p "$BUILD_DIR"

# ── Step 1: Build Rust static library ───────────────────────
echo "Building Rust library..."
cargo build --manifest-path "$RUST_WORKSPACE/Cargo.toml" \
    --package ccvv-lib --release

RUST_LIB="$RUST_WORKSPACE/target/release/libccvv_lib.a"
if [[ ! -f "$RUST_LIB" ]]; then
    echo "Error: Rust static library not found at $RUST_LIB"
    exit 1
fi

# ── Step 2: Generate C header ───────────────────────────────
echo "Generating C header..."
cbindgen --crate ccvv-lib \
    --config "$RUST_WORKSPACE/ccvv-lib/cbindgen.toml" \
    --output ccvv-bridge.h \
    "$RUST_WORKSPACE/ccvv-lib/"

# ── Step 3: Compile Swift ───────────────────────────────────
echo "Compiling..."
swiftc -o "$BUILD_DIR/$APP_NAME" main.swift \
    -framework Cocoa \
    -framework ApplicationServices \
    -import-objc-header ccvv-bridge.h \
    -L "$RUST_WORKSPACE/target/release" \
    -lccvv_lib \
    -O

# ── Step 4: App bundle + signing ────────────────────────────
echo "Creating app bundle..."
APP_BUNDLE="$BUILD_DIR/$APP_NAME.app"
rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources"
cp "$BUILD_DIR/$APP_NAME" "$APP_BUNDLE/Contents/MacOS/"
cp Info.plist "$APP_BUNDLE/Contents/"

echo "Signing..."
SIGN_ID=$(security find-identity -v -p codesigning \
    | grep "Developer ID Application" | head -1 \
    | sed 's/.*"\(.*\)"/\1/' || true)
if [[ -z "$SIGN_ID" ]]; then
    SIGN_ID=$(security find-identity -v -p codesigning \
        | head -1 | sed 's/.*"\(.*\)"/\1/' || true)
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

# Notarization (unchanged)
if [[ "$NOTARIZE" -eq 1 && "$SIGN_ID" == *"Developer ID"* ]]; then
    echo "Notarizing..."
    ZIP_PATH="$BUILD_DIR/$APP_NAME-notarize.zip"
    ditto -c -k --sequesterRsrc --keepParent "$APP_BUNDLE" "$ZIP_PATH"
    xcrun notarytool submit "$ZIP_PATH" \
        --keychain-profile "notarytool" --wait 2>&1 && {
        xcrun stapler staple "$APP_BUNDLE"
        rm -f "$ZIP_PATH"
    } || {
        echo "Notarization failed."
        rm -f "$ZIP_PATH"
    }
fi

echo ""
echo "Done: $APP_BUNDLE"
```

**Entitlements file** (`mac/ccvv.entitlements`):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
    "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <!-- Hardened Runtime is enabled via codesign --options runtime.
         This entitlements file documents what's needed and what's NOT needed.

         NOT needed:
         - com.apple.security.cs.disable-library-validation
           (Rust is statically linked into the main binary)
         - com.apple.security.cs.allow-unsigned-executable-memory
           (regex crate uses DFA/NFA, not JIT)
         - com.apple.security.app-sandbox
           (App Sandbox conflicts with CGEventTap / Accessibility TCC.
            Developer ID apps use TCC, not sandbox entitlements, for
            Accessibility and Input Monitoring consent.)

         Accessibility and Input Monitoring permissions are granted
         at runtime by the user via System Settings (TCC), not by
         entitlements. There is no entitlement that grants these.
    -->
</dict>
</plist>
```

**Why no App Sandbox:** App Sandbox and CGEventTap/Accessibility are fundamentally incompatible for Developer ID distribution. TCC (Transparency, Consent, and Control) permissions are granted at runtime by the user through System Settings. Entitlements control Hardened Runtime restrictions, not TCC consent. An empty entitlements file (with Hardened Runtime enabled via `codesign --options runtime`) is the correct configuration for a Developer ID menu bar utility that uses CGEventTap.

**Must test on a clean machine:** Sign, notarize, and run the app on a Mac that has never granted ccvv any permissions. Verify: (a) Accessibility prompt appears, (b) Input Monitoring prompt appears if required, (c) event tap functions after both are granted, (d) Gatekeeper accepts the notarized app (with quarantine attribute intact). Do NOT test only on the developer's machine which has cached permissions.

For **universal binary** (arm64 + x86_64), the Rust build step becomes:

```bash
cargo build --manifest-path "$RUST_WORKSPACE/Cargo.toml" \
    --package ccvv-lib --release --target aarch64-apple-darwin
cargo build --manifest-path "$RUST_WORKSPACE/Cargo.toml" \
    --package ccvv-lib --release --target x86_64-apple-darwin
lipo -create \
    "$RUST_WORKSPACE/target/aarch64-apple-darwin/release/libccvv_lib.a" \
    "$RUST_WORKSPACE/target/x86_64-apple-darwin/release/libccvv_lib.a" \
    -output "$BUILD_DIR/libccvv_lib.a"
```

And `swiftc` links from `$BUILD_DIR` instead. This requires both Rust targets:
```
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

---

## 10. CLI Specification

### 10.1 Commands

```
ccvv [OPTIONS]                     # Default: read stdin, transform, write stdout
ccvv preview [OPTIONS]             # Show diff + rules fired without modifying input
ccvv history [INDEX]               # List or retrieve history entries
ccvv doctor                        # Print diagnostics
ccvv validate [PATH]               # Validate config file
```

### 10.2 Global Options

| Flag | Description |
|------|-------------|
| `--config <PATH>` | Override config file path |
| `--profile <NAME>` | Apply a named profile |

### 10.3 Transform Options (default command)

| Flag | Description |
|------|-------------|
| `--strip-urls` | Run only URL cleaning stage |
| `--flatten-json` | Run only JSON flattener (minify) |
| `--prettify-json` | Run only JSON prettifier |
| `--unwrap` | Run only whitespace/unwrap stage |
| `--normalize` | Run only unicode normalization stage |
| (no flags) | Run full pipeline |

When specific stage flags are given, only those stages run. Flags are combinable.

### 10.4 Preview Subcommand

```
ccvv preview [--clipboard] [< input]
```

Reads input from stdin (or clipboard with `--clipboard`). Runs the full pipeline. Outputs:
1. A unified diff (via `similar` crate) showing changes.
2. A summary of rules fired with character counts.

Does not modify the clipboard.

### 10.5 History Subcommand

```
ccvv history                        # List 10 most recent entries
ccvv history <N>                    # Retrieve Nth most recent entry (print to stdout)
ccvv history --search <QUERY>       # Substring search
ccvv history --type <TYPE>          # Filter by type: url, code, prose, table, json
ccvv history --count <N>            # Show N entries (default 10)
```

**Note:** With `history_store_raw = false` (default), `ccvv history <N>` prints the cleaned text. Raw text is only available via the in-memory ring buffer (app process only, not CLI).

### 10.6 Doctor Subcommand

Prints:
- Platform and OS version.
- Config file path (found / not found).
- Config file permissions (owner, mode, warnings if too permissive).
- Config validation result.
- Active profile.
- Feature toggle states.
- Sensitive filter status (enabled / disabled with warning).
- Accessibility permission status (macOS).
- History database path, entry count, and file size.
- Data directory backup exclusion status.
- Whether `history_store_raw` is enabled (with warning if so).

### 10.7 Validate Subcommand

```
ccvv validate [PATH]
```

If `PATH` is omitted, validates the config at the default location. Prints all errors and warnings, then exits with code 0 (valid) or 1 (errors found).

---

## 11. macOS UI Features (v1)

### 11.1 Accessibility Permission & Event Tap Management

**Permission check on launch:**

```swift
let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue(): true] as CFDictionary
if AXIsProcessTrustedWithOptions(options) {
    registerEventTap()
} else {
    enterManualMode()  // "Clean Clipboard Now" in menu, warning badge on icon
}
```

**Input Monitoring (macOS Ventura+):** Test on clean macOS 13/14/15 installs whether CGEventTap requires Input Monitoring consent in addition to Accessibility. If so, check and prompt for both. See §2.3 for the permission matrix.

**Periodic re-check (every 30 seconds):**
```swift
Timer.scheduledTimer(withTimeInterval: 30, repeats: true) { _ in
    let trusted = AXIsProcessTrusted()  // no prompt
    if trusted && !self.eventTapActive { self.registerEventTap() }
    if !trusted && self.eventTapActive { self.enterManualMode() }
}
```

**Tap disable recovery:** macOS can disable a CGEventTap if the callback takes too long or under system pressure. Handle this in the event tap callback:

```swift
if event.type == .tapDisabledByTimeout || event.type == .tapDisabledByUserInput {
    CGEvent.tapEnable(tap: eventTap, enable: true)
    NSLog("ccvv: event tap was disabled, re-enabled")
    return Unmanaged.passUnretained(event)
}
```

If re-enable fails, update the status icon to error state and log the failure.

Add to `Info.plist`:
```xml
<key>NSAccessibilityUsageDescription</key>
<string>ccvv needs Accessibility access to detect your double-tap copy shortcut. Without it, you can still clean the clipboard manually from the menu bar.</string>
```

### 11.2 Pause/Resume

- `var isPaused: Bool = false` on AppDelegate.
- Menu item: "Pause ccvv" / "Resume ccvv" (title toggles).
- Guard at top of `handleCGKeyEvent`: `if isPaused { return }`.
- When paused, icon changes to `[--]` with `.secondaryLabelColor`.
- Pause state is transient (resets on app launch).

### 11.3 Icon States

Use **`NSImage`-based status items with template images**, not text. Template images (18×18pt, PDF or SF Symbols) automatically adapt to Light/Dark mode and are the standard approach for shipping menu bar apps. Text-based status items (`[cc]`, `[--]`) are fragile across macOS versions: spacing changed in macOS 14, baselines shift between releases, different character widths cause visual jumps, and wider items are hidden sooner when the menu bar is crowded (especially on notched laptops).

| State | Image | Notes |
|-------|-------|-------|
| Active (listening) | `ccvv-active` template (two overlapping "C" letters) | Default state |
| Paused | `ccvv-paused` template (pause bars) | Toggled via menu |
| Success flash | `ccvv-success` template (checkmark) | Swap image, revert after 300ms |
| Miss (near-miss) | `ccvv-active` with highlight | `button?.highlight(true)` for 200ms, then `highlight(false)` |
| No Accessibility | `ccvv-warning` template (exclamation) | Shown when permission denied or revoked |

Images are included in the app bundle as `Assets.xcassets` (or loose PDFs in Resources). All images are marked `isTemplate = true` so macOS handles appearance adaptation.

### 11.4 Miss Indicator

In `handleCGKeyEvent`, when `elapsed > doubleTapWindow && elapsed < doubleTapWindow * 2.0`:

```swift
func showMissIndicator() {
    guard let button = statusItem.button else { return }
    // Use highlight instead of layer animation — NSStatusBarButton does not
    // guarantee a backing layer, and layer animations fail silently on
    // some macOS versions.
    button.highlight(true)
    DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) {
        button.highlight(false)
    }
}
```

**Why not CAKeyframeAnimation:** `NSStatusBarButton` does not guarantee a backing layer. Even with `wantsLayer = true`, the system redraws the button on its own schedule, which can override or conflict with custom animations. The highlight approach works reliably across macOS versions.

### 11.5 HUD Toast

Borderless floating window near cursor:

```swift
let toast = NSWindow(
    contentRect: NSRect(x: 0, y: 0, width: 320, height: 48),
    styleMask: [.borderless],
    backing: .buffered,
    defer: false
)
toast.isOpaque = false
toast.backgroundColor = NSColor.black.withAlphaComponent(0.8)
toast.level = .statusBar           // .floating won't appear above full-screen apps
toast.ignoresMouseEvents = true
toast.hasShadow = true
toast.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
```

**Positioning:** 20pt below and right of `NSEvent.mouseLocation`, clamped to the **active screen's `visibleFrame`** (accounting for menu bar, Dock, and screen edges). Multi-display and mixed-scale setups require converting `NSEvent.mouseLocation` (global coordinates) to the correct screen's coordinate space:

```swift
let mouseLocation = NSEvent.mouseLocation
let screen = NSScreen.screens.first(where: { NSMouseInRect(mouseLocation, $0.frame, false) })
    ?? NSScreen.main ?? NSScreen.screens[0]
let visibleFrame = screen.visibleFrame
var origin = CGPoint(x: mouseLocation.x + 20, y: mouseLocation.y - 68)
// Clamp to visible frame
origin.x = min(origin.x, visibleFrame.maxX - toast.frame.width)
origin.y = max(origin.y, visibleFrame.minY)
toast.setFrameOrigin(origin)
```

Contains a single `NSTextField` with the summary string in white, 12pt system font.

**Sensitive/oversize feedback:** If `skippedSensitive`, show "Skipped: looks like a secret". If `skippedOversize`, show "Skipped: content too large ({size})".

**Stage Manager / Spaces:** `collectionBehavior` includes `.canJoinAllSpaces` and `.stationary` to ensure the toast appears on the current Space and does not persist when switching Stages.

Auto-dismiss via `NSAnimationContext` fade-out.

### 11.6 Confidence Mode

Track `transformCount` in `UserDefaults`. Toast duration schedule:

| Transform count | Toast duration |
|-----------------|----------------|
| 0–29 | 1.5s |
| 30–39 | 1.0s |
| 40–49 | 0.5s |
| 50+ | Toast disabled (icon flash only) |

User can override: "Always show toast" or "Never show toast" in Preferences.

### 11.7 Bypass Modifier

Already implemented in current code (line 621: `!flags.contains(.maskAlternate)`). Option key held during Cmd+C causes the event tap callback to return early. No additional work needed beyond verification.

### 11.8 Preferences Window

Single `NSWindow` with `NSStackView` containing sections:

```
┌─ Text Cleanup ──────────────────────────────────────┐
│  [x] Whitespace & line break cleanup                │
│  [x] Unicode normalization                          │
│  [x] Agent artifact stripping                       │
├─ Formatting ────────────────────────────────────────┤
│  [x] JSON detect & prettify                         │
│  [x] Table-to-Markdown                              │
│  [x] Code fence wrapping                            │
│  [ ] Backtick auto-wrapper (shell safety warning)   │
├─ URLs ──────────────────────────────────────────────┤
│  [x] Strip tracking parameters                      │
│  [ ] Strip URL scheme (aggressive)                  │
├─ Privacy ───────────────────────────────────────────┤
│  [x] Sensitive content filter                       │
│  [ ] Store raw clipboard in history                 │
├─ Feedback ──────────────────────────────────────────┤
│  [x] Show HUD toast                                 │
├─────────────────────────────────────────────────────┤
│  [Open Config File]   [Reset to Defaults]            │
└─────────────────────────────────────────────────────┘
```

Toggle changes are written back to the TOML config file using the atomic write protocol (§6.5).

**Config write-back uses `toml_edit`** (not `toml`) to preserve comments and formatting. The `toml` crate loses all comments and non-default formatting on round-trip (deserialize → modify → serialize). `toml_edit` parses TOML into a document tree that preserves comments, inline tables, and whitespace. The write flow is:
1. Read existing config as a `toml_edit::DocumentMut`.
2. Modify the specific key that changed.
3. Serialize back to string (comments and formatting preserved).
4. Write via atomic protocol (temp → fsync → rename).

**Race condition:** If the user edits the config in a text editor while Preferences is open, one will overwrite the other. Mitigation: before writing, compare the file's mtime with the last-read mtime. If changed, reload the file, re-apply the GUI change, and write. This does not guarantee correctness under all races but handles the common case.

If no config file exists, one is created at the default location with only the changed values (not a full dump of defaults).

### 11.9 History Panel

**Use `NSPopover`** attached to the status item button, NOT a manually positioned `NSPanel`. `NSPopover` handles positioning, screen edge avoidance, and display configuration changes automatically. Getting the screen position of a status item via `button?.window?.frame` has been unreliable since macOS 11 and worsened in macOS 14 (stale or zero-origin values after menu bar auto-hide or display changes).

```swift
let popover = NSPopover()
popover.contentSize = NSSize(width: 360, height: 400)
popover.behavior = .transient  // dismiss on click outside
popover.contentViewController = HistoryViewController()
popover.show(relativeTo: statusItem.button!.bounds,
             of: statusItem.button!,
             preferredEdge: .minY)
```

`HistoryViewController` contains an `NSTableView` with columns:
- Preview (truncated first 80 chars)
- Type badge (colored label: URL, Code, Prose, Table, JSON)
- Relative timestamp ("2m ago", "1h ago")

`NSSearchField` at top for substring filtering. Click on entry: restore cleaned text to clipboard (or raw text if stored), dismiss popover.

Data is fetched from Rust via `ccvv_history_get_recent_json()`, parsed as JSON in Swift.

### 11.10 First-Run Onboarding

On first launch (`UserDefaults.bool(forKey: "ccvv_onboarding_done") == false`):

**Sequencing with Accessibility prompt:** The Accessibility permission system dialog appears during `applicationDidFinishLaunching` (§11.1). The onboarding overlay must appear AFTER the system dialog is dismissed. Use a delay or observe `NSApplication.didBecomeActiveNotification` to detect when the app regains focus after the system dialog.

If Accessibility permission is NOT yet granted when onboarding appears, the "Try it now" instruction won't work (the event tap isn't registered). In this case, the onboarding shows:
> *Grant Accessibility access, then copy something and tap ⌘C again within a beat.*

If Accessibility IS granted:
> *Copy something, then tap ⌘C again within a beat. Watch the icon flash. That's it.*

Include "Try it now" and "Dismiss" buttons. When a successful double-tap is detected (via `NotificationCenter.default.post`), auto-dismiss and set the flag.

**Product decision notice:** The onboarding includes: "ccvv cleans your clipboard to plain text. Rich formatting will be removed." This sets expectations for users who may otherwise perceive plain-text output as a bug.

**Privacy notice:** "ccvv never connects to the internet. Your clipboard stays on your device." If `history_store_raw` is false (default), also: "Clipboard history stores only cleaned text, not originals."

### 11.11 Adaptive Double-Tap Timing

Implemented in Rust (shared with future platforms). FFI functions:

```c
void ccvv_timing_record_sample(uint32_t interval_ms);
uint32_t ccvv_timing_get_threshold_ms(void);
```

**Thread safety:** The timing module uses internal `Mutex<CircularBuffer>` for the sample buffer. Both functions are safe to call from any thread. This is necessary because `ccvv_timing_record_sample` may be called from the event tap callback (background runloop) while `ccvv_timing_get_threshold_ms` is called from the main thread.

Algorithm: Maintain a circular buffer of the last 20 confirmed double-tap intervals. After ≥ 20 samples, compute `threshold = median + 1.5 × IQR`, clamped to [150, 500]ms. Before 20 samples, return the config default (300ms).

Data is persisted to `~/Library/Application Support/ccvv/timing.json` on macOS. If corrupted or deleted, threshold resets to the config default (300ms). This is expected graceful degradation, not an error.

Swift calls `ccvv_timing_record_sample()` after each successful double-tap in `handleCGKeyEvent`. Reads `ccvv_timing_get_threshold_ms()` to set the active window. If the config specifies a fixed value (not "auto"), the config value takes precedence and the adaptive system is inactive.

---

## 12. Error Handling

### 12.1 Rust Error Type

```rust
#[derive(Debug, thiserror::Error)]
pub enum CcvvError {
    #[error("config: {0}")]
    Config(String),

    #[error("invalid regex in rule '{name}': {source}")]
    Regex {
        name: String,
        source: regex::Error,
    },

    #[error("database: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("IO: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("config integrity: {0}")]
    ConfigIntegrity(String),

    #[error("database corrupt: {0}")]
    DatabaseCorrupt(String),

    #[error("input too large: {size} bytes (limit: {limit})")]
    InputTooLarge { size: usize, limit: usize },
}
```

### 12.2 FFI Error Propagation

FFI functions that can fail accept an `error_out: *mut *mut c_char` parameter. On failure, the function sets `*error_out` to a heap-allocated C string describing the error. On success, `*error_out` is set to NULL. The caller frees the error string with `ccvv_string_free()`.

```rust
fn set_error_out(error_out: *mut *mut c_char, err: CcvvError) {
    if !error_out.is_null() {
        let msg = CString::new(err.to_string()).unwrap_or_default();
        unsafe { *error_out = msg.into_raw(); }
    }
}

fn clear_error_out(error_out: *mut *mut c_char) {
    if !error_out.is_null() {
        unsafe { *error_out = std::ptr::null_mut(); }
    }
}
```

This replaces the v1.0 thread-local `ccvv_last_error()` approach. The `error_out` pattern is safe with Swift concurrency because each call site has its own error pointer, regardless of which thread or dispatch queue the call executes on.

### 12.3 Swift Error Handling

The Swift wrapper (`CcvvCore`) treats all FFI failures as non-fatal. If `ccvv_transform` returns an empty result, the app falls back to the raw clipboard text and shows a failure indicator. The app never crashes due to a Rust error. All errors are logged via `NSLog` for diagnostic purposes.

### 12.4 Transform Pipeline Errors

Individual stages do not fail — they degrade. If a stage encounters unexpected input, it returns the input unchanged (pass-through). The pipeline always produces output. Errors within stages are logged to `TransformContext.rules_fired` as diagnostic entries, not as exceptions.

**Exception:** The sensitive content filter and input size check run before stages and can cause the pipeline to return the input unchanged (not an error — a deliberate safety bypass).

---

## 13. Performance Budget

| Metric | Target | Enforcement |
|--------|--------|-------------|
| Binary size (macOS app + Rust lib) | < 5 MB | `cargo bloat` in CI |
| Memory RSS (idle) | < 15 MB | Instrument with `leaks` and `vmmap` |
| Memory RSS (transforming) | < 25 MB | Transform allocates proportional to input size |
| Per-transform memory ceiling | 10× input size | Abort transform if exceeded |
| Idle CPU wakeups | < 1/sec | macOS `NSPasteboard` change-count polling at ≤ 2 Hz |
| Transform latency (10 KB input) | < 50ms | Benchmark with `criterion` |
| Transform latency (100 KB input) | < 200ms | Benchmark with `criterion` |
| Cold start to icon visible | < 200ms | Measure with `os_signpost` |
| Config load | < 10ms | Benchmark with `criterion` |
| Input size ceiling | 1 MB (configurable) | Content above ceiling passes through |
| Transform rate limit | 1 per 100ms | Subsequent triggers dropped |

The Rust library must never allocate on the hot path except proportional to input size. Regex compilation happens at config load time. SQLite is opened once and held for the process lifetime.

---

## 14. Testing Strategy

### 14.1 Unit Tests

Each transform module has a `#[cfg(test)] mod tests` block:

- **`normalize.rs`**: Every character replacement independently. Mojibake detection with known patterns. NFC normalization. Already-normalized text passes through unchanged.
- **`whitespace.rs`**: Port known input/output pairs from the Swift implementation. Hard-wrapped paragraph unwrapping. List preservation. Code fence handling. Terminal width detection. Excessive padding. Mixed content.
- **`agent.rs`**: ANSI codes (colors, cursor, reset). Zero-width removal. `⏺` with various trailing whitespace.
- **`structural.rs`**: Valid JSON prettification. Invalid JSON passthrough. TSV/CSV table detection. Code fence wrapping with language hints. Ambiguous content passthrough. JSON recursion limit. Table output size guard.
- **`url.rs`**: UTM stripping. Per-domain overrides. Glob matching in deny lists. URLs in prose. URLs inside code fences (must be skipped). Aggressive mode.
- **`autowrap.rs`**: Port all heuristic cases from Swift's `shouldWrapCodeToken`. File paths, camelCase, SCREAMING_CASE, CLI flags, filenames. Already-backticked tokens skipped. Code fences skipped. URLs skipped.
- **`userrules.rs`**: Capture group replacement. Empty replacement. Multiple rules in sequence. Rules that match nothing. Rule timeout behavior.
- **`secrets.rs`**: Each pattern individually. Content with multiple patterns. Content that does not match. Edge cases (short base64, non-key prefixes).
- **`config.rs`**: Valid config. Missing config (defaults). Profile overlay. Invalid regex (error). DoubleTapSetting parsing. Merge semantics (exclusions appended, not replaced). Config integrity warnings.
- **`history.rs`**: Add, retrieve, search, filter. 50-entry pruning. Empty database. Content classification. Two-phase commit/rollback. Uncommitted cleanup. Ring buffer. WAL mode verification. Corruption handling.

### 14.2 Integration Tests

In `ccvv-lib/tests/`:

- **`pipeline_idempotency.rs`**: For every test fixture, run the pipeline twice and assert output equality.
- **`pipeline_regression.rs`**: Corpus of real-world clipboard samples paired with expected outputs. `(input, expected)` pairs. New regressions are added as issues are discovered.
- **`pipeline_size_limits.rs`**: Verify behavior with 1 MB, 5 MB, and 50 MB inputs. Assert pass-through for oversize, memory stays bounded.
- **`pipeline_sensitive.rs`**: Verify sensitive content filter triggers for each pattern category. Verify it can be disabled via config.
- **`config_profiles.rs`**: Load test config files from `tests/fixtures/`, build pipeline with different profiles, verify behavior changes.
- **`config_merge.rs`**: Verify that partial user configs correctly overlay onto defaults without wiping exclusions or deny lists.

### 14.3 FFI Tests

In `ccvv-lib/tests/`:

- Round-trip: `ccvv_load_config` → `ccvv_transform` → `ccvv_string_free`. Verify output matches Rust API.
- Null pointers: Every FFI function called with NULL. No crashes.
- Error-out: Verify error strings are populated on failures and NULL on success.
- Two-phase commit: `prepare` → `commit` and `prepare` → `rollback` sequences.
- Memory: Run under AddressSanitizer (`RUSTFLAGS="-Z sanitizer=address" cargo test`).

### 14.4 CLI Tests

In `ccvv-cli/tests/`:

- Invoke binary with `std::process::Command`. Pipe known input, compare stdout.
- Test `--strip-urls`, `--flatten-json`, `--unwrap` individually.
- Test `preview` subcommand output format.
- Test `validate` with valid and invalid config files.
- Test `history` subcommand after populating via API.
- Test `doctor` output includes permission and backup exclusion checks.

### 14.5 Swift Behavior Validation

Before deleting the old Swift transform functions:
1. Collect a corpus of clipboard inputs (manually or via logging).
2. Run each through both the Swift `ccvv()` and the Rust `ccvv_transform()`.
3. Assert output equality. Any discrepancies must be investigated and resolved before proceeding.

### 14.6 Security Tests

- **Config integrity:** Create config files with wrong owner, group-writable, world-readable permissions. Verify warnings are produced.
- **Atomic write:** Kill the preferences write process mid-write (simulate crash). Verify config file is either the old version or the new version, never partial.
- **Unparseable config:** Create a config file with invalid TOML. Verify app shows warning badge and does NOT silently fall back to defaults.
- **Config merge semantics:** Create a config with `exclusions.apps = ["com.my.app"]`. Verify default password manager exclusions are still present (appended, not replaced).
- **History permissions:** Verify database file and directory are created with correct permissions (0600 / 0700).
- **Backup exclusion:** Verify `NSURLIsExcludedFromBackupKey` is set on the data directory. Verify `.metadata_never_index` exists.
- **Sensitive filter:** Verify that PEM keys, JWTs, AWS keys, GitHub tokens are not stored in history (even cleaned_text is redacted).
- **Two-phase commit:** Simulate crash after history prepare but before clipboard write. Verify clipboard unchanged and uncommitted entry cleaned up on restart.
- **Network isolation:** Verify sandbox entitlements deny network. Run app under `nettop` and verify zero socket activity.
- **Runtime integrity:** Verify code signature check runs on launch and logs warning if signature is invalid.
- **Auto-wrapper default:** Verify auto-wrapper is disabled by default in a fresh install with no config file.
- **Zeroize:** Verify that evicted ring buffer entries have their memory zeroed (debug-mode test with address inspection).

### 14.7 Fuzz Testing

- **Transform pipeline:** Use `cargo-fuzz` with arbitrary byte input. Assert no panics, no unbounded memory growth.
- **Config parser:** Fuzz TOML input. Assert no panics, graceful error returns.
- **URL parser:** Fuzz URL-like strings through Stage 6. Assert no panics.
- **JSON parser:** Fuzz JSON-like strings through Stage 5. Assert no panics, bounded memory.

---

## 15. Security Considerations

### 15.1 Threat Model Summary

ccvv operates as a privileged clipboard processor with system-wide keyboard event access. This makes it a high-value target and a potential liability. The security architecture is designed around three principles:

1. **Minimize what is stored.** By default, raw clipboard content is never persisted to disk. History stores only cleaned text (and even that is redacted for secret-like content).
2. **Minimize what is accessible.** File permissions, backup exclusion, and data directory protection reduce the attack surface for same-user and backup-access adversaries.
3. **Enforce constraints technically, not just by policy.** Network isolation via sandbox entitlements, dependency bans via `cargo deny`, and permission checks on config files.

### 15.2 Network Isolation

See §2.6. Enforced via:
- `cargo deny` banning network-capable crates.
- `cargo audit` checking for known vulnerabilities.
- Runtime assertion (debug builds) verifying no open sockets.
- Documented platform-initiated connections (OCSP, notarization, Homebrew analytics) that are outside ccvv's control.

### 15.3 Clipboard Data Protection

- **No persistence of raw content by default** (§7.1).
- **Sensitive content filter** prevents transforming and storing secrets (§5.5).
- **App exclusion list** prevents intercepting clipboard from password managers (§6.3 merge semantics ensure defaults are not accidentally removed).
- **In-memory ring buffer** provides undo without disk persistence of raw content (§7.7).

### 15.4 Config Integrity

- **Permission checks** on config file (§6.5).
- **Atomic writes** prevent partial config corruption (§6.5).
- **Merge semantics** prevent accidental removal of safety defaults (§6.3).
- **User rule limits** bound the attack surface of regex injection (§5.4, Stage 8).
- The `regex` crate's linear-time guarantee eliminates ReDoS, but regex rules remain an integrity attack surface if an adversary can modify the config file. This is mitigated by permission checks and documented as an accepted risk for same-user adversaries (who already have full clipboard access anyway).

### 15.5 Crash Consistency

- **Two-phase commit** for clipboard/history operations (§9.2).
- **Uncommitted entry cleanup** on startup (§7.2).
- **In-memory ring buffer** survives database failures (§7.7).
- **Defined invariants** (§2.5) that hold across all crash scenarios.

### 15.6 Code Signing and Distribution

- Developer ID code signing with Hardened Runtime.
- Notarization via `notarytool`. Notarization + stapling must be verified end-to-end with quarantine attribute intact (do not rely on Homebrew stripping quarantine).
- Homebrew cask distribution with SHA-256 checksum verification.
- **Homebrew quarantine policy:** Homebrew currently strips the quarantine xattr for cask installs, but this policy may change. The app must work correctly when Gatekeeper's "downloaded from the internet" confirmation appears. Verify in `brew-local-test.sh` with quarantine intact.
- **Rust staticlib + notarization:** Apple's notarization service performs binary analysis. Rust's standard library statically linked into a Swift binary is not a problem in practice. Rust panic unwinding code may produce cosmetic warnings in the notarization log but does not cause rejection.
- The build pipeline (build.sh) is committed to the repository. No external CI/CD yet — builds are local. Supply-chain risk for dependencies is mitigated by `cargo deny` (advisory database, license check, crate bans) and `cargo audit`.

### 15.7 Accessibility Scope

See §2.3 for the full trust boundary definition. Key constraints:
- Only `kCGEventKeyDown` / `kCGEventKeyUp` events observed.
- Only keycode and modifier flags inspected.
- No keystroke logging, no app identity correlation (except exclusion check).
- Passive tap — no event modification.
- Graceful degradation to manual mode if permission denied.

### 15.8 Accepted Risks

| Risk | Severity | Acceptance rationale |
|------|----------|---------------------|
| Same-user malware can read history DB | High | Same-user malware already has full clipboard access via NSPasteboard. History DB does not increase exposure beyond what the adversary already has. Mitigated by not storing raw text and redacting sensitive cleaned text. |
| Config tampering by same-user process | High | Same-user processes can already modify any user-owned file. Permission checks provide detection, not prevention. |
| Accessibility permission as privilege escalation target | High | Fundamental constraint of macOS — no way to request "only CGEventTap." Mitigated by code signing, runtime integrity check, dependency auditing, and honest documentation of the risk. |
| Downstream clipboard monitors see cleaned content | Medium | Any clipboard manager can read NSPasteboard. ccvv cannot prevent this. Cleaning may make content slightly more parseable, but this is inherent to the product's purpose. |
| Transform stages may corrupt content in edge cases | Medium | Stage-specific corruption risks are documented (§5.4). The bypass modifier (Option key), app exclusion list, sensitive filter, and `ccvv preview` command provide multiple escape hatches. |
| URL `www.` stripping may break some URLs | Low-Medium | Applied unconditionally during URL cleaning. Documented risk. Users can disable URL cleaning or add per-domain overrides. |
| `pb.setString` destroys all non-text pasteboard types | Medium | Intentional ("Blind Paste" — sanitize to plain text). Mitigated by pasteboard type check that skips content without `.string` type. Users who copy rich text will lose formatting. Documented in onboarding. |
| v1 only detects Cmd+C (not menu/right-click/programmatic copies) | Low | Functional spec's "any clipboard write" claim is incorrect under CGEventTap architecture. Documented limitation. Other copy methods can use manual "Clean Clipboard Now" from menu. |

### 15.9 Known Limitations

| Limitation | Status | Plan |
|------------|--------|------|
| No at-rest encryption for history DB (SQLCipher) | Deferred to v2 | Raw text not stored by default; sensitive cleaned text is redacted. Risk is acceptable for v1. |
| `zeroize` does not protect against kernel memory dumps | Known | Standard limitation of user-space memory clearing. Kernel-level memory forensics are outside the threat model. |
| Secure memory only covers `UndoEntry` ring buffer | Known | Rust `String` allocations in the transform pipeline are not zeroized (would require custom allocator). Pipeline strings are short-lived and overwritten quickly. |
| Homoglyph characters can survive the pipeline | Known | Unicode normalization (NFC) does not resolve all homoglyphs. A crafted clipboard payload with homoglyphs (e.g., Cyrillic `а` vs Latin `a`) will pass through. This is a content-integrity risk but not exploitable within ccvv itself. |

---

## 16. Implementation Phases

### Phase 1: Rust scaffold + whitespace port
Create workspace. Implement `Transform` trait, `Pipeline` (with size limits), `config.rs` (defaults only), `error.rs`, `secrets.rs`. Port `ccvv()` to `whitespace.rs`. Port token wrapping to `autowrap.rs`. Port `⏺` stripping to `agent.rs`. Unit tests.

**Deliverable:** `cargo test` passes. Transform output matches Swift.

### Phase 2: Wire Rust into Swift build
Add FFI layer (with `error_out` parameters). Create entitlements file. Update `build.sh`. Swift wrapper class. Replace `ccvv()` call in `performClean()` with FFI call. Verify sandbox + Accessibility interaction.

**Deliverable:** App builds and runs. Clipboard cleaning identical to before.

### Phase 3: New transform stages
`normalize.rs`, `url.rs`, `structural.rs`, `userrules.rs`. All with unit tests and idempotency checks. Add sensitive content filter.

**Deliverable:** `cargo test`. Full pipeline handles unicode, URLs, JSON, tables, code fences. Sensitive content bypassed.

### Phase 4: Config system
Full TOML parsing, profile resolution, regex compilation, merge semantics, integrity checks, atomic writes. FFI exposure. Wire into Swift.

**Deliverable:** Config file controls feature toggles and double-tap window. Merge semantics verified. Atomic write tested.

### Phase 5: History database
SQLite implementation with WAL, two-phase commit, ring buffer, corruption handling, permission enforcement, backup exclusion. FFI exposure. Wire into Swift.

**Deliverable:** History DB populates on transforms. Two-phase commit verified. Backup exclusion verified. Ring buffer undo works.

### Phase 6: CLI companion
`ccvv-cli` binary with all subcommands. Doctor includes security checks.

**Deliverable:** `echo "text" | ccvv`, `ccvv preview`, `ccvv history`, `ccvv doctor`, `ccvv validate`.

### Phase 7: macOS UI features
Template-image status items, Accessibility prompt with fallback and periodic re-check, tap-disable recovery, pause/resume, miss indicator (highlight-based), HUD toast (with correct positioning/collectionBehavior/sensitive feedback), confidence mode, preferences (with `toml_edit` write-back and privacy section), history popover (NSPopover), onboarding (sequenced with Accessibility prompt, "plain text" product message), adaptive timing (with Mutex).

**Deliverable:** All functional spec UI features working. Tested on clean macOS 13/14/15 installs.

### Phase 8: Security hardening + release
Network isolation verification. Fuzz testing. Permission tests. Config integrity tests. Runtime self-integrity check. Clean-machine signing/notarization/Gatekeeper test (with quarantine intact). Delete old Swift transform code. Update `brew-local-test.sh`. Final integration testing. Verify auto-wrapper defaults to off. Verify unparseable config behavior.

**Deliverable:** Clean codebase. All security tests pass. `brew install` works on a clean machine.

---

## 17. v1 Scope Exclusions (Deferred to v2)

| Feature | Reason for deferral |
|---------|---------------------|
| Smart Paste (context-aware destination formatting) | Architecturally complex, fragile window detection |
| "Paste As" chooser palette | Depends on Smart Paste infrastructure |
| Linux daemon (X11/Wayland tray app) | Platform scope is macOS-deep for v1 |
| Windows daemon (Win32 tray app) | Platform scope is macOS-deep for v1 |
| Invisible mode (hide icon, haptic/screen-edge feedback) | Nice-to-have, not core |
| Homebrew formula (build from source) | Cask (pre-built binary) is sufficient for v1 |
| Universal binary (arm64 + x86_64) | Build for native arch in v1, add lipo in CI later |
| SQLCipher / at-rest encryption for history DB | Evaluated for v1 but deferred — raw text not stored by default, reducing need. Revisit if `history_store_raw` becomes popular. |
| Full-disk sandbox (restrict file access to data dir only) | App Sandbox with current entitlements provides partial sandboxing. Full restriction may conflict with config file in `~/.config/`. Revisit with hardened sandbox profile in v2. |
| `cargo vet` for high-trust dependency auditing | `cargo audit` + `cargo deny` provide baseline supply-chain protection. `cargo vet` adds manual review tracking — valuable but requires ongoing maintenance. Evaluate for v2. |
| Reproducible builds | Builds are currently local (build.sh). Reproducible builds require deterministic toolchain and CI infrastructure. Plan for v2 release pipeline. |
| Homoglyph detection in transform pipeline | NFC normalization does not catch cross-script homoglyphs. A dedicated confusable-detection stage is complex and may produce false positives. Evaluate for v2. |
| Non-keyboard copy detection (right-click, Edit menu, programmatic) | v1 detects Cmd+C only via CGEventTap. Clipboard-write polling for other copy methods would require high-frequency polling (≥10 Hz) that exceeds the idle CPU budget. Evaluate NSPasteboard polling as secondary detection for v2. |
| Rich text preservation on write-back | v1 writes `.string` only (plain text). Preserving RTF/HTML types while modifying text content is complex. Evaluate for v2. |
| Universal binary (arm64 + x86_64) in cask | Build for native arch in v1. If Intel users need support, add `lipo` in CI. Intel Mac market share is small enough (March 2026) that this may not be needed. |
| Forward-compatibility harness for annual macOS menu bar changes | v1 uses standard APIs (template images, NSPopover, standard window levels) to minimize breakage. No automated forward-compat testing yet. Evaluate annual regression test suite for v2. |
