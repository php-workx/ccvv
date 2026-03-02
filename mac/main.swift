import Cocoa
import ApplicationServices

// MARK: - Text Cleaning

struct ParagraphLine {
    let text: String
    let indent: Int
}

func ccvv(_ input: String) -> String {
    let normalized = input
        .replacingOccurrences(of: "\r\n", with: "\n")
        .replacingOccurrences(of: "\r", with: "\n")

    let rawLines = normalized.components(separatedBy: "\n")

    // Detect terminal width: most common raw line length (for lines > 40 chars)
    let rawLengths = rawLines.map(\.count).filter { $0 > 40 }
    let lengthCounts = Dictionary(rawLengths.map { ($0, 1) }, uniquingKeysWith: +)
    let terminalWidth = lengthCounts.max(by: { $0.value < $1.value }).flatMap {
        $0.value >= 3 ? $0.key : nil
    } ?? 0

    var blocks: [String] = []
    var paragraph: [ParagraphLine] = []
    var codeBlock: [String] = []
    var inCodeFence = false
    var codeFenceIndent = ""
    var prevRawLineLen = 0

    func flushParagraph() {
        guard !paragraph.isEmpty else { return }
        let compacted = compactParagraph(paragraph)
        if !compacted.isEmpty {
            blocks.append(compacted)
        }
        paragraph.removeAll()
    }

    func flushCodeBlock() {
        guard !codeBlock.isEmpty else { return }
        blocks.append(codeBlock.joined(separator: "\n"))
        codeBlock.removeAll()
    }

    for rawLine in rawLines {
        var line = rawLine.replacingOccurrences(of: "\\s+$", with: "", options: .regularExpression)

        // Collapse terminal padding: runs of 3+ spaces inside content → single space
        // Excessive leading whitespace (>20) = terminal artifact: treat as paragraph break
        var excessivePadding = false
        if !inCodeFence {
            let ws = leadingWhitespace(line)
            let body = String(line.dropFirst(ws.count))
            if !body.isEmpty {
                let collapsed = body.replacingOccurrences(of: " {3,}", with: " ", options: .regularExpression)
                if ws.count > 20 {
                    excessivePadding = true
                    line = collapsed
                } else {
                    line = ws + collapsed
                }
            }
        }

        if inCodeFence {
            if isCodeFenceLine(line) {
                codeBlock.append(canonicalFenceLine(line))
                flushCodeBlock()
                inCodeFence = false
                codeFenceIndent = ""
            } else {
                codeBlock.append(dedent(line, by: codeFenceIndent))
            }
            continue
        }

        if isCodeFenceLine(line) {
            flushParagraph()
            inCodeFence = true
            codeFenceIndent = leadingWhitespace(line)
            codeBlock = [canonicalFenceLine(line)]
            continue
        }

        var indent = leadingIndentCount(line)
        var cleaned = line.replacingOccurrences(of: "^\\s+", with: "", options: .regularExpression)
        if cleaned.hasPrefix("⏺") {
            cleaned = String(cleaned.dropFirst())
                .replacingOccurrences(of: "^\\s+", with: "", options: .regularExpression)
        }
        if cleaned.range(of: #"^[◦▪]\s+"#, options: .regularExpression) != nil {
            indent += 2
        }
        cleaned = normalizeBulletMarker(cleaned)

        if cleaned.isEmpty || excessivePadding {
            flushParagraph()
        }
        if !cleaned.isEmpty {
            // Detect implicit paragraph break in terminal text:
            // previous line was short (not wrapped) + ends with punctuation,
            // current line starts a new sentence or list item
            if terminalWidth > 0,
               !paragraph.isEmpty,
               prevRawLineLen > 0,
               prevRawLineLen < terminalWidth * 85 / 100,
               let prev = paragraph.last?.text,
               let lastChar = prev.last,
               ".!?:".contains(lastChar),
               looksLikeParagraphStart(cleaned) {
                flushParagraph()
            }
            paragraph.append(ParagraphLine(text: cleaned, indent: indent))
        }
        prevRawLineLen = rawLine.replacingOccurrences(of: "\\s+$", with: "", options: .regularExpression).count
    }

    flushParagraph()
    if !codeBlock.isEmpty {
        flushCodeBlock()
    }

    return blocks.joined(separator: "\n\n")
}

func looksLikeParagraphStart(_ line: String) -> Bool {
    if isListItem(line) { return true }
    guard let first = line.first else { return false }
    return first.isUppercase
}

func isListItem(_ line: String) -> Bool {
    line.hasPrefix("- ") || line.hasPrefix("* ") ||
    line.range(of: #"^[•◦▪]\s+"#, options: .regularExpression) != nil ||
    line.range(of: #"^\d+[.)\]] "#, options: .regularExpression) != nil
}

func normalizeBulletMarker(_ line: String) -> String {
    line.replacingOccurrences(of: #"^[•◦▪]\s+"#, with: "- ", options: .regularExpression)
}

func compactParagraph(_ lines: [ParagraphLine]) -> String {
    var outputLines: [String] = []
    var buffer = ""
    let baseIndent = lines.map(\.indent).min() ?? 0
    var lastListItemRelIndent = 0

    for entry in lines {
        let relativeIndent = max(0, entry.indent - baseIndent)
        if isListItem(entry.text) {
            if !buffer.isEmpty {
                outputLines.append(buffer)
                buffer = ""
            }
            lastListItemRelIndent = relativeIndent
            outputLines.append(String(repeating: " ", count: relativeIndent) + entry.text)
        } else if !outputLines.isEmpty &&
            isListItemWithOptionalIndent(outputLines.last!) &&
            buffer.isEmpty &&
            relativeIndent > lastListItemRelIndent {
            outputLines[outputLines.count - 1] += " " + entry.text
        } else {
            buffer = buffer.isEmpty ? entry.text : buffer + " " + entry.text
        }
    }
    if !buffer.isEmpty {
        outputLines.append(buffer)
    }

    return outputLines.joined(separator: "\n")
}

func isListItemWithOptionalIndent(_ line: String) -> Bool {
    isListItem(line.replacingOccurrences(of: "^\\s+", with: "", options: .regularExpression))
}

func isCodeFenceLine(_ line: String) -> Bool {
    line.trimmingCharacters(in: .whitespaces).hasPrefix("```")
}

func canonicalFenceLine(_ line: String) -> String {
    line.trimmingCharacters(in: .whitespaces)
}

func leadingWhitespace(_ line: String) -> String {
    String(line.prefix { $0 == " " || $0 == "\t" })
}

func dedent(_ line: String, by prefix: String) -> String {
    guard !prefix.isEmpty else { return line }
    guard line.hasPrefix(prefix) else { return line }
    return String(line.dropFirst(prefix.count))
}

func leadingIndentCount(_ line: String) -> Int {
    var count = 0
    for scalar in line.unicodeScalars {
        if scalar == "\t" {
            count += 4
        } else if scalar.properties.isWhitespace {
            count += 1
        } else {
            break
        }
    }
    return count
}

func extractClipboardTextWithStyleHints(_ pb: NSPasteboard) -> String? {
    let plainCandidate = pb.string(forType: .string).map(addInlineCodeMarkersToPlainText)
    let richCandidate = clipboardAttributedString(from: pb).map(addInlineCodeMarkers)
    return chooseBestClipboardCandidate(richCandidate: richCandidate, plainCandidate: plainCandidate)
}

func clipboardAttributedString(from pb: NSPasteboard) -> NSAttributedString? {
    if let rtf = pb.data(forType: .rtf),
       let attributed = try? NSAttributedString(
           data: rtf,
           options: [.documentType: NSAttributedString.DocumentType.rtf],
           documentAttributes: nil
       ) {
        return attributed
    }
    if let html = pb.data(forType: .html),
       let attributed = try? NSAttributedString(
           data: html,
           options: [.documentType: NSAttributedString.DocumentType.html],
           documentAttributes: nil
       ) {
        return attributed
    }
    return nil
}

func addInlineCodeMarkers(from attributed: NSAttributedString) -> String {
    let source = attributed.string as NSString
    let fullRange = NSRange(location: 0, length: attributed.length)
    let terminalContext = attributed.length > 0 && isAllMonospace(attributed)

    var output = ""
    attributed.enumerateAttributes(in: fullRange, options: []) { attributes, range, _ in
        let runText = source.substring(with: range)
        if terminalContext {
            // All-monospace source (terminal): only color distinguishes code
            let colored = (attributes[.foregroundColor] as? NSColor)
                .map { isLikelySyntaxColor($0) } ?? false
            if colored {
                output += wrapInBackticks(runText)
            } else {
                output += runText
            }
        } else {
            if isStyledCodeRun(attributes) {
                output += addInlineCodeMarkersToPlainText(runText)
            } else {
                output += runText
            }
        }
    }

    // Also run token-level heuristic inference as fallback
    return addInlineCodeMarkersToPlainText(output)
}

func chooseBestClipboardCandidate(
    richCandidate: String?,
    plainCandidate: String?
) -> String? {
    switch (richCandidate, plainCandidate) {
    case let (rich?, plain?):
        if rich.isEmpty { return plain }
        if plain.isEmpty { return rich }

        let richScore = structurePreservationScore(rich)
        let plainScore = structurePreservationScore(plain)
        if richScore == 0 && plainScore > 0 {
            return plain
        }
        if rich.count * 4 < plain.count * 3 {
            return plain
        }
        return rich
    case let (rich?, nil):
        if rich.isEmpty { return nil }
        let enriched = addInlineCodeMarkersToPlainText(rich)
        return enriched.isEmpty ? rich : enriched
    case let (nil, plain?):
        return plain.isEmpty ? nil : plain
    default:
        return nil
    }
}

func structurePreservationScore(_ text: String) -> Int {
    let listLines = countMatches(
        #"(?m)^\s*(?:[-*]\s+|\d+[.)\]]\s+)"#,
        in: text
    )
    let fenceLines = countMatches(#"(?m)^\s*```"#, in: text)
    let backticks = text.filter { $0 == "`" }.count / 2
    return (listLines * 3) + (fenceLines * 5) + min(backticks, 30)
}

func countMatches(_ pattern: String, in text: String) -> Int {
    guard let regex = try? NSRegularExpression(pattern: pattern, options: []) else {
        return 0
    }
    let range = NSRange(location: 0, length: (text as NSString).length)
    return regex.numberOfMatches(in: text, options: [], range: range)
}

func isStyledCodeRun(_ attributes: [NSAttributedString.Key: Any]) -> Bool {
    let monospaced = (attributes[.font] as? NSFont).map { isMonospacedFont($0) } ?? false
    let colored = (attributes[.foregroundColor] as? NSColor).map { isLikelySyntaxColor($0) } ?? false
    return monospaced || colored
}

func addInlineCodeMarkersToPlainText(_ text: String) -> String {
    let lines = text.components(separatedBy: "\n")
    var output: [String] = []
    var inFence = false

    for line in lines {
        if isCodeFenceLine(line) {
            output.append(line)
            inFence.toggle()
            continue
        }
        if inFence {
            output.append(line)
            continue
        }
        output.append(wrapCodeLikeTokensOutsideBackticks(in: line))
    }

    return output.joined(separator: "\n")
}

func wrapCodeLikeTokensOutsideBackticks(in line: String) -> String {
    let parts = line.split(separator: "`", omittingEmptySubsequences: false)
    if parts.isEmpty {
        return line
    }

    var rebuilt = ""
    for index in parts.indices {
        let segment = String(parts[index])
        if index.isMultiple(of: 2) {
            rebuilt += wrapCodeLikeTokensInSegment(segment)
        } else {
            rebuilt += segment
        }
        if index < parts.count - 1 {
            rebuilt += "`"
        }
    }
    return rebuilt
}

func wrapCodeLikeTokensInSegment(_ segment: String) -> String {
    let ns = segment as NSString
    let regex = try? NSRegularExpression(pattern: #"\S+"#)
    guard let regex else { return segment }

    var result = segment
    let matches = regex.matches(in: segment, range: NSRange(location: 0, length: ns.length))
    for match in matches.reversed() {
        let token = (result as NSString).substring(with: match.range)
        let wrapped = wrapTokenIfCodeLike(token)
        if wrapped != token,
           let range = Range(match.range, in: result) {
            result.replaceSubrange(range, with: wrapped)
        }
    }
    return result
}

func wrapTokenIfCodeLike(_ token: String) -> String {
    let leadingSet = CharacterSet(charactersIn: "([{\"'“‘")
    let trailingSet = CharacterSet(charactersIn: ".,;:!?)]}\"'”’")

    let (leading, coreAndTrailing) = splitLeading(token, charset: leadingSet)
    let (core, trailing) = splitTrailing(coreAndTrailing, charset: trailingSet)
    guard shouldWrapCodeToken(core) else { return token }
    return "\(leading)`\(core)`\(trailing)"
}

func splitLeading(_ text: String, charset: CharacterSet) -> (String, String) {
    var start = text.startIndex
    while start < text.endIndex {
        let scalar = text[start].unicodeScalars.first!
        if charset.contains(scalar) {
            start = text.index(after: start)
        } else {
            break
        }
    }
    return (String(text[..<start]), String(text[start...]))
}

func splitTrailing(_ text: String, charset: CharacterSet) -> (String, String) {
    if text.isEmpty { return ("", "") }
    var end = text.endIndex
    while end > text.startIndex {
        let previous = text.index(before: end)
        let scalar = text[previous].unicodeScalars.first!
        if charset.contains(scalar) {
            end = previous
        } else {
            break
        }
    }
    return (String(text[..<end]), String(text[end...]))
}

func shouldWrapCodeToken(_ token: String) -> Bool {
    guard !token.isEmpty, token.count >= 2, token.count <= 100 else {
        return false
    }
    guard !token.contains("`"),
          !token.hasPrefix("http://"),
          !token.hasPrefix("https://") else {
        return false
    }
    if token.hasPrefix("--") {
        return true
    }
    if token.contains("_") {
        return true
    }
    if token.contains("/") {
        if token.hasPrefix("/") ||
            token.hasPrefix("./") ||
            token.hasPrefix("../") ||
            token.contains(".") ||
            token.contains("-") ||
            token.contains(":") {
            return true
        }
    }
    if token.range(of: #"^[A-Za-z0-9._-]+\.[A-Za-z0-9]{1,8}(:\d+)?$"#, options: .regularExpression) != nil {
        return true
    }
    if token.range(of: #"^[A-Za-z]+[A-Z][A-Za-z0-9]*$"#, options: .regularExpression) != nil {
        return true
    }
    if token.range(of: #"^[A-Z][A-Z0-9_]{2,}$"#, options: .regularExpression) != nil {
        return true
    }
    return false
}

func isMonospacedFont(_ font: NSFont) -> Bool {
    if font.fontDescriptor.symbolicTraits.contains(.monoSpace) {
        return true
    }
    let name = "\(font.fontName) \(font.familyName ?? "")".lowercased()
    return name.contains("mono") ||
        name.contains("code") ||
        name.contains("menlo") ||
        name.contains("consolas")
}

func isLikelySyntaxColor(_ color: NSColor) -> Bool {
    guard let rgb = color.usingColorSpace(.deviceRGB) else {
        return false
    }

    var red: CGFloat = 0
    var green: CGFloat = 0
    var blue: CGFloat = 0
    var alpha: CGFloat = 0
    rgb.getRed(&red, green: &green, blue: &blue, alpha: &alpha)

    if alpha < 0.95 {
        return false
    }

    let maxComp = max(red, max(green, blue))
    let minComp = min(red, min(green, blue))
    let chroma = maxComp - minComp

    // Ignore near-gray text; syntax colors are usually more saturated.
    return chroma >= 0.08
}

func isAllMonospace(_ attributed: NSAttributedString) -> Bool {
    let fullRange = NSRange(location: 0, length: attributed.length)
    var allMono = true
    attributed.enumerateAttribute(.font, in: fullRange, options: []) { value, _, stop in
        guard let font = value as? NSFont else { return }
        if !isMonospacedFont(font) {
            allMono = false
            stop.pointee = true
        }
    }
    return allMono
}

func wrapInBackticks(_ text: String) -> String {
    guard !text.isEmpty, !text.contains("`") else { return text }

    var start = text.startIndex
    while start < text.endIndex && text[start].isWhitespace { start = text.index(after: start) }

    var end = text.endIndex
    while end > start {
        let prev = text.index(before: end)
        if text[prev].isWhitespace { end = prev } else { break }
    }

    guard start < end else { return text }
    return String(text[..<start]) + "`" + String(text[start..<end]) + "`" + String(text[end...])
}

// MARK: - CcvvCore (Rust FFI Wrapper)

/// Swift wrapper around the Rust ccvv-lib via C FFI.
/// Provides safe, ergonomic access to the Rust transform engine,
/// config system, and history database.
class CcvvCore {
    private var config: OpaquePointer?
    private var history: OpaquePointer?

    /// Shared singleton instance.
    static let shared = CcvvCore()

    private init() {
        // Load config (uses default path ~/.ccvv/config.toml)
        var configErr: UnsafeMutablePointer<CChar>?
        config = ccvv_load_config(nil, &configErr)
        if let err = configErr {
            let msg = String(cString: err)
            log("CcvvCore: config load note: \(msg)")
            ccvv_string_free(err)
        }

        // Open history DB (uses default path ~/.ccvv/history.db)
        var histErr: UnsafeMutablePointer<CChar>?
        history = ccvv_history_open(nil, &histErr)
        if let err = histErr {
            let msg = String(cString: err)
            log("CcvvCore: history open note: \(msg)")
            ccvv_string_free(err)
        }
    }

    deinit {
        if let config = config {
            ccvv_config_free(config)
        }
        if let history = history {
            ccvv_history_free(history)
        }
    }

    /// Transform text using the Rust pipeline.
    /// Falls back to the Swift ccvv() function if FFI fails.
    func transform(_ input: String) -> String {
        var errPtr: UnsafeMutablePointer<CChar>?
        let resultPtr: UnsafeMutablePointer<CChar>?

        if let config = config {
            resultPtr = ccvv_transform_n(input, config, &errPtr)
        } else {
            resultPtr = ccvv_transform(input, &errPtr)
        }

        if let err = errPtr {
            let msg = String(cString: err)
            log("CcvvCore: transform error: \(msg), falling back to Swift")
            ccvv_string_free(err)
            return ccvv(input)
        }

        guard let result = resultPtr else {
            log("CcvvCore: transform returned null, falling back to Swift")
            return ccvv(input)
        }

        let output = String(cString: result)
        ccvv_string_free(result)
        return output
    }

    /// Get the double-tap window in seconds from config.
    var doubleTapWindowSeconds: TimeInterval {
        let ms = ccvv_get_double_tap_window_ms(config)
        return TimeInterval(ms) / 1000.0
    }

    /// Check if an app is excluded by bundle ID.
    func isAppExcluded(bundleId: String) -> Bool {
        return ccvv_is_app_excluded(config, bundleId)
    }

    /// Check if a feature is enabled.
    func isFeatureEnabled(_ feature: String) -> Bool {
        return ccvv_is_feature_enabled(config, feature)
    }

    /// Record a timing sample for adaptive threshold.
    func recordTimingSample(intervalMs: UInt32) {
        ccvv_timing_record_sample(intervalMs)
    }

    /// Get the adaptive timing threshold (0 = use config default).
    var adaptiveThresholdMs: UInt32 {
        return ccvv_timing_get_threshold_ms()
    }

    /// Record a history entry (two-phase commit).
    func recordHistory(raw: String, cleaned: String, storeRaw: Bool = false) {
        guard let history = history else { return }

        var prepErr: UnsafeMutablePointer<CChar>?
        let entryId = ccvv_history_prepare(history, raw, cleaned, storeRaw, &prepErr)
        if let err = prepErr {
            let msg = String(cString: err)
            log("CcvvCore: history prepare error: \(msg)")
            ccvv_string_free(err)
            return
        }
        if entryId < 0 { return }

        var commitErr: UnsafeMutablePointer<CChar>?
        let ok = ccvv_history_commit(history, entryId, &commitErr)
        if let err = commitErr {
            let msg = String(cString: err)
            log("CcvvCore: history commit error: \(msg)")
            ccvv_string_free(err)
        }
        if !ok {
            log("CcvvCore: history commit failed for entry \(entryId)")
        }
    }

    /// Expose the history handle for direct FFI calls (e.g., from HistoryViewController).
    var historyHandle: OpaquePointer? { history }

    /// Get the raw text for undo (most recent uncommitted or committed entry).
    func undoRaw() -> String? {
        guard let history = history else { return nil }

        var errPtr: UnsafeMutablePointer<CChar>?
        let rawPtr = ccvv_history_undo_raw(history, &errPtr)
        if let err = errPtr {
            let msg = String(cString: err)
            log("CcvvCore: undo error: \(msg)")
            ccvv_string_free(err)
            return nil
        }
        guard let raw = rawPtr else { return nil }
        let result = String(cString: raw)
        ccvv_string_free(raw)
        return result
    }
}

// MARK: - App Delegate

let appVersion = "1.1.11"

let logFile: FileHandle? = {
    let path = NSHomeDirectory() + "/Library/Logs/ccvv.log"
    FileManager.default.createFile(atPath: path, contents: nil)
    return FileHandle(forWritingAtPath: path)
}()

func log(_ msg: String) {
    let ts = ISO8601DateFormatter().string(from: Date())
    let line = "[\(ts)] \(msg)\n"
    logFile?.seekToEndOfFile()
    logFile?.write(line.data(using: .utf8)!)
    fputs(line, stderr)
}

class AppDelegate: NSObject, NSApplicationDelegate {
    var statusItem: NSStatusItem!
    var eventTap: CFMachPort?
    var eventTapActive = false
    var lastCmdCAt: Date = .distantPast
    var menu: NSMenu!
    var isPaused = false
    var pauseMenuItem: NSMenuItem!
    var accessibilityCheckTimer: Timer?
    var preferencesWindow: PreferencesWindowController?
    var historyPopover: NSPopover?

    var doubleCopyWindowSeconds: TimeInterval {
        let adaptive = CcvvCore.shared.adaptiveThresholdMs
        if adaptive > 0 {
            return TimeInterval(adaptive) / 1000.0
        }
        return CcvvCore.shared.doubleTapWindowSeconds
    }
    let postCopySettleDelaySeconds: TimeInterval = 0.15

    /// Confidence mode: track transform count for toast duration
    var transformCount: Int {
        get { UserDefaults.standard.integer(forKey: "ccvv_transform_count") }
        set { UserDefaults.standard.set(newValue, forKey: "ccvv_transform_count") }
    }

    var toastDuration: TimeInterval {
        let toastPref = UserDefaults.standard.string(forKey: "ccvv_toast_pref") ?? "auto"
        switch toastPref {
        case "always": return 1.5
        case "never": return 0
        default: // "auto" — confidence mode
            switch transformCount {
            case 0..<30: return 1.5
            case 30..<40: return 1.0
            case 40..<50: return 0.5
            default: return 0  // 50+ = icon flash only
            }
        }
    }

    func applicationDidFinishLaunching(_: Notification) {
        setupStatusItem()
        checkAccessibilityAndSetup()
        startAccessibilityCheckTimer()

        // First-run onboarding (after a short delay to let Accessibility prompt appear first)
        if !UserDefaults.standard.bool(forKey: "ccvv_onboarding_done") {
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { [weak self] in
                self?.showOnboarding()
            }
        }
    }

    // MARK: - Accessibility Management

    func checkAccessibilityAndSetup() {
        let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue(): true] as CFDictionary
        if AXIsProcessTrustedWithOptions(options) {
            registerEventTap()
        } else {
            enterManualMode()
        }
    }

    func startAccessibilityCheckTimer() {
        accessibilityCheckTimer = Timer.scheduledTimer(withTimeInterval: 30, repeats: true) { [weak self] _ in
            guard let self = self else { return }
            let trusted = AXIsProcessTrusted()
            if trusted && !self.eventTapActive {
                self.registerEventTap()
                self.updateIconState()
            }
            if !trusted && self.eventTapActive {
                self.enterManualMode()
            }
        }
    }

    func enterManualMode() {
        eventTapActive = false
        if let tap = eventTap {
            CGEvent.tapEnable(tap: tap, enable: false)
        }
        eventTap = nil
        updateIconState()
        log("Entered manual mode — Accessibility permission not granted")
    }

    // MARK: - Status Item Setup

    func setupStatusItem() {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        if let button = statusItem.button {
            button.title = "[cc]"
            button.font = NSFont.monospacedSystemFont(ofSize: 12, weight: .medium)
            button.action = #selector(statusItemClicked(_:))
            button.target = self
            button.sendAction(on: [.leftMouseUp, .rightMouseUp])
        }

        buildMenu()
    }

    func buildMenu() {
        menu = NSMenu()

        let versionItem = NSMenuItem(title: "ccvv v\(appVersion)", action: nil, keyEquivalent: "")
        versionItem.isEnabled = false
        menu.addItem(versionItem)
        menu.addItem(NSMenuItem.separator())

        let cleanItem = NSMenuItem(
            title: "Clean Clipboard Now",
            action: #selector(cleanClipboardAction),
            keyEquivalent: ""
        )
        cleanItem.target = self
        menu.addItem(cleanItem)

        let undoItem = NSMenuItem(
            title: "Undo Last Clean",
            action: #selector(undoLastClean),
            keyEquivalent: "z"
        )
        undoItem.target = self
        menu.addItem(undoItem)

        menu.addItem(NSMenuItem.separator())

        pauseMenuItem = NSMenuItem(
            title: "Pause ccvv",
            action: #selector(togglePause),
            keyEquivalent: "p"
        )
        pauseMenuItem.target = self
        menu.addItem(pauseMenuItem)

        menu.addItem(NSMenuItem.separator())

        let historyItem = NSMenuItem(
            title: "History...",
            action: #selector(showHistoryPopover),
            keyEquivalent: "h"
        )
        historyItem.target = self
        menu.addItem(historyItem)

        let prefsItem = NSMenuItem(
            title: "Preferences...",
            action: #selector(showPreferences),
            keyEquivalent: ","
        )
        prefsItem.target = self
        menu.addItem(prefsItem)

        menu.addItem(NSMenuItem.separator())

        let quitItem = NSMenuItem(
            title: "Quit",
            action: #selector(quitApp),
            keyEquivalent: "q"
        )
        quitItem.target = self
        menu.addItem(quitItem)
    }

    // MARK: - Icon State Management

    func updateIconState() {
        guard let button = statusItem.button else { return }
        if !eventTapActive {
            button.title = "[!!]"
            button.contentTintColor = .systemOrange
        } else if isPaused {
            button.title = "[--]"
            button.contentTintColor = .secondaryLabelColor
        } else {
            button.title = "[cc]"
            button.contentTintColor = nil
        }
    }

    func showSuccessFlash() {
        guard let button = statusItem.button else { return }
        let originalTitle = button.title
        let originalTint = button.contentTintColor
        button.title = " ok "
        button.contentTintColor = .systemGreen
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { [weak self] in
            button.title = originalTitle
            button.contentTintColor = originalTint
            self?.updateIconState()
        }
    }

    func showMissIndicator() {
        guard let button = statusItem.button else { return }
        button.highlight(true)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) {
            button.highlight(false)
        }
    }

    // MARK: - Actions

    @objc func statusItemClicked(_ sender: NSStatusBarButton) {
        guard let event = NSApp.currentEvent else { return }
        if event.type == .rightMouseUp {
            statusItem.menu = menu
            statusItem.button?.performClick(nil)
            statusItem.menu = nil
        } else {
            performClean()
        }
    }

    @objc func cleanClipboardAction() {
        performClean()
    }

    @objc func undoLastClean() {
        guard let rawText = CcvvCore.shared.undoRaw() else {
            log("Undo: no raw text available")
            return
        }
        let pb = NSPasteboard.general
        pb.clearContents()
        pb.setString(rawText, forType: .string)
        log("Undo: restored raw text (\(rawText.count) chars)")
        showHUDToast("Restored original clipboard")
    }

    @objc func togglePause() {
        isPaused.toggle()
        pauseMenuItem.title = isPaused ? "Resume ccvv" : "Pause ccvv"
        updateIconState()
        log(isPaused ? "Paused" : "Resumed")
    }

    @objc func showPreferences() {
        if preferencesWindow == nil {
            preferencesWindow = PreferencesWindowController()
        }
        preferencesWindow?.showWindow(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    @objc func showHistoryPopover() {
        if let popover = historyPopover, popover.isShown {
            popover.close()
            return
        }

        let popover = NSPopover()
        popover.contentSize = NSSize(width: 360, height: 400)
        popover.behavior = .transient
        popover.contentViewController = HistoryViewController()

        if let button = statusItem.button {
            popover.show(relativeTo: button.bounds, of: button, preferredEdge: .minY)
        }
        self.historyPopover = popover
    }

    @objc func quitApp() {
        NSApplication.shared.terminate(nil)
    }

    // MARK: - Event Tap

    func registerEventTap() {
        if eventTapActive { return }

        let eventMask: CGEventMask = (1 << CGEventType.keyDown.rawValue)
            | (1 << CGEventType.tapDisabledByTimeout.rawValue)
            | (1 << CGEventType.tapDisabledByUserInput.rawValue)

        guard let tap = CGEvent.tapCreate(
            tap: .cgSessionEventTap,
            place: .headInsertEventTap,
            options: .listenOnly,
            eventsOfInterest: eventMask,
            callback: { _, type, event, refcon -> Unmanaged<CGEvent>? in
                let delegate = Unmanaged<AppDelegate>.fromOpaque(refcon!).takeUnretainedValue()

                // Handle tap-disable recovery
                if type == .tapDisabledByTimeout || type == .tapDisabledByUserInput {
                    if let tap = delegate.eventTap {
                        CGEvent.tapEnable(tap: tap, enable: true)
                        log("Event tap was disabled by system, re-enabled")
                    }
                    return Unmanaged.passRetained(event)
                }

                delegate.handleCGKeyEvent(event)
                return Unmanaged.passRetained(event)
            },
            userInfo: Unmanaged.passUnretained(self).toOpaque()
        ) else {
            log("Failed to create event tap — Accessibility permission missing?")
            enterManualMode()
            return
        }

        eventTap = tap
        eventTapActive = true
        let source = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0)
        CFRunLoopAddSource(CFRunLoopGetCurrent(), source, .commonModes)
        CGEvent.tapEnable(tap: tap, enable: true)
        updateIconState()
        log("CGEventTap registered")
    }

    func handleCGKeyEvent(_ event: CGEvent) {
        if isPaused { return }

        let flags = event.flags
        let keyCode = event.getIntegerValueField(.keyboardEventKeycode)
        let isRepeat = event.getIntegerValueField(.keyboardEventAutorepeat) != 0

        // keyCode 8 = 'c'
        guard keyCode == 8,
              flags.contains(.maskCommand),
              !flags.contains(.maskControl),
              !flags.contains(.maskAlternate),
              !isRepeat else {
            return
        }

        let now = Date()
        let elapsed = now.timeIntervalSince(lastCmdCAt)

        if elapsed <= doubleCopyWindowSeconds {
            lastCmdCAt = .distantPast
            CcvvCore.shared.recordTimingSample(intervalMs: UInt32(elapsed * 1000))
            log("double Cmd+C detected (interval=\(String(format: "%.2f", elapsed))s)")
            DispatchQueue.main.asyncAfter(deadline: .now() + postCopySettleDelaySeconds) { [weak self] in
                self?.performClean()
            }
        } else {
            // Miss indicator: near-miss detection
            if elapsed > doubleCopyWindowSeconds && elapsed < doubleCopyWindowSeconds * 2.0 {
                DispatchQueue.main.async { [weak self] in
                    self?.showMissIndicator()
                }
            }
            lastCmdCAt = now
        }
    }

    // MARK: - Clipboard Cleaning

    func performClean() {
        let pb = NSPasteboard.general

        log("performClean triggered")
        log("  clipboard types: \(pb.types?.map(\.rawValue) ?? [])")

        guard let text = extractClipboardTextWithStyleHints(pb) else {
            log("  no text extracted from clipboard")
            showFeedback(success: false, message: nil)
            return
        }

        let rawClip = pb.string(forType: .string) ?? ""
        log("  raw clipboard lines:")
        for (i, line) in rawClip.components(separatedBy: "\n").prefix(30).enumerated() {
            log("    [\(i)] (\(line.count)ch): \(String(line.prefix(120)))")
        }
        log("  extracted (\(text.count) chars): \(String(text.prefix(500)))")

        let originalText = pb.string(forType: .string) ?? text
        let cleaned = CcvvCore.shared.transform(text)

        log("  cleaned (\(cleaned.count) chars): \(String(cleaned.prefix(500)))")

        // Check if no changes
        if cleaned == originalText {
            log("  no changes needed")
            showFeedback(success: true, message: "No changes needed")
            return
        }

        pb.clearContents()
        guard pb.setString(cleaned, forType: .string) else {
            pb.clearContents()
            _ = pb.setString(originalText, forType: .string)
            showFeedback(success: false, message: nil)
            return
        }

        // Record history entry
        CcvvCore.shared.recordHistory(raw: originalText, cleaned: cleaned)

        // Increment confidence counter
        transformCount += 1

        let charDiff = originalText.count - cleaned.count
        let message: String
        if charDiff > 0 {
            message = "Cleaned (\(charDiff) chars removed)"
        } else if charDiff < 0 {
            message = "Formatted (\(-charDiff) chars added)"
        } else {
            message = "Cleaned (content reformatted)"
        }

        showFeedback(success: true, message: message)
    }

    // MARK: - Feedback

    func showFeedback(success: Bool, message: String?) {
        if success {
            showSuccessFlash()
        }
        if let msg = message, toastDuration > 0 {
            showHUDToast(msg)
        }
    }

    func showHUDToast(_ message: String) {
        let duration = toastDuration
        guard duration > 0 else { return }

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
        label.font = NSFont.systemFont(ofSize: 12, weight: .medium)
        label.alignment = .center
        label.frame = NSRect(x: 10, y: 8, width: 300, height: 24)
        toast.contentView?.addSubview(label)

        // Round corners
        toast.contentView?.wantsLayer = true
        toast.contentView?.layer?.cornerRadius = 8
        toast.contentView?.layer?.masksToBounds = true

        // Position near cursor
        let mouseLocation = NSEvent.mouseLocation
        let screen = NSScreen.screens.first(where: { NSMouseInRect(mouseLocation, $0.frame, false) })
            ?? NSScreen.main ?? NSScreen.screens[0]
        let visibleFrame = screen.visibleFrame
        var origin = CGPoint(x: mouseLocation.x + 20, y: mouseLocation.y - 60)
        origin.x = min(origin.x, visibleFrame.maxX - toast.frame.width)
        origin.x = max(origin.x, visibleFrame.minX)
        origin.y = max(origin.y, visibleFrame.minY)
        toast.setFrameOrigin(origin)

        toast.orderFront(nil)
        toast.alphaValue = 1.0

        // Fade out after duration
        DispatchQueue.main.asyncAfter(deadline: .now() + duration) {
            NSAnimationContext.runAnimationGroup { context in
                context.duration = 0.3
                toast.animator().alphaValue = 0
            } completionHandler: {
                toast.orderOut(nil)
            }
        }
    }

    // MARK: - Onboarding

    func showOnboarding() {
        let hasAccessibility = AXIsProcessTrusted()

        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 420, height: 340),
            styleMask: [.titled, .closable],
            backing: .buffered,
            defer: false
        )
        window.title = "Welcome to ccvv"
        window.center()

        let contentView = NSView(frame: window.contentView!.bounds)
        contentView.autoresizingMask = [.width, .height]
        window.contentView = contentView

        var yOffset: CGFloat = 290

        let titleLabel = NSTextField(labelWithString: "ccvv - clipboard text sanitizer")
        titleLabel.font = NSFont.systemFont(ofSize: 18, weight: .bold)
        titleLabel.frame = NSRect(x: 20, y: yOffset, width: 380, height: 30)
        contentView.addSubview(titleLabel)
        yOffset -= 40

        let descLabel = NSTextField(wrappingLabelWithString:
            "ccvv cleans your clipboard to plain text. Rich formatting will be removed. " +
            "Double-tap Cmd+C to activate."
        )
        descLabel.font = NSFont.systemFont(ofSize: 13)
        descLabel.frame = NSRect(x: 20, y: yOffset, width: 380, height: 50)
        contentView.addSubview(descLabel)
        yOffset -= 60

        let instructionText: String
        if hasAccessibility {
            instructionText = "Copy something, then tap Cmd+C again within a beat. Watch the icon flash. That's it."
        } else {
            instructionText = "Grant Accessibility access, then copy something and tap Cmd+C again within a beat."
        }

        let instructLabel = NSTextField(wrappingLabelWithString: instructionText)
        instructLabel.font = NSFont.systemFont(ofSize: 13, weight: .medium)
        instructLabel.frame = NSRect(x: 20, y: yOffset, width: 380, height: 50)
        contentView.addSubview(instructLabel)
        yOffset -= 60

        let privacyLabel = NSTextField(wrappingLabelWithString:
            "ccvv never connects to the internet. Your clipboard stays on your device. " +
            "Clipboard history stores only cleaned text, not originals."
        )
        privacyLabel.font = NSFont.systemFont(ofSize: 11)
        privacyLabel.textColor = .secondaryLabelColor
        privacyLabel.frame = NSRect(x: 20, y: yOffset, width: 380, height: 50)
        contentView.addSubview(privacyLabel)

        let dismissButton = NSButton(title: "Get Started", target: nil, action: nil)
        dismissButton.frame = NSRect(x: 300, y: 20, width: 100, height: 32)
        dismissButton.bezelStyle = .rounded
        dismissButton.keyEquivalent = "\r"
        contentView.addSubview(dismissButton)

        // Use a closure-based action
        class DismissTarget: NSObject {
            let window: NSWindow
            init(window: NSWindow) { self.window = window }
            @objc func dismiss() {
                UserDefaults.standard.set(true, forKey: "ccvv_onboarding_done")
                window.close()
            }
        }
        let target = DismissTarget(window: window)
        dismissButton.target = target
        dismissButton.action = #selector(DismissTarget.dismiss)

        // Keep reference alive
        objc_setAssociatedObject(window, "dismissTarget", target, .OBJC_ASSOCIATION_RETAIN)

        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    // MARK: - Lifecycle

    func applicationWillTerminate(_: Notification) {
        accessibilityCheckTimer?.invalidate()
        if let eventTap {
            CGEvent.tapEnable(tap: eventTap, enable: false)
        }
    }
}

// MARK: - Preferences Window

class PreferencesWindowController: NSWindowController {
    convenience init() {
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 440, height: 520),
            styleMask: [.titled, .closable],
            backing: .buffered,
            defer: false
        )
        window.title = "ccvv Preferences"
        window.center()
        self.init(window: window)
        setupUI()
    }

    private func setupUI() {
        guard let contentView = window?.contentView else { return }
        contentView.autoresizingMask = [.width, .height]

        let scrollView = NSScrollView(frame: contentView.bounds)
        scrollView.autoresizingMask = [.width, .height]
        scrollView.hasVerticalScroller = true
        scrollView.drawsBackground = false

        let documentView = NSView(frame: NSRect(x: 0, y: 0, width: 420, height: 480))
        scrollView.documentView = documentView
        contentView.addSubview(scrollView)

        var yOffset: CGFloat = 450

        // Section: Text Cleanup
        yOffset = addSectionHeader("Text Cleanup", to: documentView, y: yOffset)
        yOffset = addToggle("Whitespace & line break cleanup", feature: "whitespace_cleanup", to: documentView, y: yOffset)
        yOffset = addToggle("Unicode normalization", feature: "normalize_unicode", to: documentView, y: yOffset)
        yOffset = addToggle("Agent artifact stripping", feature: "agent_strip", to: documentView, y: yOffset)
        yOffset -= 10

        // Section: Formatting
        yOffset = addSectionHeader("Formatting", to: documentView, y: yOffset)
        yOffset = addToggle("JSON detect & prettify / Table-to-Markdown / Code fence", feature: "structural_detection", to: documentView, y: yOffset)
        yOffset = addToggle("Backtick auto-wrapper (shell safety risk)", feature: "auto_wrapper", to: documentView, y: yOffset)
        yOffset -= 10

        // Section: URLs
        yOffset = addSectionHeader("URLs", to: documentView, y: yOffset)
        yOffset = addToggle("Strip tracking parameters", feature: "url_cleaning", to: documentView, y: yOffset)
        yOffset -= 10

        // Section: Privacy
        yOffset = addSectionHeader("Privacy", to: documentView, y: yOffset)
        yOffset = addToggle("Sensitive content filter", feature: "sensitive_filter", to: documentView, y: yOffset)
        yOffset -= 10

        // Section: Feedback
        yOffset = addSectionHeader("Feedback", to: documentView, y: yOffset)
        yOffset = addToastPrefControl(to: documentView, y: yOffset)
        yOffset -= 20

        // Buttons
        let openConfigButton = NSButton(title: "Open Config File", target: self, action: #selector(openConfigFile))
        openConfigButton.frame = NSRect(x: 20, y: yOffset, width: 150, height: 32)
        openConfigButton.bezelStyle = .rounded
        documentView.addSubview(openConfigButton)

        let resetButton = NSButton(title: "Reset to Defaults", target: self, action: #selector(resetToDefaults))
        resetButton.frame = NSRect(x: 180, y: yOffset, width: 150, height: 32)
        resetButton.bezelStyle = .rounded
        documentView.addSubview(resetButton)
    }

    private func addSectionHeader(_ title: String, to view: NSView, y: CGFloat) -> CGFloat {
        let label = NSTextField(labelWithString: title)
        label.font = NSFont.systemFont(ofSize: 13, weight: .bold)
        label.frame = NSRect(x: 20, y: y, width: 380, height: 20)
        view.addSubview(label)
        return y - 28
    }

    private func addToggle(_ title: String, feature: String, to view: NSView, y: CGFloat) -> CGFloat {
        let checkbox = NSButton(checkboxWithTitle: title, target: self, action: #selector(toggleChanged(_:)))
        checkbox.frame = NSRect(x: 30, y: y, width: 380, height: 20)
        checkbox.state = CcvvCore.shared.isFeatureEnabled(feature) ? .on : .off
        checkbox.identifier = NSUserInterfaceItemIdentifier(feature)
        view.addSubview(checkbox)
        return y - 26
    }

    private func addToastPrefControl(to view: NSView, y: CGFloat) -> CGFloat {
        let label = NSTextField(labelWithString: "HUD Toast:")
        label.font = NSFont.systemFont(ofSize: 12)
        label.frame = NSRect(x: 30, y: y, width: 80, height: 20)
        view.addSubview(label)

        let popup = NSPopUpButton(frame: NSRect(x: 110, y: y - 2, width: 160, height: 24), pullsDown: false)
        popup.addItems(withTitles: ["Auto (confidence mode)", "Always show", "Never show"])
        let pref = UserDefaults.standard.string(forKey: "ccvv_toast_pref") ?? "auto"
        switch pref {
        case "always": popup.selectItem(at: 1)
        case "never": popup.selectItem(at: 2)
        default: popup.selectItem(at: 0)
        }
        popup.target = self
        popup.action = #selector(toastPrefChanged(_:))
        view.addSubview(popup)
        return y - 30
    }

    @objc func toggleChanged(_ sender: NSButton) {
        guard let feature = sender.identifier?.rawValue else { return }
        let enabled = sender.state == .on
        let configPath = NSHomeDirectory() + "/.ccvv/config.toml"

        // Ensure directory exists
        let configDir = NSHomeDirectory() + "/.ccvv"
        try? FileManager.default.createDirectory(atPath: configDir, withIntermediateDirectories: true)

        var errPtr: UnsafeMutablePointer<CChar>?
        let ok = ccvv_config_set_bool(feature, enabled, configPath, &errPtr)
        if let err = errPtr {
            let msg = String(cString: err)
            log("Preferences: failed to write \(feature)=\(enabled): \(msg)")
            ccvv_string_free(err)
        }
        if ok {
            log("Preferences: set \(feature) = \(enabled)")
        }
    }

    @objc func toastPrefChanged(_ sender: NSPopUpButton) {
        let values = ["auto", "always", "never"]
        let pref = values[sender.indexOfSelectedItem]
        UserDefaults.standard.set(pref, forKey: "ccvv_toast_pref")
        log("Preferences: toast pref = \(pref)")
    }

    @objc func openConfigFile() {
        let configPath = NSHomeDirectory() + "/.ccvv/config.toml"
        let configDir = NSHomeDirectory() + "/.ccvv"

        // Create default config if it does not exist
        if !FileManager.default.fileExists(atPath: configPath) {
            try? FileManager.default.createDirectory(atPath: configDir, withIntermediateDirectories: true)
            let defaultConfig = """
            # ccvv configuration
            # See: https://github.com/ccvv/ccvv

            [settings]
            # normalize_unicode = true
            # whitespace_cleanup = true
            # agent_strip = true
            # structural_detection = true
            # url_cleaning = true
            # auto_wrapper = false
            # sensitive_filter = true
            """
            try? defaultConfig.write(toFile: configPath, atomically: true, encoding: .utf8)
        }

        NSWorkspace.shared.open(URL(fileURLWithPath: configPath))
    }

    @objc func resetToDefaults() {
        let configPath = NSHomeDirectory() + "/.ccvv/config.toml"
        try? FileManager.default.removeItem(atPath: configPath)
        log("Preferences: reset to defaults (config file removed)")
        // Refresh the window
        if let window = self.window {
            window.contentView?.subviews.forEach { $0.removeFromSuperview() }
            setupUI()
        }
    }
}

// MARK: - History View Controller

class HistoryViewController: NSViewController {
    var tableView: NSTableView!
    var searchField: NSSearchField!
    var entries: [(preview: String, contentType: String, timestamp: String)] = []

    override func loadView() {
        let container = NSView(frame: NSRect(x: 0, y: 0, width: 360, height: 400))
        self.view = container

        // Search field
        searchField = NSSearchField(frame: NSRect(x: 10, y: 365, width: 340, height: 28))
        searchField.placeholderString = "Search history..."
        searchField.target = self
        searchField.action = #selector(searchChanged(_:))
        container.addSubview(searchField)

        // Scroll view with table
        let scrollView = NSScrollView(frame: NSRect(x: 0, y: 0, width: 360, height: 360))
        scrollView.hasVerticalScroller = true
        scrollView.autoresizingMask = [.width, .height]

        tableView = NSTableView()
        tableView.delegate = self
        tableView.dataSource = self

        let previewCol = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("preview"))
        previewCol.title = "Preview"
        previewCol.width = 220
        tableView.addTableColumn(previewCol)

        let typeCol = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("type"))
        typeCol.title = "Type"
        typeCol.width = 50
        tableView.addTableColumn(typeCol)

        let timeCol = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("time"))
        timeCol.title = "Time"
        timeCol.width = 60
        tableView.addTableColumn(timeCol)

        tableView.target = self
        tableView.doubleAction = #selector(rowDoubleClicked)

        scrollView.documentView = tableView
        container.addSubview(scrollView)

        loadHistory(query: nil)
    }

    func loadHistory(query: String?) {
        entries.removeAll()

        let jsonStr: String?
        if let q = query, !q.isEmpty {
            var errPtr: UnsafeMutablePointer<CChar>?
            let ptr = ccvv_history_search_json(CcvvCore.shared.historyHandle, q, 50, &errPtr)
            if let err = errPtr { ccvv_string_free(err) }
            if let p = ptr {
                jsonStr = String(cString: p)
                ccvv_string_free(p)
            } else {
                jsonStr = nil
            }
        } else {
            var errPtr: UnsafeMutablePointer<CChar>?
            let ptr = ccvv_history_get_recent_json(CcvvCore.shared.historyHandle, 50, &errPtr)
            if let err = errPtr { ccvv_string_free(err) }
            if let p = ptr {
                jsonStr = String(cString: p)
                ccvv_string_free(p)
            } else {
                jsonStr = nil
            }
        }

        if let json = jsonStr,
           let data = json.data(using: .utf8),
           let array = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] {
            for item in array {
                let preview = (item["preview"] as? String) ?? ""
                let ct = (item["content_type"] as? String) ?? "unknown"
                let ts = (item["created_at"] as? String) ?? ""
                entries.append((preview: String(preview.prefix(80)), contentType: ct, timestamp: relativeTime(ts)))
            }
        }

        tableView?.reloadData()
    }

    @objc func searchChanged(_ sender: NSSearchField) {
        loadHistory(query: sender.stringValue)
    }

    @objc func rowDoubleClicked() {
        let row = tableView.selectedRow
        guard row >= 0, row < entries.count else { return }
        // Copy the preview text to clipboard (the full cleaned text would require another FFI call)
        let pb = NSPasteboard.general
        pb.clearContents()
        pb.setString(entries[row].preview, forType: .string)
        log("History: restored entry to clipboard")

        // Dismiss the popover
        if let popover = (NSApp.delegate as? AppDelegate)?.historyPopover {
            popover.close()
        }
    }

    func relativeTime(_ isoString: String) -> String {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        guard let date = formatter.date(from: isoString) else {
            // Try without fractional seconds
            let basic = ISO8601DateFormatter()
            guard let d = basic.date(from: isoString) else { return isoString }
            return relativeTimeFromDate(d)
        }
        return relativeTimeFromDate(date)
    }

    func relativeTimeFromDate(_ date: Date) -> String {
        let elapsed = -date.timeIntervalSinceNow
        if elapsed < 60 { return "now" }
        if elapsed < 3600 { return "\(Int(elapsed / 60))m ago" }
        if elapsed < 86400 { return "\(Int(elapsed / 3600))h ago" }
        return "\(Int(elapsed / 86400))d ago"
    }
}

extension HistoryViewController: NSTableViewDataSource, NSTableViewDelegate {
    func numberOfRows(in tableView: NSTableView) -> Int {
        return entries.count
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard row < entries.count else { return nil }
        let entry = entries[row]
        let cell = NSTextField(labelWithString: "")
        cell.lineBreakMode = .byTruncatingTail
        cell.font = NSFont.systemFont(ofSize: 11)

        switch tableColumn?.identifier.rawValue {
        case "preview": cell.stringValue = entry.preview
        case "type": cell.stringValue = entry.contentType
        case "time": cell.stringValue = entry.timestamp
        default: break
        }
        return cell
    }
}

// MARK: - Entry Point

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.run()
