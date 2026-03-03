# CCVV Gap Analysis: Functional Specification vs. Swift Implementation

**Date:** 2026-03-02  
**Analyzed Files:**
- `/Users/runger/workspaces/ccvv/mac/main.swift` (1582 lines)
- `/Users/runger/workspaces/ccvv/specs/technical_v1.md` (2284 lines, §11 macOS UI, §5 core timing, §9 architecture)
- `/Users/runger/workspaces/ccvv/specs/functional_v1.md` (product spec)
- `/Users/runger/workspaces/ccvv/mac/Info.plist`
- `/Users/runger/workspaces/ccvv/mac/build.sh`

---

## UI Features Implementation Status

### 1. Icon States (§11.3)

**SPECIFICATION REQUIRES:**
- NSImage-based status items with template images (18×18pt, PDF or SF Symbols)
- Automatic Light/Dark mode adaptation via `isTemplate = true`
- Five states: active, paused, success flash, miss indicator, no accessibility warning
- Reject text-based status items (`[cc]`, `[--]`) as fragile across macOS versions

**IMPLEMENTATION:** **PARTIAL - INCOMPLETE**

**CURRENT STATE (main.swift:772-780, 850-868):**
```swift
statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
if let button = statusItem.button {
    button.title = "[cc]"  // TEXT-BASED, NOT TEMPLATE IMAGES
    button.font = NSFont.monospacedSystemFont(ofSize: 12, weight: .medium)
}
```

**Icon updates (lines 850-861):**
- Active: `[cc]` (text)
- Paused: `[--]` (text)
- No accessibility: `[!!]` (text)
- Warning color: `contentTintColor = .systemOrange`

**MISSING COMPONENTS:**
1. **NSImage template images** - Not present. App uses text-based rendering.
2. **Asset bundle** - No `Assets.xcassets` or PDF image resources for `ccvv-active`, `ccvv-paused`, `ccvv-success`, `ccvv-warning` templates.
3. **isTemplate property** - Not applicable since using text, not images.
4. **Success flash image** - Line 868 shows " ok " text instead of checkmark template image.

**RISK:** Text-based status items are acknowledged in spec as "fragile across macOS versions: spacing changed in macOS 14, baselines shift between releases, different character widths cause visual jumps, and wider items are hidden sooner when the menu bar is crowded."

---

### 2. Miss Indicator (§11.4)

**SPECIFICATION REQUIRES:**
- Highlight-based approach: `button.highlight(true)` for 200ms, then `highlight(false)`
- Triggered when `elapsed > doubleTapWindow && elapsed < doubleTapWindow * 2.0`
- No layer animations; use highlight because NSStatusBarButton doesn't guarantee backing layer

**IMPLEMENTATION:** **COMPLETE**

**EVIDENCE (main.swift:877-881, 1023-1026):**
```swift
func showMissIndicator() {
    guard let button = statusItem.button else { return }
    button.highlight(true)
    DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) {
        button.highlight(false)
    }
}

// In handleCGKeyEvent, line 1023-1026:
if elapsed > doubleCopyWindowSeconds && elapsed < doubleCopyWindowSeconds * 2.0 {
    DispatchQueue.main.async { [weak self] in
        self?.showMissIndicator()
    }
}
```

✓ Uses highlight (not CAKeyframeAnimation)  
✓ 200ms duration  
✓ Conditional on 1x-2x window

---

### 3. HUD Toast (§11.5)

**SPECIFICATION REQUIRES:**
- Borderless NSWindow with `.statusBar` level
- `collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]`
- Position: 20pt below and right of mouse, clamped to active screen's visibleFrame
- Single NSTextField, 12pt system font, white text
- Sensitive/oversize feedback messages
- Auto-dismiss via NSAnimationContext fade-out

**IMPLEMENTATION:** **PARTIAL - MISSING POSITIONING & SCREEN LOGIC**

**EVIDENCE (main.swift:1103-1150):**
```swift
let toast = NSWindow(
    contentRect: NSRect(x: 0, y: 0, width: 320, height: 40),
    styleMask: [.borderless],
    backing: .buffered,
    defer: false
)
toast.isOpaque = false
toast.backgroundColor = NSColor.black.withAlphaComponent(0.8)
toast.level = .statusBar
toast.ignoresMouseEvents = true
toast.hasShadow = true
toast.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]

let label = NSTextField(labelWithString: message)
label.textColor = .white
label.font = NSFont.systemFont(ofSize: 12)
```

**MISSING:**
1. **Screen-aware positioning** - No code to:
   - Find active screen using `NSScreen.screens.first(where: NSMouseInRect(mouseLocation, ...)`
   - Get visibleFrame accounting for menu bar/dock
   - Clamp `origin.x` and `origin.y` to screen bounds
2. **Mouse location offset** - No `NSEvent.mouseLocation` + 20pt/68pt positioning
3. **Sensitive/oversize feedback** - Toast shows generic message but doesn't check `skippedSensitive` or `skippedOversize` flags
4. **Fade-out animation** - No NSAnimationContext fade

**CURRENT BEHAVIOR:** Toast appears at 320×40 fixed location, likely top-left corner (not clamped to cursor position).

---

### 4. Confidence Mode (§11.6)

**SPECIFICATION REQUIRES:**
- Track `transformCount` in UserDefaults
- Toast duration based on transform count:
  - 0–29: 1.5s
  - 30–39: 1.0s
  - 40–49: 0.5s
  - 50+: Toast disabled (icon flash only)
- User override: "Always show toast" or "Never show toast"

**IMPLEMENTATION:** **PARTIAL - DURATION LOGIC INCOMPLETE**

**EVIDENCE (main.swift:701-717):**
```swift
var transformCount: Int {
    get { UserDefaults.standard.integer(forKey: "ccvv_transforms") }
    set { UserDefaults.standard.set(newValue, forKey: "ccvv_transforms") }
}

var toastDuration: TimeInterval {
    let toastPref = UserDefaults.standard.string(forKey: "ccvv_toast_pref") ?? "auto"
    switch toastPref {
    case "never": return 0
    case "always": return 1.5
    case "auto":
        switch transformCount {
        case 0..<30: return 1.5
        case 30..<40: return 1.0
        case 40..<50: return 0.5
        default: return 0  // 50+ = icon flash only
        }
    default: return 1.5
    }
}
```

✓ transformCount tracked in UserDefaults  
✓ Duration schedule matches spec exactly  
✓ User overrides implemented in preferences

**Line 1077 increments counter after each clean:**
```swift
transformCount += 1
```

✓ Confidence mode COMPLETE

---

### 5. Pause/Resume (§11.2)

**SPECIFICATION REQUIRES:**
- `var isPaused: Bool = false`
- Menu item "Pause ccvv" / "Resume ccvv" (title toggles)
- Guard at top of `handleCGKeyEvent`: `if isPaused { return }`
- Icon changes to `[--]` with `.secondaryLabelColor`
- Transient state (resets on app launch)

**IMPLEMENTATION:** **COMPLETE**

**EVIDENCE (main.swift:686, 812-817, 914-918, 996, 855-857):**
```swift
var isPaused = false

pauseMenuItem = NSMenuItem(
    title: "Pause ccvv",
    action: #selector(togglePause),
    keyEquivalent: "p"
)

@objc func togglePause() {
    isPaused.toggle()
    pauseMenuItem.title = isPaused ? "Resume ccvv" : "Pause ccvv"
    updateIconState()
}

func handleCGKeyEvent(_ event: CGEvent) {
    if isPaused { return }
    ...
}

// Icon update:
} else if isPaused {
    button.title = "[--]"
    button.contentTintColor = .secondaryLabelColor
}
```

✓ All spec requirements met

---

### 6. Accessibility Permission Management (§11.1)

**SPECIFICATION REQUIRES:**
- Permission check on launch with prompt
- Periodic re-check every 30 seconds
- Manual mode fallback if permission denied
- Tap disable recovery (re-enable if tap is disabled by timeout/pressure)
- NSAccessibilityUsageDescription in Info.plist

**IMPLEMENTATION:** **PARTIAL - MISSING PERIODIC RE-CHECK & RECOVERY**

**EVIDENCE:**
- **Prompt on launch (main.swift:738-755):** ✓
  ```swift
  let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue(): true] as CFDictionary
  if AXIsProcessTrustedWithOptions(options) {
      registerEventTap()
  } else {
      enterManualMode()
  }
  ```

- **Info.plist (line 25-26):** ✓
  ```xml
  <key>NSAccessibilityUsageDescription</key>
  <string>ccvv needs Accessibility access to detect your double-tap copy shortcut...</string>
  ```

**MISSING:**
1. **Periodic re-check (30s timer)** - Spec requires `Timer.scheduledTimer(withTimeInterval: 30, repeats: true)` to re-enable event tap if permission is re-granted. Not found in main.swift.
2. **Tap disable recovery** - Spec requires handling `event.type == .tapDisabledByTimeout || .tapDisabledByUserInput` with `CGEvent.tapEnable(tap: eventTap, enable: true)`. Not found in event tap callback.
3. **accessibilityCheckTimer** - Created at line 769 but never checked in code search results.

---

### 7. Preferences Window (§11.8)

**SPECIFICATION REQUIRES:**
- Single NSWindow with toggle groups:
  - Text Cleanup (whitespace, Unicode, agent artifact stripping)
  - Formatting (JSON, table-to-Markdown, code fence, backtick auto-wrapper)
  - URLs (strip tracking, aggressive mode)
  - Privacy (sensitive filter, store raw in history)
  - Feedback (show HUD toast)
- Config write-back using `toml_edit` (preserves comments/formatting)
- [Open Config File] and [Reset to Defaults] buttons

**IMPLEMENTATION:** **PARTIAL - BASIC TOGGLES PRESENT, NO CONFIG WRITE-BACK**

**EVIDENCE (main.swift:1252-1318):**
```swift
class PreferencesWindowController: NSWindowController {
    // Setup creates toggles for:
    yOffset = addToggle("Whitespace & line break cleanup", feature: "whitespace_cleanup", ...)
    yOffset = addToggle("Unicode normalization", feature: "normalize_unicode", ...)
    yOffset = addToggle("Agent artifact stripping", feature: "agent_strip", ...)
    yOffset = addToggle("JSON detect & prettify...", feature: "structural_detection", ...)
    yOffset = addToggle("Backtick auto-wrapper...", feature: "auto_wrapper", ...)
    yOffset = addToggle("Strip tracking parameters", feature: "url_cleaning", ...)
    yOffset = addToggle("Sensitive content filter", feature: "sensitive_filter", ...)
    yOffset = addToastPrefControl(...)
    
    let openConfigButton = NSButton(title: "Open Config File", ...)
    let resetButton = NSButton(title: "Reset to Defaults", ...)
}
```

**MISSING:**
1. **toml_edit write-back** - Spec requires atomic write with comment preservation. No Swift code found implementing `toml_edit` Rust FFI calls or TOML serialization. Search for "toml" yields no matches in main.swift.
2. **mtime race condition handling** - Spec mentions comparing file mtime before writing. Not implemented.
3. **Toggle action handlers** - The `addToggle` helper creates UI but no evidence of save callbacks. Line 1379 shows `toastPrefChanged` but no config write.

---

### 8. History Panel (§11.9)

**SPECIFICATION REQUIRES:**
- NSPopover (not NSPanel) attached to status item button
- NSTableView with 3 columns: Preview (80 chars), Type badge (URL/Code/Prose/Table/JSON), Timestamp
- NSSearchField for filtering
- Click to restore cleaned/raw text
- Data via `ccvv_history_get_recent_json()` FFI call

**IMPLEMENTATION:** **PARTIAL - BASIC POPOVER & TABLE VIEW PRESENT**

**EVIDENCE (main.swift:930-943, 1426-1570):**
```swift
@objc func showHistoryPopover() {
    if let popover = historyPopover, popover.isShown {
        popover.performClose(nil)
        historyPopover = nil
        return
    }
    let popover = NSPopover()
    popover.contentSize = NSSize(width: 360, height: 400)
    popover.behavior = .transient
    popover.contentViewController = HistoryViewController()
    if let button = statusItem.button {
        popover.show(relativeTo: button.bounds, of: button, preferredEdge: .minY)
    }
    historyPopover = popover
}

class HistoryViewController: NSViewController {
    var tableView: NSTableView!
    var entries: [(id: String, preview: String, contentType: String, timestamp: String)] = []
    
    let previewCol = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("preview"))
    let typeCol = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("type"))
    let timeCol = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("time"))
    
    // NSSearchField at top
    let searchField = NSSearchField(frame: NSRect(x: 10, y: 360, width: 340, height: 24))
}
```

✓ NSPopover used (not NSPanel)  
✓ NSTableView with 3 columns  
✓ NSSearchField present  

**PARTIAL:**
1. **Type badges** - Table shows `contentType` string but no visual styling (colors/labels) mentioned in spec
2. **Data source** - Line 1481, 1491 call `ccvv_history_search_json` and `ccvv_history_get_recent_json` (FFI present)
3. **Click to restore** - Line 1523-1525 restores cleaned text to clipboard, but spec allows restore of raw if `history_store_raw` enabled

---

### 9. First-Run Onboarding (§11.10)

**SPECIFICATION REQUIRES:**
- Appears after Accessibility permission dialog (delay or observe `didBecomeActiveNotification`)
- Two instruction variants:
  - With Accessibility: "Copy something, then tap Cmd+C again within a beat. Watch the icon flash. That's it."
  - Without Accessibility: "Grant Accessibility access, then copy something and tap Cmd+C again within a beat."
- Product decision: "ccvv cleans your clipboard to plain text. Rich formatting will be removed."
- Privacy notice: "ccvv never connects to the internet. Your clipboard stays on your device."
- History caveat: "Clipboard history stores only cleaned text, not originals." (if `history_store_raw = false`)
- "Try it now" and "Dismiss" buttons
- Auto-dismiss on successful double-tap detection

**IMPLEMENTATION:** **PARTIAL - TEXT PRESENT, BEHAVIOR INCOMPLETE**

**EVIDENCE (main.swift:727-730, 1159-1238):**
```swift
// First-run check:
if !UserDefaults.standard.bool(forKey: "ccvv_onboarding_done") {
    // showOnboarding() called (delayed)
}

func showOnboarding() {
    let hasAccessibility = AXIsProcessTrusted()
    
    let descLabel = NSTextField(wrappingLabelWithString:
        "ccvv cleans your clipboard to plain text. Rich formatting will be removed. " +
        "Double-tap Cmd+C to activate."
    )
    
    let instructionText: String
    if hasAccessibility {
        instructionText = "Copy something, then tap Cmd+C again within a beat. Watch the icon flash. That's it."
    } else {
        instructionText = "Grant Accessibility access, then copy something and tap Cmd+C again within a beat."
    }
    
    let privacyLabel = NSTextField(wrappingLabelWithString:
        "ccvv never connects to the internet. Your clipboard stays on your device. " +
        "Clipboard history stores only cleaned text, not originals."
    )
    
    let dismissButton = NSButton(title: "Get Started", ...)
}
```

✓ Text messages match spec  
✓ Accessibility check and conditional instruction  
✓ Privacy notice present  

**MISSING:**
1. **Accessibility permission dialog timing** - Spec requires observing `didBecomeActiveNotification` to wait for system permission dialog. Not found.
2. **"Try it now" button** - Only "Get Started" button present; no "Try it now" CTA
3. **Auto-dismiss on successful double-tap** - Spec requires detecting successful double-tap and auto-dismissing. Instead, only manual dismiss button.
4. **Conditional privacy text** - History caveat always shown; should check `history_store_raw` config

---

### 10. Bypass Modifier (§11.7)

**SPECIFICATION REQUIRES:**
- Option key held during Cmd+C suppresses double-tap detection (early return)
- Check `!flags.contains(.maskAlternate)`

**IMPLEMENTATION:** **COMPLETE**

**EVIDENCE (main.swift:1006):**
```swift
guard keyCode == 8,
      flags.contains(.maskCommand),
      !flags.contains(.maskControl),
      !flags.contains(.maskAlternate),  // ← Option key bypass
      !isRepeat else {
    return
}
```

✓ Bypass modifier implemented

---

### 11. Adaptive Double-Tap Timing (§11.11)

**SPECIFICATION REQUIRES:**
- FFI functions: `ccvv_timing_record_sample(interval_ms)` and `ccvv_timing_get_threshold_ms()`
- Record sample after each successful double-tap
- Read threshold to set active window
- Config value takes precedence (disables adaptive if fixed)

**IMPLEMENTATION:** **COMPLETE**

**EVIDENCE (main.swift:604-611, 1016, 693-695):**
```swift
func recordTimingSample(intervalMs: UInt32) {
    ccvv_timing_record_sample(intervalMs)
}

var adaptiveThresholdMs: UInt32 {
    return ccvv_timing_get_threshold_ms()
}

var doubleCopyWindowSeconds: TimeInterval {
    let adaptive = CcvvCore.shared.adaptiveThresholdMs
    if adaptive > 0 {
        return TimeInterval(adaptive) / 1000.0
    }
    return CcvvCore.shared.doubleTapWindowSeconds
}

// In handleCGKeyEvent line 1016:
CcvvCore.shared.recordTimingSample(intervalMs: UInt32(elapsed * 1000))
```

✓ FFI calls present  
✓ Sample recorded after detection  
✓ Threshold read and applied

---

### 12. Two-Phase Commit (§9.2)

**SPECIFICATION REQUIRES:**
- Phase 1: `ccvv_history_prepare(history, raw, cleaned, storeRaw, &error)` → returns entry_id
- Phase 2: Write to clipboard via `pb.setString(cleaned, forType: .string)`
- Phase 3: `ccvv_history_commit(history, entry_id, &error)`
- Guard: `isWritingBack` flag prevents re-detection during write

**IMPLEMENTATION:** **PARTIAL - PHASES PRESENT, GUARD MISSING**

**EVIDENCE (main.swift:1073-1074, 614-636):**
```swift
// Phase 1: Prepare
CcvvCore.shared.recordHistory(raw: originalText, cleaned: cleaned)

// In CcvvCore:
func prepareHistory(raw: String, cleaned: String) -> Int64? {
    var error: UnsafeMutablePointer<CChar>?
    let entryId = ccvv_history_prepare(history, rawPtr, cleanPtr, storeRaw, &error)
    return entryId >= 0 ? entryId : nil
}

func commitHistory(entryId: Int64) {
    var error: UnsafeMutablePointer<CChar>?
    ccvv_history_commit(h, entryId, &error)
}

// Phase 2: Write (main.swift:1065-1066)
pb.clearContents()
guard pb.setString(cleaned, forType: .string) else { ... }
```

**MISSING:**
1. **isWritingBack guard** - Spec mentions setting `isWritingBack = true` before clipboard write and clearing after. Not found in code. This flag is crucial to prevent the write-back from being detected as a new user copy by changeCount monitoring (§5.6).
2. **Phase ordering** - Current code calls `recordHistory()` before clipboard write, but should be:
   - Prepare (uncommitted entry)
   - Write clipboard
   - Commit (mark as committed)
3. **Error handling in commit** - Spec requires commit to be called regardless of write success

---

### 13. Write-Back Guard (isWritingBack)

**SPECIFICATION REQUIRES (§5.6, §9.2):**
- `var isWritingBack = false` 
- Set to `true` before `pb.setString()`
- Record `expectedChangeCount = pb.changeCount` after write
- Clear flag after write
- In changeCount polling loop, skip notification if `isWritingBack && changeCount == expectedChangeCount`

**IMPLEMENTATION:** **MISSING**

**Evidence:** No occurrences of "isWritingBack" in main.swift (search performed). This is critical for preventing the app's own clipboard writes from triggering the double-tap detector.

---

### 14. Pasteboard Type Check

**SPECIFICATION REQUIRES (§9.2, §1343):**
- Check `pb.types?.contains(.string)` before reading
- Skip if pasteboard contains only files, images, or other non-text types
- Prevents corrupting Finder file copies or image pastes

**IMPLEMENTATION:** **PARTIAL - NO EXPLICIT TYPE CHECK**

**EVIDENCE (main.swift:1038, 1040):**
```swift
log("  clipboard types: \(pb.types?.map(\.rawValue) ?? [])")
guard let text = extractClipboardTextWithStyleHints(pb) else {
    log("  no text extracted from clipboard")
    return
}
```

**Analysis:**
- Code logs pasteboard types but doesn't explicitly check `.string` type
- `extractClipboardTextWithStyleHints()` tries to extract text but doesn't guard against non-text types
- If pasteboard contains only a file (Finder copy), `pb.string(forType: .string)` returns nil, so the function gracefully returns nil
- **Verdict:** Works by accident (graceful degradation) but lacks explicit type check mentioned in spec

---

### 15. App Exclusion Check

**SPECIFICATION REQUIRES (§1288):**
- Check `NSWorkspace.shared.frontmostApplication?.bundleIdentifier`
- Call `ccvv_is_app_excluded(config, bundleId)`
- Return immediately if excluded
- Prevents processing clipboard from password managers, terminals, etc.

**IMPLEMENTATION:** **MISSING FROM EVENT HANDLER**

**Evidence:**
- `CcvvCore.isAppExcluded()` exists (line 1449-1451):
  ```swift
  func isAppExcluded(_ bundleId: String) -> Bool {
      guard let c = config else { return false }
      return bundleId.withCString { ccvv_is_app_excluded(c, $0) }
  }
  ```
- BUT it's never called in `handleCGKeyEvent()` or `performClean()`
- Spec requires check at top of main thread dispatch

---

### 16. changeCount Polling (§5.6)

**SPECIFICATION REQUIRES:**
- After double-tap detected, poll changeCount every 10ms for up to 200ms
- Confirm clipboard updated before reading
- If no change after 200ms, abort (keystroke didn't produce a copy)

**IMPLEMENTATION:** **MISSING**

**Evidence:** No polling loop in code. The app delays by `postCopySettleDelaySeconds` (a fixed value) but doesn't confirm changeCount increment.

---

## Build System Assessment

### Does build.sh correctly build Rust + Swift?

**SPECIFICATION REQUIRES:**
- Build Rust static library: `cargo build --package ccvv-lib --release`
- Locate generated header: `ccvv-bridge.h`
- Link in Swift compilation: `-L <rust_lib_dir> -lccvv_lib`
- Support `--skip-rust` for dev

**IMPLEMENTATION:** **COMPLETE**

**EVIDENCE (mac/build.sh:23-59):**
```bash
if [[ "$SKIP_RUST" -eq 0 ]]; then
    (cd "$CORE_DIR" && cargo build --package ccvv-lib --release)
    RUST_OUT_DIR=$(cd "$CORE_DIR" && cargo metadata ...)
    HEADER_DIR=$(find "$RUST_OUT_DIR/release/build" -name "ccvv-bridge.h" ...)
    LIB_PATH="$RUST_OUT_DIR/release/libccvv_lib.a"
    cp "$HEADER_DIR/ccvv-bridge.h" "$BUILD_DIR/ccvv-bridge.h"
fi

# Lines 63-70:
swiftc -o "$BUILD_DIR/$APP_NAME" main.swift \
    -framework Cocoa \
    -framework ApplicationServices \
    -import-objc-header "$BUILD_DIR/ccvv-bridge.h" \
    -L "$(dirname "$LIB_PATH")" \
    -lccvv_lib \
    -lsqlite3 \
    -O
```

✓ Rust build integrated  
✓ Header generation and copying  
✓ Linker flags correct  
✓ --skip-rust support present

---

## Summary Table

| Feature | Status | Severity | Notes |
|---------|--------|----------|-------|
| **Icon States** | PARTIAL | High | Text-based instead of NSImage templates; no image assets |
| **Miss Indicator** | COMPLETE | - | ✓ highlight-based, 200ms |
| **HUD Toast** | PARTIAL | High | Missing cursor positioning & screen clamping; no sensitive/oversize feedback |
| **Confidence Mode** | COMPLETE | - | ✓ Duration schedule correct |
| **Pause/Resume** | COMPLETE | - | ✓ Full implementation |
| **Accessibility Mgmt** | PARTIAL | Medium | Missing 30s re-check timer & tap-disable recovery |
| **Preferences Window** | PARTIAL | High | Toggles present but no `toml_edit` write-back |
| **History Panel** | PARTIAL | Medium | NSPopover & NSTableView present but type badges not styled |
| **First-Run Onboarding** | PARTIAL | Medium | Text correct but missing async timing & "Try it now" button |
| **Bypass Modifier** | COMPLETE | - | ✓ maskAlternate check present |
| **Adaptive Timing** | COMPLETE | - | ✓ FFI calls and recording present |
| **Two-Phase Commit** | PARTIAL | High | Phase 1 & 3 present but ordering wrong; no isWritingBack guard |
| **Write-Back Guard** | MISSING | Critical | No isWritingBack flag to prevent re-detection |
| **Pasteboard Type Check** | PARTIAL | Medium | Works by accident via nil return but no explicit type check |
| **App Exclusion Check** | MISSING | Critical | API exists but never called in event handler |
| **changeCount Polling** | MISSING | High | No 10ms polling to confirm clipboard sync |
| **Build System** | COMPLETE | - | ✓ Rust + Swift integration correct |
| **Info.plist** | COMPLETE | - | ✓ NSAccessibilityUsageDescription present |

---

## Critical Issues (Blocking)

1. **Write-Back Guard Missing** (isWritingBack): Without this, the app will detect its own clipboard writes as user copies, causing infinite loops or double-processing
2. **App Exclusion Check Missing**: Even though the API exists, it's never called, so password managers and terminals are NOT excluded
3. **changeCount Polling Missing**: The app doesn't confirm clipboard synchronization, relying only on fixed delay
4. **Icon Template Images**: Using text instead of NSImage violates macOS HIG and will look poor on macOS 14+

---

## Medium Severity Issues

1. **HUD Toast Positioning**: Toast won't appear near cursor; fixed position likely off-screen
2. **Preferences Config Write-Back**: Settings changes are not persisted (UI updates only)
3. **Accessibility Re-Check**: If user grants permission after app starts, event tap won't be enabled
4. **Two-Phase Commit Ordering**: History prepare called before clipboard write (should be after to ensure clipboard write succeeds first)

---

## Low Severity Issues

1. **Toast Sensitive/Oversize Messages**: Generic message shown regardless of skip reason
2. **History Panel Type Badges**: Colored badges not styled (just plain text)
3. **Onboarding "Try it Now"**: Only "Get Started" button; no interactive demo

