# Technical Review: ccvv v1.0 Specification (Consolidated)

**Reviewer posture:** Senior Rust systems engineer, 8+ years, extensive FFI (Rust↔C↔Swift), cross-compilation on Apple silicon, production staticlib shipping.

**Note:** This review consolidates two independent reviews. Where both identified the same issue, the stronger analysis is kept. Where they diverged, the divergence is noted.

---

## Critical (Must Fix Before Implementation)

### 1. FFI error strategy (`ccvv_last_error()` TLS) is broken in Swift's threading model

The implementation stores a `CString` in thread-local `RefCell<Option<CString>>` and returns `s.as_ptr()` from inside a `with` borrow. The spec says "Returned string is valid until the next FFI call" — this is only accidentally true when calls are single-threaded.

The real killer: Swift's Grand Central Dispatch (GCD) will dispatch work across threads. If `ccvv_transform` runs on thread A and sets the error, then Swift calls `ccvv_last_error()` from thread B (the main thread, a background queue, anything touched by async/await), it gets `NULL` — the error is silently lost. The spec *itself* acknowledges this risk by requiring `Send + Sync` "because the FFI layer may invoke from a non-Rust thread," which makes TLS-based errors actively hostile.

**Fix:** Return errors in-band. Three options in order of preference:

- **Option A (cleanest):** Out-parameters: `bool ccvv_transform(const uint8_t* ptr, size_t len, const CcvvConfig*, CcvvTransformResult* out, char** err_out)`
- **Option B (easy Swift import):** Fat result struct: `CcvvResult { uint32_t ok; CcvvTransformResult result; char* error; }`
- **Option C (best ABI stability):** Numeric error code return + `const char* ccvv_error_message(uint32_t code)` for stable, statically-allocated strings

Any of these eliminates the thread-safety hazard entirely. Option A also solves Critical #4 (struct return by value) simultaneously.

### 2. Memory ownership contract is ambiguous / contradictory

The spec provides both:
- `ccvv_transform_result_free(CcvvTransformResult result)` — implying one-shot cleanup
- Section 7.4 table listing `cleaned_text` and `summary` individually as "free with `ccvv_string_free()`"

These are contradictory. The Swift wrapper (Section 8.3) calls only `ccvv_transform_result_free` in a `defer` block and never calls `ccvv_string_free` on individual fields. So the *intent* is clearly that `ccvv_transform_result_free` frees everything inside. But the ownership table says otherwise. Callers reading only the table will double-free or leak.

**Fix:** Choose one contract and kill the other:

- **Preferred:** `ccvv_transform_result_free` frees *everything inside* the struct (both pointers). Change the signature to accept a pointer (`void ccvv_transform_result_free(CcvvTransformResult* r)`) so the implementation can zero the fields after freeing, preventing use-after-free. Remove the individual pointer entries from the ownership table, or annotate them as "freed as part of `ccvv_transform_result_free`; do NOT free individually."
- Reserve `ccvv_string_free` for standalone returned strings only (history JSON, validate_config output).

### 3. Returning `CcvvTransformResult` by value is avoidable ABI risk

`CcvvTransformResult` is `8 + 8 + 4 = 20 bytes`, likely padded to 24 for alignment. On arm64-apple-darwin, structs ≤ 16 bytes are returned in registers; larger structs use a hidden `sret` pointer parameter. At 20+ bytes, this struct exceeds the register-return threshold. Swift and Rust must agree on the calling convention. If they disagree — silent memory corruption, not a compiler error.

Beyond correctness, returning by value costs you:
- No "NULL means error" sentinel
- No forward compatibility (adding a field breaks ABI)
- The free function receives a *copy* of the struct, so the caller retains dangling pointers

**Fix:** Return via out-parameter (see Critical #1, Option A) or return `*mut CcvvTransformResult`. This eliminates all ABI ambiguity and is the standard pattern for FFI struct returns for a reason.

### 4. `*const c_char` inputs silently truncate at embedded NUL bytes

Swift's `String.withCString` produces a null-terminated C string. If clipboard content contains an embedded null byte (rare but possible with binary-ish sources, some app bugs, copied protobuf dumps, mojibake), the Rust side constructs a `&str` from `CStr::from_ptr` which stops at the first null. Everything after the null is silently dropped with no way to detect it.

**Fix:** Accept `(*const u8, usize)` pairs for all FFI string inputs. Swift side passes `String.utf8` bytes + count (or `Data`). This also makes the API honest about what it receives. If you decide this is over-engineering for a clipboard tool (null bytes in clipboard text are vanishingly rare), at minimum document the truncation behavior and have the Swift wrapper pre-strip nulls before calling.

### 5. Performance budget contradicts itself: "< 1 wakeup/sec" vs. "polling ≤ 2 Hz"

Section 12's Performance Budget table says "Idle CPU wakeups: < 1/sec" and in the same row says "NSPasteboard change-count polling at ≤ 2 Hz." These are in direct contradiction — 2 Hz polling is 2 wakeups/second minimum.

The architectural confusion runs deeper: Section 1 says the macOS app uses `CGEventTap` (event-driven keyboard monitoring), the functional spec describes "two clipboard-write events in rapid succession" (implying clipboard polling), and the technical spec's `handleCGKeyEvent` (Section 8.2) implies keyboard-event-driven triggering. These are fundamentally different architectures:

- **Keyboard-driven (CGEventTap):** The app watches for Cmd+C keystrokes, acts on double-tap. Clipboard is only read *after* a keyboard event. Idle wakeups can truly be < 1/sec.
- **Clipboard-polling:** The app polls `NSPasteboard.changeCount` at 2 Hz to detect writes regardless of how they occurred (keyboard, menu, AppleScript, etc.). This is 2 wakeups/sec minimum.

**Fix:** Decide the architecture and make the budget consistent. If CGEventTap is the trigger and you only read the clipboard reactively, you don't need polling at all for v1 — and you get < 1 wakeup/sec honestly. If you need to detect non-keyboard clipboard writes, the budget must be ≥ 2.

### 6. The `swiftc` link line is missing required system libraries

The build script (Section 8.4) links with:
```
-framework Cocoa -framework ApplicationServices -lccvv_lib
```

But the Rust staticlib transitively requires:
- `rusqlite` (bundled) → links compiled SQLite C code, which needs `-lz` (if compression is enabled — the `bundled` feature may enable it)
- Rust `std` on macOS → needs `-lSystem`, `-lc`, `-lm`, `-liconv`, and `-framework Security` (for `SecRandomCopyBytes` used by the random number generator)
- Potentially `-lsqlite3` linker symbols even with bundled (depends on feature flags)

At minimum you're missing `-lz`, `-liconv`, and `-framework Security`. You'll discover this as confusing "undefined symbol" errors on the first link attempt.

**Fix:** Add a section to the build documentation that enumerates every `-l` and `-framework` flag required when linking the staticlib. Test the link line on a clean machine (not just your dev box where Xcode has everything). Alternatively, write a helper script that extracts the required flags from `cargo rustc --package ccvv-lib -- --print native-static-libs`.

---

## Significant (Should Fix; Will Cause Pain if Ignored)

### 1. The FFI surface, history layer, and core library should be separate crates

The spec puts FFI (`ffi.rs`), the transform engine, config parsing, and SQLite history all in `ccvv-lib`. This has multiple consequences:

- All `unsafe` FFI code lives alongside the pure, safe transform engine. An `unsafe` audit requires reviewing the entire crate.
- `ccvv-cli` depends on `ccvv-lib`, which builds `staticlib` + `cdylib` + `lib`. Cargo compiles all three on every build, tripling compile time for CLI-only development.
- `rusqlite` with `bundled` pulls a C compiler requirement into *every* consumer. The CLI doesn't need FFI; the FFI doesn't need to compile SQLite internals.

**Fix:** Split into three or four crates:

| Crate | Type | Contents | Depends on |
|-------|------|----------|------------|
| `ccvv-core` | `lib` | Pipeline, stages, config types, classification. Pure safe Rust. | regex, serde, toml, url, serde_json, unicode-normalization |
| `ccvv-history` | `lib` | SQLite schema, persistence, pruning. Isolates C toolchain pain. | rusqlite (bundled), ccvv-core |
| `ccvv-ffi` | `staticlib` + `cdylib` | Only `extern "C"`, pointer validation, CString allocation, error bridging. | ccvv-core, ccvv-history |
| `ccvv-cli` | `bin` | clap, stdin/stdout, preview, doctor. | ccvv-core, ccvv-history |

The CLI never compiles FFI artifacts. The FFI crate has a minimal, auditable surface. History's C-toolchain pain doesn't infect the core.

### 2. `TransformContext` as `&mut` flowing through every stage is hidden coupling

The spec explicitly says stages annotate the context and later stages read those annotations ("Stage 4 can annotate that ANSI codes were found, which Stage 5 uses as a signal the text originated from a terminal"). This is coupling by side-channel — stages become order-dependent in non-obvious ways, and "just add a flag" becomes the path of least resistance for every future feature.

**Fix:** Split the context into two structs:

- **`TransformSignals`** (immutable, decided up-front): content origin hints, active profile, source app bundle ID, detected content type from the classifier. Passed as `&self` to every stage.
- **`TransformReport`** (mutable, write-mostly): `Vec<RuleFired>`, character change counts, summary data. Stages push events but rarely read from it.

This makes the data flow explicit: stages read from signals and write to the report. The only cross-stage communication channel is the text itself (which is already explicit). If a stage genuinely needs to communicate downstream (e.g., "ANSI codes were found"), that information goes into `TransformSignals` during a pre-classification pass, not as a side-effect of stage execution.

### 3. Idempotency contract is not achievable as specified

The spec claims "enforced by design" across all stages. Here are concrete violations:

**Stage 7 → Stage 5 interaction (autowrap → structural detection):** On pass 1, Stage 7 wraps tokens in backticks (e.g., `config.yaml`). On pass 2, Stage 5's code-detection heuristic counts "percentage of lines starting with non-alphabetic character" — backtick-wrapped tokens change this metric. Structural detection can make a different classification decision on pass 2.

**Stage 3 → Stage 5 interaction (whitespace → structural detection):** Stage 5 emits Markdown pipe tables. On pass 2, Stage 3 runs *before* Stage 5 and its `compactParagraph` logic could attempt to join the header-separator row (`|---|---|`) with the first data row if heuristics misclassify the table as a paragraph block.

**Stage 6 → Stage 7 interaction (URL cleaning → autowrap):** If autowrap wraps a cleaned URL in backticks on pass 1, the URL detector may skip it on pass 2 ("already in backticks"), leaving a different cleaned/un-cleaned outcome.

**Stage 8 (user rules) is definitionally not idempotent.** Example: `s/foo/foobar/g` produces a new match on every pass. The spec handwaves this as "well-formed replacements are idempotent if they don't produce new matches" — that's a suggestion, not a contract.

**Fix:** Weaken the contract honestly:
- "The built-in pipeline (stages 2–7) is *designed* to be idempotent. Integration tests verify this property against a comprehensive fixture corpus."
- "User-defined rules (stage 8) may break idempotency. This is the user's responsibility."
- Add targeted cross-stage interaction tests (not just "run twice and compare" — specifically test the interactions enumerated above).

### 4. RFC 4180 CSV parsing in table detection cannot be done with `regex`

The spec says table detection should "respect RFC 4180 quoted fields" when splitting by delimiter. RFC 4180 quoted fields can contain the delimiter character, embedded newlines, and escaped quotes (`""`). This requires a stateful scanner that tracks whether you're inside a quoted field. The `regex` crate cannot handle this.

More broadly, the spec uses `regex` for everything: ANSI stripping, URL detection, user rules, token heuristics, and table detection. Some of these are better served by hand-rolled scanners:

- **ANSI stripping:** A simple byte scanner handling `\x1b[` sequences is faster and easier to reason about than a compiled regex. It's ~20 lines of Rust.
- **"Already-in-backticks" detection** (autowrap): You'll end up writing regex that approximates a tokenizer and then debugging edge cases indefinitely. A split-by-backtick approach (which the spec already describes) is a scanner, not a regex.
- **Table detection field splitting:** Needs a real (tiny) CSV field parser.

Keep `regex` for: user rules (that's the point), URL candidate scanning (the pattern is simple enough), and the `shouldWrapCodeToken` heuristics (where regex is actually a good fit).

**Fix:** Write a ~30-line hand-rolled CSV field splitter for table detection. Use a byte scanner for ANSI stripping. Reserve regex for cases where it's the natural tool.

### 5. `ccvv_history_get_recent_json` forces JSON serialization round-trip on the UI hot path

The history panel (Section 10.9) fetches data via `ccvv_history_get_recent_json`, which serializes entries to JSON in Rust, passes a `char*` over FFI, then deserializes JSON in Swift. For 50 entries with full `raw_text` and `cleaned_text`, this could be hundreds of KB of JSON allocated, serialized, copied, and parsed — just to show a scrollable list.

**Fix:** Expose a structured C API instead:

```c
// Opaque iterator
CcvvHistoryIter* ccvv_history_iter(const CcvvHistory* h, uint32_t count);
bool ccvv_history_iter_next(CcvvHistoryIter* iter, CcvvHistoryEntry* out);
void ccvv_history_iter_free(CcvvHistoryIter* iter);

// Or flat array
CcvvHistoryEntries ccvv_history_recent(const CcvvHistory* h, uint32_t count);
void ccvv_history_entries_free(CcvvHistoryEntries* entries);
```

JSON is fine for CLI output (`ccvv history --json`). It's wrong for a core UI bridge that runs on every panel open.

### 6. `cbindgen` is in `[build-dependencies]` but invoked from `build.sh` — these are different things

`cbindgen` in `[build-dependencies]` means Cargo compiles it from source on every clean build. cbindgen has a large dep tree (~80 crates including `syn`, `quote`, the full Rust parser). This adds 30–60 seconds to clean builds. But the spec invokes `cbindgen` as a CLI tool from `build.sh`, not from `build.rs`. The Cargo dependency accomplishes nothing except wasting compile time.

**Fix:** Remove `cbindgen` from `[build-dependencies]`. Either:
- Install as a CLI tool (`cargo install cbindgen --locked`) and invoke from `build.sh` (current approach, just fix the Cargo.toml)
- Create an `xtask` crate (`cargo xtask gen-headers`) that invokes cbindgen programmatically
- Or write a `build.rs` that *actually* calls cbindgen (valid pattern, but then remove the manual invocation from `build.sh`)

Pin the cbindgen version somewhere in the repo regardless of approach.

### 7. Universal binary deferral is probably not acceptable for a v1 Homebrew cask

The spec defers universal binary (arm64 + x86_64 via lipo) to "later." But if you distribute a prebuilt macOS app via Homebrew cask and want it to "just work," you need both architectures. Shipping two separate casks is messy. Org fleets still have significant Intel Mac populations.

**Fix:** Either include universal binary in v1 scope, or explicitly target arm64-only for v1 and document that Intel Mac users must build from source or use Rosetta 2 (and verify the app works correctly under Rosetta).

### 8. No maximum input size is specified

What happens when someone copies a 50 MB log file? The pipeline allocates 50 MB × 7 stages worth of strings, the JSON detection stage attempts `serde_json::from_str` on 50 MB (O(n) even on failure), and the table detection iterates every line. The 200 ms latency target is unachievable.

**Fix:** Add an input size cap (e.g., 512 KB or 1 MB). Above the threshold, either pass through raw or run only cheap stages (normalize, whitespace) and skip structural detection, JSON parsing, and regex-heavy stages. This also bounds your memory usage proportional to a known constant.

### 9. `rusqlite` with `bundled` complicates cross-compilation and CI

The `bundled` feature compiles the SQLite amalgamation from C source using the `cc` crate. Cross-compiling from arm64 → x86_64 (or vice versa) requires a C cross-compiler with correct env vars (`CC`, `AR`, `SDKROOT`, deployment target). On macOS with Xcode this usually works, but CI runners (GitHub Actions) may not have the right SDK paths configured.

Every `cargo build` also recompiles SQLite (~5–10 seconds), adding to the already-painful cbindgen compile time.

**Fix:** Document that Xcode command-line tools are required. Set up and test cross-compilation in CI during Phase 2 (not Phase 7+). If the crate split isolates history into `ccvv-history`, the C-toolchain pain is at least contained — `ccvv-core` and `ccvv-cli` (for basic transform-only usage) can build without a C compiler.

### 10. Allocation strategy ("new String per stage") should use `Cow<str>` for no-op stages

The spec dismisses allocation cost as "negligible." The math checks out for v1 workloads (7 × 100 KB ≈ 700 KB of memcpy, ~70 µs on Apple Silicon), so this isn't a correctness problem. But many stages will frequently be no-ops (agent stripping on text with no ANSI codes, URL cleaning on text with no URLs, user rules that match nothing). Allocating and copying 100 KB to return identical text is wasteful when a borrowed reference costs nothing.

**Fix:** Change the `Transform` trait to return `Cow<'_, str>`:

```rust
fn apply<'a>(&self, input: &'a str, ctx: &mut TransformContext) -> Cow<'a, str>;
```

No-op stages return `Cow::Borrowed(input)` (zero allocation). Stages that modify text return `Cow::Owned(new_string)`. The pipeline chains naturally because `Cow` derefs to `&str`. This is a small signature change with a meaningful win for the common case, and no downside for stages that do allocate.

---

## Minor (Nice to Fix; Won't Block)

### 1. `unicode-normalization` dependency may be unnecessary

Stage 2 already maps every problematic Unicode character explicitly (curly quotes, dashes, NBSP, zero-width chars, BOM). NFC normalization after this scan catches combining characters (e.g., `e` + combining acute → `é`). If you can't produce a concrete test case where NFC normalization fixes a real clipboard paste that the explicit map doesn't, drop the dependency. It pulls in ~30 KB of Unicode tables.

### 2. `ContentType::Mixed` variant is never assigned

`classify.rs` (Section 6.3) returns `Prose` as the default — no code path returns `Mixed`. Either add detection logic or remove the dead variant.

### 3. `DoubleTapSetting` with `#[serde(untagged)]` has a confusing failure mode

If someone writes `double_tap_window_ms = "300"` (string that looks like a number), serde tries `Fixed(u32)` first, fails, then succeeds with `Adaptive("300")`. The user gets adaptive timing instead of an error. Use a custom deserializer that rejects non-`"auto"` strings.

### 4. `cdylib` crate type is compiled but never consumed

The spec says `cdylib` is "for future use." It's built on every compile, adding ~3–5 seconds and producing a `libccvv_lib.dylib` nobody uses. Remove it and add back when needed.

### 5. Binary size budget (< 5 MB) is optimistic with the dependency set

`regex` + `serde_json` + `rusqlite` (bundled) + `clap` (derive) together can exceed 5 MB in release mode, especially with debug info. The `cargo bloat` CI check is good, but expect painful tradeoffs (e.g., `clap` derive → manual parsing, or aggressive `strip` / LTO).

### 6. Adaptive timing FFI functions are specified in Section 10.11 but missing from the FFI surface

`ccvv_timing_record_sample` and `ccvv_timing_get_threshold_ms` appear in the adaptive timing section but are not listed in Section 7.3's function signatures. They need to be added to the FFI surface, cbindgen config, and memory management table.

### 7. Mutex'd single rusqlite connection can become a latency spike

Probably fine for v1 with 50-entry history. But if transforms happen in quick succession (rapid copy-paste workflow), SQLite writes on the same thread as event handling will cause jitter. WAL mode is not mentioned in the spec — the default journal mode (`DELETE`) takes a write lock that blocks concurrent reads. Specify WAL mode.

### 8. `SCREAMING_CASE` regex matches common English words

Stage 7's `SCREAMING_CASE` pattern (`[A-Z][A-Z0-9_]{2,}`) matches `THE`, `AND`, `FOR`, `USA`, `API`, `SQL`, etc. These will be spuriously wrapped in backticks. The spec doesn't mention an exclusion list for common abbreviations.

### 9. Config format is TOML-only — be explicit about this

Many backend/devops users expect JSON or YAML. TOML is a fine choice, but the spec should explicitly state "TOML only" rather than leaving room for ambiguity. Don't add multi-format support — just be clear about the decision.

---

## Questions for the Spec Author

### 1. What is the exact "Stage 1 inline code markers" format?

The spec says Rust receives "plain text with backtick-wrapped code markers inserted" from Swift's Stage 1. Does Swift insert literal backticks into the text? If yes, the text is already polluted before Rust sees it — downstream stages (structural detection, table detection, URL detection) will misinterpret the backticks.

Alternative: pass style spans as a side-channel structure (e.g., `Vec<(usize, usize)>` byte ranges that are inline code), or use private-use-area Unicode sentinels that no real clipboard content would contain.

### 2. Do you actually need NSPasteboard polling if you already have CGEventTap?

If transforms only occur on explicit user action (double-tap Cmd+C detected via CGEventTap), clipboard polling is wasted wakeups. The only reason to poll is to detect clipboard writes that didn't come from a keyboard shortcut (menu Edit→Copy, AppleScript, etc.). Is that a v1 requirement?

### 3. Is the idempotency contract meant to include user-defined regex rules (Stage 8)?

If yes, you need a real policy for non-idempotent rules (detection, warning, or iteration-to-fixpoint). If no, the spec should explicitly exclude Stage 8 from the contract.

### 4. Should Stage 5's JSON detection have a cheap pre-check?

Currently, `serde_json::from_str` is called on the entire input. For 100 KB of non-JSON text, serde scans the whole string before returning `Err`. A one-line pre-check (`first non-whitespace char is '{' or '['`) would eliminate this cost for the ~95% of clipboard content that isn't JSON.

### 5. What does the Preferences toggle-write do to the live config?

Section 10.8 says toggle changes are written back to the TOML config file. But config is loaded at startup and compiled into `ResolvedConfig` with pre-compiled regexes. When the user flips a toggle, does the app (a) reload and recompile the entire config, (b) mutate the in-memory `ResolvedConfig`, or (c) require a restart? (a) re-compiles user regexes on every toggle. (b) lets TOML and in-memory state diverge. (c) is hostile UX.

### 6. What is the minimum macOS deployment target and Rust MSRV?

The spec uses `NSStatusItem.button?.layer` for the shake animation, which requires layer-backing. Behavior varies across macOS versions. If targeting macOS 12+ (Monterey), say so. If 10.15 or 11, the animation code may need adjustment. Similarly: MSRV for Rust isn't documented. `clap 4` derive requires Rust 1.74+, `rusqlite 0.32` requires 1.70+, `thiserror 2` requires 1.65+. What's the floor? Is it documented anywhere a contributor would find it?

### 7. What is the expected behavior for edge-case inputs?

The spec describes behavior but not failure modes for several stages:
- What does the URL cleaner do with malformed URLs that the `url` crate rejects? (Pass through? Strip the malformed URL? Log a warning?)
- What does the JSON prettifier do with JSON containing embedded null bytes?
- What does the table detector do with a 10,000-row spreadsheet paste? (Is there a row cap?)
- What does Stage 3 do with a file that is 100% blank lines?

### 8. Where do SQLite writes occur (threading)?

In the event tap callback? Main thread? Background queue? If writes happen synchronously on the event tap callback thread, a slow SQLite write blocks the next keyboard event. Should writes be dispatched to a background serial queue? Do you require WAL mode / specific synchronous settings?

### 9. What is the test corpus strategy?

Section 13.5 says "collect a corpus of clipboard inputs" to verify behavioral parity with the Swift implementation. Is there an existing corpus from the current Swift app's logging? If the intent is to re-implement from the spec description alone, behavioral parity on the first try is unlikely — the Swift code will have edge cases not captured in the spec.

### 10. Table detection scope — how far does "tabular" go?

Is detection limited to TSV/CSV from spreadsheets, or does it also cover: aligned columnar text (spaces as delimiters), pipe-delimited tables (already Markdown), fixed-width columnar output (e.g., `ps aux`, `ls -l`)? Each of these requires different detection heuristics, and the spec only describes delimiter-based splitting. If you truly want RFC 4180-ish behavior, are you prepared to add a CSV parser dependency or write one?
