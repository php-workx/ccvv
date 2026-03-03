# Technical Review: ccvv v1.0 Specification

**Reviewer perspective:** Senior macOS platform engineer, 10+ years AppKit, shipped multiple menu bar utilities with CGEventTap, NSPasteboard monitoring, Accessibility APIs, and notarized distribution.

**Documents reviewed:** `specs/functional.md`, `specs/technical_v1_0.md`

---

## Critical (Will Break in Production)

**1. Clipboard monitoring architecture is contradictory and neither approach works cleanly for double-tap.**

The functional spec (§3) explicitly states: *"ccvv hooks the OS-level clipboard, not individual key events. The double-tap detection listens for two clipboard-write events in rapid succession."* It then specifies `NSPasteboard` change-count polling.

The technical spec (§10.1, §8.2) implements `CGEventTap` to intercept `Cmd+C` keystrokes — the opposite approach. The call flow diagram shows `handleCGKeyEvent()` as the entry point.

These are fundamentally different architectures with different tradeoffs, failure modes, and permission requirements. The spec must pick one. Here's the real tradeoff:

- **CGEventTap (current implementation):** Detects the *keystroke*, not the clipboard write. This means it fires on `Cmd+C` even if the app handles it differently (e.g., Electron apps that intercept copy, apps where `Cmd+C` does something else). It requires Accessibility permission. It does NOT work for clipboard writes that aren't `Cmd+C` (tmux yank, right-click → Copy, programmatic copies). The functional spec's claim that it works with "any application that writes to the system clipboard regardless of keybinding" is **false** under this architecture.

- **NSPasteboard polling (functional spec's stated approach):** Detects actual clipboard writes regardless of source. No Accessibility permission needed. But: polling at 2 Hz gives 500ms granularity. The double-tap window is 300ms. A copy event detected at the next poll tick could be up to 500ms late. Two copies 250ms apart might both land in the same poll interval or straddle two intervals unpredictably. **Reliable double-tap detection at 300ms is impossible at 2 Hz polling.** You'd need ≥10 Hz (100ms granularity) to reliably distinguish within a 300ms window, which pushes idle CPU wakeups well above the 1/sec performance budget.

**Recommendation:** Use a **hybrid approach** for v1: CGEventTap for precise double-tap *gesture* detection (sub-millisecond keystroke timing), but treat `NSPasteboard.changeCount` as the authoritative signal that the clipboard *actually changed*. After each detected `Cmd+C`, start a short-lived wait loop (5–20ms poll intervals, up to ~250–400ms) to confirm the pasteboard actually updated before reading content. This avoids the "pasteboard updated after the keyDown" race (see Critical #2) without running a permanent high-frequency poll. The functional spec must be corrected to acknowledge this is keystroke-triggered, not clipboard-write-triggered. Document the limitations: v1 works for `Cmd+C` and nothing else (not menu copy, right-click copy, programmatic writes, tmux yank).

**2. CGEventTap fires before the pasteboard updates — reading immediately on second tap will intermittently get stale content.**

The event tap callback fires on `keyDown` for `Cmd+C`. But `NSPasteboard.general` is not updated synchronously with the keystroke — the foreground app's copy handler runs asynchronously, and the pasteboard typically updates *after* the key event has been delivered and processed by the target app. Under load (JetBrains IDEs, Electron apps, VM/remote desktop scenarios), this delay can be tens or even hundreds of milliseconds.

If ccvv reads `NSPasteboard.general` immediately in the event tap callback for the second `Cmd+C`, it will intermittently read the *previous* clipboard content (from the first copy), or catch the pasteboard mid-update. The "same content hash" comparison would then see identical content (because the second copy hasn't landed yet), triggering transformation on the first copy's content — or it would see a half-written state.

**Fix:** After detecting the second `Cmd+C` keystroke, do NOT read the pasteboard immediately. Instead, start a short-lived poll loop: check `changeCount` every 5–20ms for up to ~250–400ms, and only read the pasteboard content once `changeCount` increments (confirming the second copy actually landed). This is the proven pattern in shipped clipboard utilities. If `changeCount` doesn't increment within the timeout (the keystroke didn't result in a copy), abort without transforming.

**3. Pasteboard write-back creates a feedback loop with no documented prevention.**

When ccvv writes cleaned text back via `pb.setString(cleaned, forType: .string)` (§8.2, line 870), this increments `NSPasteboard.general.changeCount`. If any part of the system monitors `changeCount` for state (the adaptive timing system, any future polling fallback, or the content-hash comparison for double-tap detection), this write-back looks like a *new clipboard event*. 

With CGEventTap the feedback loop doesn't trigger (the write-back isn't a keystroke), but the spec should explicitly document a `isWritingBack` guard flag that suppresses processing during the write-back window. This becomes critical if you ever add `changeCount` polling as a secondary detection mechanism.

**4. `ccvv_last_error()` returns a dangling pointer.**

Section 11.2: `ccvv_last_error()` returns `s.as_ptr()` from inside a `RefCell` borrow. The pointer is valid only while the `RefCell` borrow is held — but the borrow is dropped at the end of the `with` closure. The returned `*const c_char` points into the `RefCell`'s `Option<CString>`, which remains valid only until the next `set_last_error()` call on the same thread. 

The comment says "valid until the next FFI call" which is *technically* correct, but the implementation is fragile. If Swift caches the pointer and reads it after another FFI call, it reads freed or overwritten memory. The Swift wrapper should immediately copy the string via `String(cString:)` before any other FFI call. This must be documented as a hard contract, not an assumption.

**5. `pb.setString(cleaned, forType: .string)` destroys all pasteboard types.**

Section 8.2 line 870: `pb.setString(cleaned, forType: .string)`. `NSPasteboard.setString` first calls `pb.clearContents()`, then writes only `.string`. This vaporizes every other pasteboard type: `.rtf`, `.html`, `.pdf`, `.tiff`, file promises, etc.

The functional spec calls this "Blind Paste" (§5) and frames it as intentional — but the consequences aren't fully explored. If a user copies a rich email in Mail.app, double-taps, then pastes into another Mail.app compose window, they get unstyled plain text. If they copy an image alongside text from a web page, the image is gone. If they copy a file in Finder (which uses `NSFilenamesPboardType`), double-tap by accident, the file reference is destroyed and replaced with the filename as text.

**Recommendation:** At minimum, skip transformation entirely when the pasteboard contains *only* non-text types (files, images). Check for `.string` or `.rtf` presence before attempting transformation. The current spec would corrupt Finder file copies.

**6. The spec conflates Accessibility permission with Input Monitoring — these are separate TCC categories on modern macOS.**

The spec only addresses `AXIsProcessTrustedWithOptions` and implies that Accessibility permission is sufficient for CGEventTap. On macOS Ventura and later, Apple exposes **Input Monitoring** as a distinct privacy control. Users can have Accessibility enabled but Input Monitoring disabled, or vice versa. CGEventTap for keyboard monitoring may require Input Monitoring permission depending on how the tap is created and what events it observes.

Furthermore, the review prompt mentions `com.apple.security.accessibility` as an entitlement — but **this is not how TCC permissions work for Developer ID apps**. TCC (Transparency, Consent, and Control) permissions are granted at runtime by the user through System Settings, not by entitlements in the code signature. Entitlements control Hardened Runtime restrictions (like `com.apple.security.cs.disable-library-validation`), not TCC consent. The spec needs to:

- Test on a clean macOS Ventura+ install whether CGEventTap creation requires Input Monitoring consent, Accessibility consent, or both.
- Handle the case where Input Monitoring is a separate prompt/toggle.
- Remove any assumption that an entitlement file can grant Accessibility or Input Monitoring access.

**7. CcvvCore is a `struct` but holds heap-allocated opaque pointers with a `deinit`.**

Section 8.3: `CcvvCore` is declared as `struct` but has a `deinit` block. **Swift structs do not have `deinit`.** Only classes do. This code will not compile.

If changed to a `class`, it works but introduces reference-counting semantics. If kept as a struct, you need to manually manage lifecycle — perhaps with an explicit `shutdown()` method called from `applicationWillTerminate`. The spec must choose.

---

## Significant (Will Cause Real User Pain)

**1. CGEventTap disability is not handled.**

macOS can disable a CGEventTap if the callback takes too long or under system pressure. When this happens, the tap receives a `CGEvent` of type `.tapDisabledByTimeout` or `.tapDisabledByUserInput`. The spec's `handleCGKeyEvent` (§10.1–10.4) shows no handling for these event types. The app would silently stop detecting double-taps with no user-visible indication and no recovery path.

**Fix:** Check for tap-disabled events in the callback. When received, call `CGEvent.tapEnable(tap:, enable: true)` to re-enable. Update the status icon to indicate a problem if re-enable fails. Log it.

**2. Accessibility and Input Monitoring permissions can be revoked at runtime with no recovery path.**

The spec checks `AXIsProcessTrustedWithOptions` on launch (§10.1). But the user can revoke Accessibility permission (or Input Monitoring permission — see Critical #6) in System Settings while the app is running. The CGEventTap doesn't immediately fail — it just stops receiving events. The app appears to be running normally but does nothing. Apple's platform trend is that permissions can silently stop working after OS updates or policy tightening, which is exactly what event-tap apps run into.

**Fix:** Periodically (every 30–60s) re-check `AXIsProcessTrusted()` (without the prompt option). If permission is lost, update the status icon to an error state and show a notification with a "Open System Settings" deep link. Also detect tap-disable events (see Significant #1) and the absence of events over unusual periods as secondary signals. Consider checking both Accessibility and Input Monitoring states if testing reveals they're separate gates on your target macOS versions.

**3. NSStatusItem text rendering is fragile across macOS versions.**

Section 10.3 uses text-based status items: `[cc]`, `[--]`, `✓`, `[cc]•N`. I've shipped text-based status items and they are a maintenance nightmare:

- macOS 14 (Sonoma) changed menu bar spacing and introduced variable-width status items that can be clipped by the system when space is constrained.
- macOS 15 (Sequoia) further changed the rendering pipeline. Text baselines shift between versions.
- The brackets `[ ]` add width that competes with other status items. When the menu bar is crowded (common on laptops with notches), the system hides status items from left to right. A wider item is hidden sooner.
- The `✓` character renders at different widths than `[cc]`, causing the status item to visually jump/resize on every success flash.
- `.secondaryLabelColor` and `.tertiaryLabelColor` look nearly identical in Dark Mode on some displays.

**Recommendation:** Use an `NSImage`-based status item with template images. Create 3-4 SF Symbols or small PNGs (active, paused, success, miss) at 18×18pt. This is how every shipping menu bar app does it. Text status items are fine for prototyping but will generate a stream of "icon looks wrong on my Mac" bug reports.

**4. NSStatusBarButton layer animation may not work.**

Section 10.4: `showMissIndicator()` accesses `button.layer` and adds a `CAKeyframeAnimation`. `NSStatusBarButton` does not guarantee a backing layer. You must set `button?.wantsLayer = true` first. Even then, the system redraws the status bar button on its own schedule, which can override or conflict with your animation. In practice I've seen the shake animation work on some macOS versions and silently fail on others.

**Safer approach:** Instead of animating the layer, swap the icon to a "miss" variant for 200ms, or use `NSStatusItem.button?.highlight(true)` with a timer.

**5. HUD toast positioning has multiple failure modes.**

Section 10.5: The toast is positioned "20pt below and right of `NSEvent.mouseLocation`." Problems:

- **Multiple displays with different scaling:** `NSEvent.mouseLocation` is in global screen coordinates. Converting to window coordinates for a specific screen requires accounting for `screen.frame` and `screen.backingScaleFactor`. A toast positioned correctly on a Retina display will be offset on a non-Retina external monitor.
- **Full-screen apps:** A `.floating` level window will not appear above a full-screen app. You need `.screenSaver` or `.statusBar` level, or use `window.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]`.
- **Stage Manager (macOS Ventura+):** Floating windows behave unpredictably in Stage Manager. They may appear in the wrong Stage or persist visually when switching Stages.
- **Spaces/Mission Control:** Without `.canJoinAllSpaces` in `collectionBehavior`, the toast may appear on the wrong Space.

**Fix:** Set `toast.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]`. Use `.statusBar` window level. Clamp the toast position to the active screen's visible frame.

**6. History panel positioning below the status item is unreliable.**

Section 10.9: "NSPanel anchored below the status item." Getting the screen position of a status item has been unreliable since macOS 11 and got worse in macOS 14. `NSStatusItem.button?.window?.frame` can return stale or zero-origin values, especially after the menu bar auto-hides or the display configuration changes.

**Robust approach:** Use `NSPopover` attached to the status item button instead of a manually positioned `NSPanel`. `NSPopover` handles positioning, screen edge avoidance, and display changes automatically. If you need more control than `NSPopover` provides, use the status item's `menu` property to present a custom `NSMenu` and position your panel relative to the menu's window frame when it appears.

**7. Config write-back from Preferences UI to TOML file is underspecified.**

Section 10.8: "Toggle changes are written back to the TOML config file." This is deceptively complex:

- Preserving comments and formatting in TOML when programmatically modifying values requires a TOML-preserving parser (like `toml_edit` in Rust, not `toml`). The `toml` crate deserializes into structs and loses all comments and formatting on round-trip.
- "If no config file exists, one is created at the default location with only the changed values." This means partial TOML files are valid, which the config resolution handles — but write-back must not dump all defaults into a new file.
- Race condition: if the user edits the config file in a text editor while Preferences is open, one will overwrite the other.

**Recommendation:** Use `toml_edit` for config write-back, or treat the TOML file as read-only from the GUI and store GUI toggle state separately (e.g., in `UserDefaults` on macOS), with the TOML file taking precedence when present.

**8. TOCTOU on pasteboard content hash — needs precise specification and changeCount correlation.**

Section 3 of the functional spec describes comparing content hashes between first and second copy. With the CGEventTap approach, the "first copy" content is read from the pasteboard after the keystroke event is processed. But `NSPasteboard` access isn't synchronized — another app (clipboard manager, universal clipboard from iPhone) could write to the pasteboard between the keystroke event and the pasteboard read. The window is small but real, especially with Universal Clipboard which has network latency.

The spec doesn't specify the hash algorithm or normalization. For clipboard-sized text (typically < 100 KB), a simple string equality comparison is fine and avoids hash collision concerns. For very large pasteboard contents (entire files pasted as text), you'd want a fast hash (e.g., xxHash). Specify:

- The hash algorithm (or that you're using direct string equality).
- What you hash: the `.string` type only? Normalized? Raw bytes?
- Use `pasteboard.changeCount` as a cheap first-pass comparison before doing any content read — if `changeCount` hasn't changed between first and second tap, the clipboard hasn't been written to and you can skip entirely.
- Consider storing a short-lived "pasteboard snapshot" (changeCount + content hash at first-copy time) rather than re-reading the pasteboard on the second tap for comparison. This avoids the expensive read and bounds the TOCTOU window.

---

## Minor (Polish, Not Blocking)

**1. Universal binary deferred but Homebrew cask may need it.**

Section 16 defers universal binary to post-v1. But if the cask is distributed as a pre-built binary, x86_64 users on Intel Macs get nothing. Either ship a universal binary for the cask, or provide two cask variants. In practice, the Apple Silicon transition is far enough along (March 2026) that Intel-only might be acceptable if documented.

**2. `NSSwitch` reference is fine but unnecessary to call out.**

Section 10.8 mentions "NSSwitch (macOS 10.15+)" with a minimum deployment target of 13.0. This is fine — no issue. But the spec wastes words noting it. Remove the version annotation; it adds confusion about whether there are other minimum-version concerns (there aren't, with a 13.0 floor).

**3. Confidence Mode counter reset on plist deletion.**

Section 10.6: `transformCount` in `UserDefaults`. If `~/Library/Preferences/com.ccvv.app.plist` is deleted, the counter resets and the user gets 50 more toasts. This is low-impact and arguably a feature (fresh start). Not worth engineering around.

**4. Adaptive timing data persistence.**

Section 10.11: Timing samples persisted to `~/Library/Application Support/ccvv/timing.json`. If corrupted or deleted, threshold resets to 300ms. The spec should document this as expected graceful degradation, not leave it implicit.

**5. `cdylib` crate type adds build time for no v1 use.**

Section 3.2: The lib crate produces both `staticlib` and `cdylib`. The `cdylib` is "for future use." It adds compilation time and produces an unused artifact. Remove it from v1 and add it when needed.

**6. Hardened Runtime entitlements file is missing from the build script.**

The build script (§8.4) does `codesign --force --options runtime --sign ...` but never specifies an `--entitlements` flag pointing to an entitlements plist. Hardened Runtime blocks certain capabilities by default. While CGEventTap permissions are governed by TCC at runtime (see Critical #6), not by entitlements, there may still be Hardened Runtime restrictions that need explicit entitlements — for example, `com.apple.security.automation.apple-events` if the app uses `NSWorkspace.shared.open()` to launch System Settings.

Static linking of Rust means `com.apple.security.cs.disable-library-validation` is NOT needed (the Rust code is part of the main binary). The `regex` crate uses DFA/NFA, not JIT, so `allow-unsigned-executable-memory` is also not needed. But the spec should include an explicit entitlements file — even if it's empty — so the build is self-documenting about what's needed and what's not. Test signing and notarization on a clean machine.

**7. Homebrew cask quarantine assumptions may not hold — policy is shifting.**

Homebrew currently strips the quarantine xattr for cask installs, but the project has [discussed deprecating `--no-quarantine` behavior](https://github.com/Homebrew/brew/issues/20755) for casks. If Homebrew moves toward not bypassing Gatekeeper, your first-run experience changes: users will see Gatekeeper's "unidentified developer" dialog (or, if notarized, the "downloaded from the internet" confirmation). This is fine if you're properly notarized — but the spec should define the Gatekeeper-first-run UX and not assume quarantine is silently stripped. Verify in `brew-local-test.sh` with quarantine *intact* to ensure the notarization + stapling path works end-to-end.

**8. Notarization with Rust static libraries.**

Apple's notarization service runs binary analysis that can flag unusual code patterns. Rust's standard library statically linked into a Swift binary is not a problem in practice — I've shipped notarized binaries with Rust staticlibs without issues. The main risk is if Rust panics produce stack unwinding code that looks like exception-handling shenanigans, but this is cosmetic (warnings in the notarization log, not rejections).

**9. No forward-compatibility strategy for annual menu bar behavior changes.**

Apple changes menu bar rendering, spacing, and window behavior nearly every macOS release. The spec has no plan for this. Keeping UI paths shallow reduces the blast radius: image-based status items (not text), `NSPopover` for the history panel (not manual positioning), and standard window levels for the HUD toast. Consider adding a "disable fancy positioning" fallback (centered panel, or standard window anchoring) for when status-item-relative positioning breaks on a future macOS version.

---

## Questions for the Spec Author

1. **Which architecture are you actually building?** The functional spec says clipboard-write monitoring; the technical spec implements keystroke interception. These have completely different capability profiles. The functional spec's compatibility claims (works with tmux yank, right-click copy, any clipboard write) are false under CGEventTap. Which claims does v1 actually support? What's the behavior for non-keyboard copies (menu, context menu, programmatic)?

2. **What is your pasteboard synchronization strategy?** If CGEventTap is the primary trigger, the pasteboard updates asynchronously after the keyDown. Do you delay-then-read? Wait for changeCount to increment? Use a short-lived poll loop? This is not an edge case — it's the main path. (See Critical #2.)

3. **Does your app require Input Monitoring permission in addition to Accessibility on Ventura+?** If yes, how do you detect and message that state? Have you tested on a clean macOS 13/14/15 install where neither permission is pre-granted? (See Critical #6.)

4. **Is destroying all pasteboard types on write-back intentional for all cases?** Specifically: what happens when a user copies a file in Finder and double-taps? What about an image from Preview? Should ccvv check pasteboard types and bail out if no `.string` type is present?

5. **`CcvvCore` is a struct with `deinit` — has this been compiled?** Swift structs don't support `deinit`. This is either pseudocode or a bug. Which is it?

6. **What is the tap recovery mechanism?** How do you handle `kCGEventTapDisabledByTimeout` / `kCGEventTapDisabledByUserInput`? What user-visible error state is shown? (See Significant #1.)

7. **How do you prevent self-trigger loops when you write back to NSPasteboard?** The spec doesn't document a sentinel mechanism. Options include: a stored `changeCount` to ignore, an `isWritingBack` flag, or a custom pasteboard type used as a marker. Which one? (See Critical #3.)

8. **What is the app exclusion check actually checking?** Section 8.2 calls `ccvv_is_app_excluded(bundleId)`. But in a CGEventTap callback, you only have the CGEvent — not the frontmost app's bundle ID. Getting the frontmost app requires `NSWorkspace.shared.frontmostApplication?.bundleIdentifier`, which must be called on the main thread. The event tap callback runs on a background runloop source. Is this dispatched to main? Is there latency from the dispatch?

9. **Has the `ccvv_timing_record_sample` / `ccvv_timing_get_threshold_ms` pair been thought through for thread safety?** These functions use state (the circular buffer), but §7.5 only discusses thread safety for Config, History, and Transform. The timing module appears to use unprotected global state.

10. **Why NSPanel for the history panel instead of NSPopover?** `NSPopover` attached to the status item button handles positioning, screen edge avoidance, and display changes automatically. A manually positioned `NSPanel` requires fragile status-item-position lookup that breaks across macOS releases. What's the fallback when anchoring fails?

11. **The functional spec promises "Force Touch trackpad haptic tap" for invisible mode (§2).** The technical spec defers "Invisible mode" to v2 (§16). But if a user enables "Hide Icon" in Preferences (§10.8), what feedback do they get? The spec shows a "Hide menu bar icon" toggle but no implementation for feedback when the icon is hidden. Is this toggle supposed to be present in v1?

12. **How does the first-run onboarding overlay (§10.10) interact with the Accessibility permission prompt (§10.1)?** On first launch, the user sees both: an Accessibility permission dialog (system) and the onboarding overlay (app). Which appears first? Can the user interact with the onboarding before granting Accessibility permission? If so, "Try it now" will fail silently because the event tap isn't registered yet.

13. **Where is the entitlements.plist?** The build script signs with Hardened Runtime but no entitlements file is referenced. Even an empty entitlements file makes the build self-documenting. Has this been tested on a clean Mac (not the developer's machine, which already has permissions cached)?

14. **Homebrew distribution: do you assume quarantine removal?** If Homebrew changes its quarantine bypass policy, what's your Gatekeeper-first-run UX? Is notarization + stapling verified end-to-end with quarantine intact?

15. **Is "sanitize means plain-text only" the explicit product decision?** If so, say so in onboarding and product copy — otherwise users who copy rich text and get plain text back will perceive it as a bug and uninstall. If you want to preserve rich types, what does "cleaned rich text" mean when you've modified the text content?
