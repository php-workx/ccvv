# Security & Reliability Review: ccvv v1.0

**Reviewer role:** Security and reliability engineer (adversarial review)
**Document under review:** `specs/technical_v1.0.md` (Draft)
**Reference:** `specs/functional.md`
**Date:** 2026-03-01

---

## Preamble

The spec describes a well-architected system — the Rust/Swift split is clean, the FFI boundary is explicit, and the idempotency contract is the kind of thing most clipboard tools never think about. That said, this application sits in an extremely privileged position: it has Accessibility permission, reads every copy event system-wide, stores clipboard history in cleartext, and forcefully overwrites the pasteboard. The spec has a one-page "Security Considerations" section (§15) that reads like a reassurance rather than a threat model. The findings below are organized by severity.

---

## Critical

These are security issues or high-probability data loss scenarios that should block a v1 release.

### C1. The crash window in `performClean()` can destroy clipboard data with no recovery

The call flow in §8.2 is:

```
ccvv_transform(text, config)    → get cleaned text
pb.setString(cleaned_text)      → overwrite clipboard
ccvv_history_push(raw, cleaned) → write to history
```

If the process is killed (SIGKILL, OOM, macOS force-quit, kernel panic) after `pb.setString` but before `ccvv_history_push` completes, the original raw text is permanently lost. The clipboard now contains the transformed version, and the history database does not have the original. The user has no undo path.

Worse: `NSPasteboard.setString` is internally a `clearContents()` followed by a write. If the process dies between clear and write, the clipboard is empty *and* the history has no record. This is a two-operation non-atomic sequence on a shared system resource.

**Recommendation:** Reverse the ordering. Write to history *first*, then overwrite the clipboard. A crash after history-write but before clipboard-write is recoverable (the user's original is in the undo stack and the clipboard still holds the pre-transform content). A crash after both writes is the normal success path. This eliminates the unrecoverable window entirely.

### C2. The history database stores sensitive data in cleartext with no access controls

The spec stores the last 50 clipboard items — `raw_text` and `cleaned_text` — in `~/Library/Application Support/ccvv/history.db`. This will inevitably contain passwords copied from password managers (unless the app-exclusion list is perfectly configured), API keys, session tokens, personal messages, credit card numbers, and other sensitive data.

The spec specifies no protections:

- **No encryption.** The SQLite database is plaintext on disk. Any process running as the same user can open it with `sqlite3`. This includes npm postinstall scripts, Electron apps with `node:fs`, browser extensions with filesystem access, other background utilities, and any malicious payload that achieves user-level code execution.
- **No file permissions.** The spec doesn't set restrictive permissions (e.g., `0600`) on the database file. The default `umask` on macOS is typically `022`, meaning the file will be world-readable (`0644`).
- **No Time Machine exclusion.** The `~/Library/Application Support/ccvv/` directory will be backed up by Time Machine by default. Every snapshot contains a copy of the cleartext history. Even if the user deletes their history, Time Machine retains it.
- **No FileVault dependency.** If the user's disk is not encrypted, the history is in cleartext on the physical disk, recoverable with disk forensics.

**Recommendation (minimum for v1):** Set `0600` permissions on `history.db` at creation time. Add `history.db` to the Time Machine exclusion list via `NSURLIsExcludedFromBackupKey`. Document in the README that ccvv stores clipboard history locally and recommend FileVault. For v1.1, investigate SQLCipher for at-rest encryption.

### C3. Accessibility permission grants far more access than the spec acknowledges

§15 says: "The app requests the minimum permission needed and documents why." This is not accurate. On macOS, granting Accessibility permission to an application gives it the ability to:

- Read *all* keyboard input system-wide (not just Cmd+C).
- Synthesize keyboard and mouse events.
- Read the contents of any text field in any application via the AX API.
- Observe and manipulate the UI hierarchy of all running applications.

ccvv uses this permission only for `CGEventTap` in listen-only mode to detect Cmd+C double-taps. But the permission itself is binary — there is no way to grant "only CGEventTap" without granting everything above. If ccvv is compromised (e.g., a supply chain attack on a dependency, or a malicious config-file regex injection — see C4), the attacker inherits all of these capabilities.

The spec should explicitly acknowledge this in a threat model section: "ccvv's Accessibility permission is the most dangerous permission in the application. If the application binary is compromised, the attacker gains full input monitoring and UI automation capabilities." This is not a fixable issue — it's a fundamental constraint of macOS — but users deserve to know the trust they're extending.

**Recommendation:** Add a threat model section to the spec that enumerates what Accessibility permission grants and why. Consider runtime self-integrity checks (verify the binary's code signature on launch). Document the attack surface honestly.

### C4. Config file regex injection can silently modify clipboard content

The config file at `~/.config/ccvv/config.toml` is writable by any process running as the user. §5.2 allows user-defined regex rules with arbitrary replacement strings:

```toml
[[rules]]
name = "my rule"
pattern = "..."
replace = "..."
```

An attacker who can write to the config file can inject a rule that matches cryptocurrency wallet addresses and replaces them with the attacker's address, or matches API keys and exfiltrates them via the replacement string (though the replacement is local-only, so the exfiltration vector is indirect — but the *modification* vector is immediate).

The config file has no integrity protection: no checksum, no signature, no `immutable` flag. The spec doesn't even specify file permissions on the config file.

**Recommendation:** Set `0600` permissions on the config file at creation time. When the Preferences window writes the config (§10.8), verify the file hasn't been modified by an external process since last read (compare mtime or hash). Log a warning to the user if the config file was modified externally. Consider a `ccvv doctor` check that warns if the config file permissions are too permissive.

---

## Significant

These are real risks that should be mitigated before v1 ships, but are not as immediately exploitable or destructive as the critical findings.

### S1. The app exclusion list defaults are compiled into the binary, not the config — and user configs silently override them

§15 says: "The app exclusion list defaults to password managers (KeePassXC, 1Password)." §5.2 shows `Exclusions` has `apps: Vec<String>` with `#[serde(default)]`. If a user creates a config file with an `[exclusions]` section (even an empty one), the default empty `Vec` replaces the compiled-in defaults.

A user who adds `exclusions.apps = ["com.my.app"]` almost certainly intends to *extend* the exclusion list, not replace it. But the serde `default` behavior means the password manager exclusions vanish. The user now has ccvv reading and transforming their 1Password clipboard copies.

**Recommendation:** The config resolution logic (§5.3) should merge user exclusions with hardcoded defaults, not replace them. Add a separate `exclusions.remove_defaults` flag that must be explicitly set to `true` to suppress the built-in list. `ccvv doctor` should warn if the effective exclusion list does not contain the default password manager bundle IDs.

### S2. No maximum input size — large clipboard content is a DoS vector

The transform pipeline (§4) processes whatever text `NSPasteboard` provides. Clipboard content can be arbitrarily large: a user might copy an entire 10MB log file, a base64-encoded image, or a multi-MB JSON response.

The performance budget (§12) specifies targets for 10KB and 100KB inputs, but says nothing about what happens at 1MB, 10MB, or 100MB. Several stages are super-linear in the worst case:

- **Stage 5 (JSON):** `serde_json::from_str` on a 50MB string will allocate a `Value` tree that's 3–5× the input size. `to_string_pretty` then allocates again.
- **Stage 5 (table detection):** A 50,000-row CSV will produce a 50,000-row Markdown table — both slow to generate and unusable as output.
- **Stage 6 (URL scanning):** The URL regex scanned against a 10MB string with many partial matches could be slow even with the `regex` crate's linear-time guarantee (linear-time doesn't mean fast in absolute terms).
- **Stage 8 (user regex):** Multiple regex replacements over a large input multiply the cost.

While the process is transforming, the user's Cmd+C is blocked (the double-tap detection is waiting). A 5-second transform on a large paste makes the tool feel broken.

**Recommendation:** Add a configurable maximum input size (default: 512KB). Content exceeding this limit passes through untransformed with a diagnostic log entry. This is a safety valve, not a feature limitation — clipboard content above 512KB is almost never prose or URLs that benefit from sanitization.

### S3. The backtick auto-wrapper (Stage 7) can produce dangerous shell content

§4.3 Stage 7 wraps tokens matching heuristics in backticks. In a shell context, backticks are command substitution operators. If a user copies text that Stage 7 wraps (e.g., `file.txt` becomes `` `file.txt` ``), and then pastes it into a terminal, the shell will attempt to execute `file.txt` as a command.

The spec defers Smart Paste (context-aware destination formatting) to v2, which means v1 has no way to know *where* the user will paste. The auto-wrapper always runs, and its output is always backtick-wrapped.

This is particularly dangerous because the auto-wrapper's heuristics (§4.3 Stage 7) target exactly the kind of tokens that appear in shell commands: file paths, CLI flags, snake_case identifiers. The tool's core audience — developers — pastes these into terminals constantly.

**Recommendation:** This is a design tension with no clean fix in v1 (without Smart Paste). Options: (a) disable auto-wrapper by default and make it opt-in, (b) add a prominent warning in onboarding that auto-wrapped text should not be pasted directly into a terminal, or (c) use a different wrapping strategy that's shell-safe (e.g., Unicode quotation marks for visual distinction without shell semantics — though this creates other problems). At minimum, document this footgun explicitly.

### S4. Hash collision in double-tap detection could trigger unintended transforms

The spec describes double-tap detection by comparing the content of two sequential Cmd+C events. If the user copies the same text twice (which is common — copying a URL from the address bar, getting distracted, copying it again), the second copy looks like a double-tap and triggers transformation.

The spec doesn't clarify the detection mechanism precisely, but the functional spec mentions "same content hash." If two different texts produce a hash collision (however unlikely), a genuine second copy of different text would be incorrectly treated as a double-tap.

**Recommendation:** Specify the hash function (SHA-256 is fine; CRC32 is not). Document the expected false-positive rate. More importantly, clarify the "same content = double tap" logic: if the user genuinely copies the same text twice, is the second copy always transformed? If so, this should be documented as expected behavior, because users who re-copy the same URL will get an unwanted transformation.

### S5. SQLite cross-process access between the app and CLI has no coordination

§7.5 says `CcvvHistory` uses an internal `Mutex<rusqlite::Connection>`. This serializes access within a single process. But the CLI (`ccvv history`) is a separate process that opens the same database file.

SQLite handles this via file-level locking, but the spec doesn't specify:

- **Journal mode.** WAL mode allows concurrent readers but requires careful cleanup of `-wal` and `-shm` files. DELETE mode is simpler but blocks readers during writes. The spec doesn't specify which mode is used. If WAL mode is used and the app crashes, stale `-wal` files can cause the next open to replay a potentially large write-ahead log.
- **Busy timeout.** If the CLI reads while the app writes (or vice versa), one will get `SQLITE_BUSY`. The spec doesn't specify a retry strategy. The default behavior is to fail immediately.
- **VACUUM strategy.** The pruning query (§6.2) deletes old rows but doesn't reclaim disk space. If the user regularly copies large content, the database file grows monotonically. Over months, it could reach tens or hundreds of MB of free pages.

**Recommendation:** Specify WAL mode with a 5-second busy timeout. Add periodic `VACUUM` (e.g., on app launch, or after every 100 prune operations). Add a `ccvv doctor` check that reports database file size and offers to vacuum.

### S6. `ccvv_last_error()` returns a dangling pointer if called after the next FFI call

§11.2 stores the error in thread-local `RefCell<Option<CString>>`. The `ccvv_last_error()` function returns a raw pointer into the `CString`'s buffer. If the caller (Swift) holds this pointer and then calls another FFI function that overwrites `LAST_ERROR`, the pointer is dangling.

The spec says "Returned string is valid until the next FFI call" (§7.3), but Swift's `String(cString:)` copies the bytes immediately. So in the *current* Swift wrapper, this is safe. However, the contract is fragile — any future FFI consumer that caches the pointer (e.g., a Python binding, a C program) will hit use-after-free.

**Recommendation:** Document this constraint loudly in the C header with a comment on the function. Consider changing the API to require the caller to provide a buffer, or return a `char*` that the caller must free (consistent with other FFI functions). The asymmetry between "caller frees" (most functions) and "caller must not free" (`ccvv_last_error`) is a footgun.

### S7. The "zero network access" claim (§15) is incomplete

The spec says: "ccvv makes zero network calls. No telemetry, no update checks, no analytics." This is likely true for ccvv's own code, but the claim should be more precise:

- **Apple's Gatekeeper and notarization.** On first launch of a notarized app, macOS may contact Apple's OCSP servers to validate the code signature. This is an Apple behavior, not ccvv's, but it means the app's first launch *does* generate a network call that reveals "this user launched ccvv." The spec should acknowledge this.
- **Homebrew.** `brew install ccvv` transmits installation analytics to Homebrew's servers (unless the user has opted out via `HOMEBREW_NO_ANALYTICS`). The spec should note this in the installation section.
- **Rust dependencies.** The `regex` crate, `rusqlite`, `serde`, etc. are unlikely to make network calls at runtime, but the spec should state that dependency audit has verified this.
- **DNS.** Even without explicit HTTP calls, some system APIs can trigger DNS resolution. The spec should clarify that ccvv does not call any API that could trigger network activity.

**Recommendation:** Rewrite §15 bullet 1 to say: "ccvv's own code makes zero network calls. The application does not include telemetry, update checks, or analytics. Note: macOS may contact Apple's OCSP servers to verify the application's notarization on first launch; this is OS-level behavior outside ccvv's control. Homebrew installation may transmit analytics unless the user has set HOMEBREW_NO_ANALYTICS=1."

---

## Minor

Defense-in-depth measures and good practices that improve robustness.

### M1. URL cleaning can silently break functional URLs

Stage 6 (§4.3) strips parameters matching a global deny list (`utm_source`, `gclid`, `fbclid`, etc.) and per-domain overrides. Some applications use `source=` or parameters with names that collide with tracking parameter patterns. The glob `utm_*` is safe (unlikely to be functional), but the per-domain override mechanism doesn't have a "known safe" list — it only has deny lists.

More subtly: stripping `www.` from the host can break URLs for servers that don't have a DNS record for the bare domain. `www.example.com` and `example.com` are not guaranteed to resolve to the same host.

**Recommendation:** Make `www.` stripping opt-in or add a "verify DNS" note in documentation. Add a `url_params.known_functional` allowlist that overrides deny lists for parameters that are known to be functional in specific APIs.

### M2. Mojibake repair confidence threshold is undefined

Stage 2 (§4.3) says: "Applies repair only when confidence is high (≥3 matching patterns in the same text block)." But "text block" is undefined. Is it a line? A paragraph? The entire input? If the input is a single line with 3 mojibake sequences, the repair fires. If it's 1000 lines with 3 mojibake sequences scattered throughout, does it still fire? A false positive here silently corrupts the user's text, and the user won't notice until the corrupted text is published.

**Recommendation:** Define "text block" precisely (suggest: per-paragraph). Add a `rules_fired` entry when mojibake repair triggers so the HUD toast shows "repaired encoding" and the user can catch false positives.

### M3. The `fancy-regex` crate prohibition should be explicit

§15 correctly notes that the `regex` crate guarantees linear-time matching, eliminating ReDoS. But this guarantee is lost if a future contributor adds `fancy-regex` (which supports backreferences, lookahead, and is vulnerable to ReDoS) as a dependency for a user-requested feature. The Cargo.toml doesn't have a deny list for crates.

**Recommendation:** Add a comment in `Cargo.toml` and in `userrules.rs`: "SECURITY: Do not replace `regex` with `fancy-regex`. The `regex` crate's linear-time guarantee is a security property that prevents user-defined rules from causing denial-of-service." Consider adding a `cargo deny` rule to block `fancy-regex` in CI.

### M4. The config file can be partially written if the Preferences window crashes

§10.8 says toggle changes are written back to the TOML config file. If the app crashes mid-write (or the disk fills up), the config file may be truncated or contain invalid TOML. On next launch, `toml::from_str` will return a parse error, and §5.1 step 4 says "if none found, all defaults apply" — but a *found but unparseable* config is different from "not found." The spec should clarify: does a parse error fall through to defaults (silent data loss of user preferences) or surface as an error?

**Recommendation:** Write the config to a temporary file, then atomically rename it (`rename(2)` is atomic on POSIX). This eliminates the partial-write window. If the config file exists but doesn't parse, show a `ccvv doctor`-style error in the menu bar icon (e.g., a warning badge) rather than silently falling back to defaults.

### M5. The `ccvv_transform` FFI function accepts unbounded `*const c_char` input

§7.3 shows `ccvv_transform(const char *input, ...)`. The function will call `CStr::from_ptr(input)` which scans for a null terminator with no length limit. If the input is not null-terminated (Swift's `withCString` guarantees this, but other FFI consumers might not), this is a buffer over-read. Even with a null terminator, the function will process arbitrarily large input (see S2).

**Recommendation:** Add a `ccvv_transform_n(const char *input, size_t len, ...)` variant that accepts an explicit length. The length-delimited variant should be the recommended API; the null-terminated version is a convenience wrapper.

### M6. No database corruption recovery strategy

§6 doesn't address what happens if `history.db` becomes corrupted (disk error, power loss during WAL checkpoint, full disk during write). `HistoryDb::open()` will presumably fail with a `rusqlite::Error`. The spec says errors in stages cause pass-through (§11.4), but a database error during `ccvv_history_push` could be silently swallowed, leaving the user without an undo stack they expect to exist.

**Recommendation:** If `HistoryDb::open()` fails, attempt to rename the corrupted file to `history.db.corrupt` and create a fresh database. Log the event. Add a `ccvv doctor` check that tests database integrity (`PRAGMA integrity_check`).

### M7. The `CcvvTransformResult` struct leaks if not freed

§7.2 returns a struct with two heap-allocated strings. If the Swift caller checks `cleaned_text` for NULL and returns early (§8.3 line `guard let cleanedPtr = result.cleaned_text else { return nil }`), `summary` is never freed. The `defer { ccvv_transform_result_free(result) }` line is *before* the guard in the spec's code, so this specific case is handled — but the pattern is fragile. If anyone reorders the `defer` and `guard`, or if a new FFI consumer forgets the free, memory leaks.

**Recommendation:** Consider making `ccvv_transform_result_free` accept a pointer (so it can be set to NULL after freeing), or document the ownership transfer more explicitly in the header with a `MUST_FREE` annotation.

---

## Threat Model Gaps

The spec does not have a threat model. These are attack surfaces and failure scenarios that are entirely unaddressed.

### T1. No threat model exists

This is the single most important finding. An application that has Accessibility permission, reads all clipboard events, stores clipboard history, and forcefully modifies the pasteboard should have a formal threat model that enumerates:

- **Trust boundaries:** What does ccvv trust? (The OS, the user, the config file, the clipboard content, the SQLite database, Rust dependencies.)
- **Assets:** What is ccvv protecting? (The user's clipboard content, the integrity of transformed output, the confidentiality of history.)
- **Threat actors:** Who might attack ccvv? (Malware with user-level access, a compromised dependency, a malicious config file, a crafted clipboard payload.)
- **Attack vectors:** How? (Config injection, dependency supply chain, clipboard poisoning, history exfiltration, privilege escalation via Accessibility permission.)

### T2. Clipboard-as-attack-vector is not considered

ccvv reads untrusted input (clipboard content) and processes it through a complex pipeline that includes regex matching, JSON parsing, URL parsing, and heuristic content detection. A crafted clipboard payload could:

- Trigger pathological behavior in `serde_json::from_str` (deeply nested JSON, extremely long strings).
- Exploit a bug in the `regex` crate (unlikely given its maturity, but not impossible).
- Craft content that passes through all stages but produces subtly different output than the user expects (e.g., a URL that looks normal but has a homoglyph character that survives the pipeline).

The spec treats clipboard content as benign input. It should be treated as untrusted input from a potentially adversarial source.

### T3. Supply chain risk from Rust dependencies is unaddressed

The spec lists 8 direct dependencies (§3.4). Each of these has its own dependency tree. A compromised version of any transitive dependency could:

- Exfiltrate clipboard content (the dependency runs in the same process, with the same Accessibility permission).
- Modify transformed output.
- Corrupt the history database.

The spec doesn't mention `cargo audit`, `cargo vet`, dependency pinning, or reproducible builds.

**Recommendation:** Add `cargo audit` and `cargo deny` to CI. Pin dependency versions in `Cargo.lock` (already standard practice, but worth stating explicitly). Consider `cargo vet` for high-trust dependencies.

### T4. The app's Accessibility permission makes it a high-value target for privilege escalation

If an attacker achieves code execution within ccvv (via a dependency vulnerability, a crafted config file, or a malicious clipboard payload that triggers a memory safety bug in a C dependency), they inherit Accessibility permission. This allows them to:

- Keylog the user's entire session.
- Inject synthetic keystrokes (e.g., open Terminal, run commands).
- Read the contents of any text field (including password fields that don't use secure input).
- Automate the macOS UI to grant further permissions.

This is not hypothetical — clipboard managers have been used as privilege escalation vectors in documented attacks.

### T5. No consideration of clipboard content visible to other apps after transformation

When ccvv writes cleaned text to `NSPasteboard`, that content is visible to every app that monitors the clipboard. This includes other clipboard managers, analytics SDKs embedded in running apps, and accessibility tools. If the user copies a password and ccvv transforms it (because the password manager wasn't in the exclusion list), the transformed password is now broadcast to every clipboard observer.

The spec's security model assumes ccvv is the only clipboard-aware app on the system. In practice, many users run multiple clipboard utilities, and some apps poll the clipboard silently.

### T6. No secure memory handling for sensitive clipboard content

The history database stores `raw_text` in a `String` (Rust heap allocation). When the entry is pruned, the `String` is dropped, but the memory is returned to the allocator — not zeroed. The sensitive content remains in the process's address space until the page is reused. A memory dump (via another app with Accessibility permission, or a crash reporter) could recover recently-pruned sensitive clipboard content.

This is a defense-in-depth concern, not a primary attack vector, but it's standard practice for applications that handle sensitive data (password managers use `mlock` and `zeroize`).

**Recommendation:** For v1.1, consider using the `zeroize` crate on `HistoryEntry` fields when entries are pruned. For v1, document this as a known limitation.

---

## Summary: The Five Worst Failure Modes (Ranked by Severity × Likelihood)

| Rank | Scenario | Severity | Likelihood | Mitigation |
|------|----------|----------|------------|------------|
| 1 | **Silent text corruption via auto-wrapper backticks pasted into a shell** — user pastes `` `config.yaml` `` into a terminal, shell executes it as a command | High (arbitrary command execution) | High (developers paste into terminals constantly) | S3: disable auto-wrapper by default or scope it |
| 2 | **Crash during `performClean()` loses clipboard data permanently** — no undo, no history, clipboard is empty | High (data loss) | Medium (app crashes do happen; macOS can kill background apps under memory pressure) | C1: reverse operation ordering |
| 3 | **History database exfiltrated by malware** — npm postinstall script reads `history.db`, extracts API keys and passwords | High (credential theft) | Medium (supply chain attacks are common) | C2: file permissions, encryption |
| 4 | **Config file injection replaces cryptocurrency addresses** — malware writes a regex rule to `config.toml` that swaps wallet addresses | High (financial loss) | Low-Medium (requires user-level code execution, but many apps have this) | C4: file permissions, integrity checks |
| 5 | **URL stripping breaks a functional URL in published documentation** — stripped parameter was not a tracker but a required API parameter; goes unnoticed until readers report broken links | Medium (reputation damage) | Medium (the deny list is broad; false positives are inevitable) | M1: conservative defaults, allowlist |
