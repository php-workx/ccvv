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
        _ = reloadConfig()

        // Open history DB (uses default path ~/.ccvv/history.db)
        var histErr: UnsafeMutablePointer<CChar>?
        history = ccvv_history_open(nil, &histErr)
        if let err = histErr {
            let msg = String(cString: err)
            log("CcvvCore: history open note: \(msg)")
            ccvv_string_free(err)
        }
    }

    /// Reload config from disk and swap into the active pipeline.
    @discardableResult
    func reloadConfig() -> Bool {
        var configErr: UnsafeMutablePointer<CChar>?
        let newConfig = ccvv_load_config(nil, &configErr)
        if let err = configErr {
            let msg = String(cString: err)
            log("CcvvCore: config load note: \(msg)")
            ccvv_string_free(err)
        }
        guard let loaded = newConfig else {
            log("CcvvCore: config reload failed; keeping previous config")
            return false
        }
        if let existing = config {
            ccvv_config_free(existing)
        }
        config = loaded
        return true
    }

    deinit {
        if let config = config {
            ccvv_config_free(config)
        }
        if let history = history {
            ccvv_history_free(history)
        }
    }

    /// Transform result with metadata from the Rust pipeline.
    struct TransformResult {
        let cleanedText: String
        let summary: String
        let rulesCount: UInt32
        let skippedSensitive: Bool
        let skippedOversize: Bool
    }

    struct TableExtractionResult: Decodable {
        let detected: Bool
        let format: String?
        let confidence: Double
        let rows: [[String]]
        let warnings: [String]
    }

    /// Transform text using the Rust pipeline.
    /// Falls back to the Swift ccvv() function if FFI fails.
    func transform(_ input: String) -> String {
        return transformWithResult(input).cleanedText
    }

    /// Transform text and return full result with metadata.
    func transformWithResult(_ input: String) -> TransformResult {
        var errPtr: UnsafeMutablePointer<CChar>?
        let ffiResult: CcvvTransformResult

        if let config = config {
            ffiResult = ccvv_transform_n(input, config, &errPtr)
        } else {
            ffiResult = ccvv_transform(input, &errPtr)
        }

        if let err = errPtr {
            let msg = String(cString: err)
            log("CcvvCore: transform error: \(msg), falling back to Swift")
            ccvv_string_free(err)
            return TransformResult(cleanedText: ccvv(input), summary: "fallback to Swift",
                                   rulesCount: 0, skippedSensitive: false, skippedOversize: false)
        }

        guard let cleanedPtr = ffiResult.cleaned_text else {
            log("CcvvCore: transform returned null, falling back to Swift")
            return TransformResult(cleanedText: ccvv(input), summary: "fallback to Swift",
                                   rulesCount: 0, skippedSensitive: false, skippedOversize: false)
        }

        let output = String(cString: cleanedPtr)
        let summary = ffiResult.summary != nil ? String(cString: ffiResult.summary) : "no changes"
        let result = TransformResult(
            cleanedText: output,
            summary: summary,
            rulesCount: ffiResult.rules_count,
            skippedSensitive: ffiResult.skipped_sensitive,
            skippedOversize: ffiResult.skipped_oversize
        )
        ccvv_transform_result_free(ffiResult)
        return result
    }

    /// Parse clipboard text into table rows/columns for cell picker UI.
    func extractTable(_ input: String) -> TableExtractionResult? {
        var errPtr: UnsafeMutablePointer<CChar>?
        let jsonPtr = ccvv_extract_table_json(input, config, &errPtr)

        if let err = errPtr {
            let msg = String(cString: err)
            log("CcvvCore: table extract error: \(msg)")
            ccvv_string_free(err)
            return nil
        }
        guard let ptr = jsonPtr else { return nil }
        let json = String(cString: ptr)
        ccvv_string_free(ptr)
        guard let data = json.data(using: .utf8) else { return nil }
        return try? JSONDecoder().decode(TableExtractionResult.self, from: data)
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

let appVersion = "1.4.0"

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
    struct PendingCleanupCandidate {
        let createdAt: Date
        let sourceChangeCount: Int
        let rawText: String
        let cleanedText: String
        let summary: String
        let skippedSensitive: Bool
        let skippedOversize: Bool
        let tableExtraction: CcvvCore.TableExtractionResult?
    }

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
    var tablePickerWindow: TableCellPickerWindowController?
    var isWritingBack = false
    var writeBackChangeCount: Int = 0
    var pendingCleanupCandidate: PendingCleanupCandidate?
    var pendingCandidateExpiryTimer: Timer?
    var precomputeGeneration: UInt64 = 0
    var onboardingObserver: Any?
    var onboardingWindow: NSWindow?
    var onboardingCleanObserver: Any?

    var doubleCopyWindowSeconds: TimeInterval {
        let adaptive = CcvvCore.shared.adaptiveThresholdMs
        if adaptive > 0 {
            return TimeInterval(adaptive) / 1000.0
        }
        return CcvvCore.shared.doubleTapWindowSeconds
    }
    let postCopySettleDelaySeconds: TimeInterval = 0.15
    let precomputeClipboardWaitSeconds: TimeInterval = 0.8
    let pendingCandidateTTLSeconds: TimeInterval = 1.0

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

        // First-run onboarding (wait for Accessibility dialog to dismiss)
        if !UserDefaults.standard.bool(forKey: "ccvv_onboarding_done") {
            onboardingObserver = NotificationCenter.default.addObserver(
                forName: NSApplication.didBecomeActiveNotification,
                object: nil, queue: .main
            ) { [weak self] _ in
                guard let self = self,
                      !UserDefaults.standard.bool(forKey: "ccvv_onboarding_done") else { return }
                if let token = self.onboardingObserver {
                    NotificationCenter.default.removeObserver(token)
                    self.onboardingObserver = nil
                }
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
                    self.showOnboarding()
                }
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
            keyEquivalent: ""
        )
        undoItem.target = self
        menu.addItem(undoItem)

        menu.addItem(NSMenuItem.separator())

        pauseMenuItem = NSMenuItem(
            title: "Pause ccvv",
            action: #selector(togglePause),
            keyEquivalent: ""
        )
        pauseMenuItem.target = self
        menu.addItem(pauseMenuItem)

        menu.addItem(NSMenuItem.separator())

        let historyItem = NSMenuItem(
            title: "History...",
            action: #selector(showHistoryPopover),
            keyEquivalent: ""
        )
        historyItem.target = self
        menu.addItem(historyItem)

        let prefsItem = NSMenuItem(
            title: "Preferences...",
            action: #selector(showPreferences),
            keyEquivalent: ""
        )
        prefsItem.target = self
        menu.addItem(prefsItem)

        menu.addItem(NSMenuItem.separator())

        let quitItem = NSMenuItem(
            title: "Quit",
            action: #selector(quitApp),
            keyEquivalent: ""
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
        statusItem.menu = menu
        statusItem.button?.performClick(nil)
        statusItem.menu = nil
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
        popover.contentSize = NSSize(width: 720, height: 560)
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
            if isWritingBack { return }
            lastCmdCAt = .distantPast
            CcvvCore.shared.recordTimingSample(intervalMs: UInt32(elapsed * 1000))
            log("double Cmd+C detected (interval=\(String(format: "%.2f", elapsed))s)")

            // Take the precomputed candidate (if any) without writing yet.
            // We must wait for the system's clipboard write from this second
            // Cmd+C before writing cleaned text — the event tap is .listenOnly
            // and cannot swallow the key event, so the system copy will overwrite
            // anything we write now.
            let candidate = pendingCleanupCandidate
            if candidate != nil {
                clearPendingCleanupCandidate(reason: "consumed")
            }

            let countBefore = NSPasteboard.general.changeCount
            waitForClipboardUpdate(previousCount: countBefore, timeout: 0.5) { [weak self] updated in
                guard let self = self else { return }
                if let candidate = candidate {
                    self.applyPrecomputedCandidate(candidate)
                } else {
                    if !updated {
                        log("double Cmd+C: no new clipboard change detected, using immediate fallback clean")
                    }
                    self.performClean()
                }
            }
        } else {
            // Miss indicator: near-miss detection
            if elapsed > doubleCopyWindowSeconds && elapsed < doubleCopyWindowSeconds * 2.0 {
                DispatchQueue.main.async { [weak self] in
                    self?.showMissIndicator()
                }
            }
            lastCmdCAt = now
            beginFirstCopyPrecompute()
        }
    }

    // MARK: - Clipboard Cleaning

    func waitForClipboardUpdate(previousCount: Int, timeout: TimeInterval = 0.2,
                                  completion: @escaping (Bool) -> Void) {
        let start = Date()
        let timer = Timer.scheduledTimer(withTimeInterval: 0.01, repeats: true) { timer in
            let current = NSPasteboard.general.changeCount
            if current != previousCount && current != self.writeBackChangeCount {
                timer.invalidate()
                completion(true)
            } else if Date().timeIntervalSince(start) >= timeout {
                timer.invalidate()
                completion(false)
            }
        }
        RunLoop.current.add(timer, forMode: .common)
    }

    func beginFirstCopyPrecompute() {
        precomputeGeneration &+= 1
        let generation = precomputeGeneration
        let previousCount = NSPasteboard.general.changeCount
        waitForClipboardUpdate(previousCount: previousCount, timeout: precomputeClipboardWaitSeconds) { [weak self] updated in
            guard let self = self else { return }
            guard generation == self.precomputeGeneration else { return }
            guard updated else {
                self.clearPendingCleanupCandidate(reason: "precompute timeout")
                return
            }
            self.capturePendingCleanupCandidate()
        }
    }

    func capturePendingCleanupCandidate() {
        if let frontApp = NSWorkspace.shared.frontmostApplication,
           let bundleId = frontApp.bundleIdentifier,
           CcvvCore.shared.isAppExcluded(bundleId: bundleId) {
            clearPendingCleanupCandidate(reason: "app excluded")
            return
        }

        let pb = NSPasteboard.general
        guard let text = extractClipboardTextWithStyleHints(pb) else {
            clearPendingCleanupCandidate(reason: "no text to precompute")
            return
        }

        let originalText = pb.string(forType: .string) ?? text
        let result = CcvvCore.shared.transformWithResult(text)
        let table = CcvvCore.shared.extractTable(originalText)
        pendingCleanupCandidate = PendingCleanupCandidate(
            createdAt: Date(),
            sourceChangeCount: pb.changeCount,
            rawText: originalText,
            cleanedText: result.cleanedText,
            summary: result.summary,
            skippedSensitive: result.skippedSensitive,
            skippedOversize: result.skippedOversize,
            tableExtraction: table
        )

        pendingCandidateExpiryTimer?.invalidate()
        let timer = Timer.scheduledTimer(withTimeInterval: pendingCandidateTTLSeconds, repeats: false) { [weak self] _ in
            self?.clearPendingCleanupCandidate(reason: "candidate expired")
        }
        pendingCandidateExpiryTimer = timer
        RunLoop.current.add(timer, forMode: .common)

        log("precomputed cleanup candidate (changeCount=\(pb.changeCount), table=\(table?.detected ?? false))")
    }

    func clearPendingCleanupCandidate(reason: String) {
        if pendingCleanupCandidate != nil {
            log("cleared pending cleanup candidate: \(reason)")
        }
        pendingCleanupCandidate = nil
        pendingCandidateExpiryTimer?.invalidate()
        pendingCandidateExpiryTimer = nil
    }

    /// Apply a precomputed candidate AFTER the system's clipboard write
    /// from the second Cmd+C has completed.
    func applyPrecomputedCandidate(_ candidate: PendingCleanupCandidate) {
        if let table = candidate.tableExtraction, table.detected {
            presentTablePicker(extraction: table)
            return
        }
        if candidate.skippedSensitive {
            showFeedback(success: false, message: "Skipped: looks like a secret")
            return
        }
        if candidate.skippedOversize {
            let sizeKB = candidate.rawText.utf8.count / 1024
            showFeedback(success: false, message: "Skipped: content too large (\(sizeKB) KB)")
            return
        }
        if candidate.cleanedText == candidate.rawText {
            showFeedback(success: true, message: "No changes needed")
            return
        }

        let pb = NSPasteboard.general
        isWritingBack = true
        pb.clearContents()
        guard pb.setString(candidate.cleanedText, forType: .string) else {
            pb.clearContents()
            _ = pb.setString(candidate.rawText, forType: .string)
            isWritingBack = false
            showFeedback(success: false, message: nil)
            return
        }
        writeBackChangeCount = pb.changeCount
        isWritingBack = false

        CcvvCore.shared.recordHistory(raw: candidate.rawText, cleaned: candidate.cleanedText, storeRaw: true)
        transformCount += 1
        NotificationCenter.default.post(name: Notification.Name("ccvv.cleanSuccess"), object: nil)

        let message = candidate.summary.isEmpty ? "Cleaned" : candidate.summary
        showFeedback(success: true, message: message)
    }

    @discardableResult
    func applyPendingCandidateIfAvailable() -> Bool {
        guard let candidate = pendingCleanupCandidate else { return false }
        let age = Date().timeIntervalSince(candidate.createdAt)
        if age > pendingCandidateTTLSeconds {
            clearPendingCleanupCandidate(reason: "stale candidate")
            return false
        }
        if NSPasteboard.general.changeCount != candidate.sourceChangeCount {
            clearPendingCleanupCandidate(reason: "clipboard changed after precompute")
            return false
        }

        clearPendingCleanupCandidate(reason: "consumed")

        if let table = candidate.tableExtraction, table.detected {
            presentTablePicker(extraction: table)
            return true
        }
        if candidate.skippedSensitive {
            showFeedback(success: false, message: "Skipped: looks like a secret")
            return true
        }
        if candidate.skippedOversize {
            let sizeKB = candidate.rawText.utf8.count / 1024
            showFeedback(success: false, message: "Skipped: content too large (\(sizeKB) KB)")
            return true
        }
        if candidate.cleanedText == candidate.rawText {
            showFeedback(success: true, message: "No changes needed")
            return true
        }

        let pb = NSPasteboard.general
        isWritingBack = true
        pb.clearContents()
        guard pb.setString(candidate.cleanedText, forType: .string) else {
            pb.clearContents()
            _ = pb.setString(candidate.rawText, forType: .string)
            isWritingBack = false
            showFeedback(success: false, message: nil)
            return true
        }
        writeBackChangeCount = pb.changeCount
        isWritingBack = false

        CcvvCore.shared.recordHistory(raw: candidate.rawText, cleaned: candidate.cleanedText, storeRaw: true)
        transformCount += 1
        NotificationCenter.default.post(name: Notification.Name("ccvv.cleanSuccess"), object: nil)

        let message = candidate.summary.isEmpty ? "Cleaned" : candidate.summary
        showFeedback(success: true, message: message)
        return true
    }

    func performClean() {
        if let frontApp = NSWorkspace.shared.frontmostApplication,
           let bundleId = frontApp.bundleIdentifier,
           CcvvCore.shared.isAppExcluded(bundleId: bundleId) {
            log("  app excluded: \(bundleId)")
            return
        }

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
        let result = CcvvCore.shared.transformWithResult(text)
        let cleaned = result.cleanedText

        log("  cleaned (\(cleaned.count) chars): \(String(cleaned.prefix(500)))")

        // Check skip reasons before "no changes" comparison
        if result.skippedSensitive {
            log("  skipped: sensitive content detected")
            showFeedback(success: false, message: "Skipped: looks like a secret")
            return
        }
        if result.skippedOversize {
            let sizeKB = text.utf8.count / 1024
            log("  skipped: oversize content (\(sizeKB) KB)")
            showFeedback(success: false, message: "Skipped: content too large (\(sizeKB) KB)")
            return
        }

        if let extraction = CcvvCore.shared.extractTable(originalText), extraction.detected {
            log("  table detected (format=\(extraction.format ?? "unknown"), confidence=\(String(format: "%.2f", extraction.confidence)))")
            presentTablePicker(extraction: extraction)
            return
        }

        // Check if no changes
        if cleaned == originalText {
            log("  no changes needed")
            showFeedback(success: true, message: "No changes needed")
            return
        }

        isWritingBack = true
        pb.clearContents()
        guard pb.setString(cleaned, forType: .string) else {
            pb.clearContents()
            _ = pb.setString(originalText, forType: .string)
            isWritingBack = false
            showFeedback(success: false, message: nil)
            return
        }
        writeBackChangeCount = pb.changeCount
        isWritingBack = false

        // Record history entry
        CcvvCore.shared.recordHistory(raw: originalText, cleaned: cleaned, storeRaw: true)

        // Increment confidence counter
        transformCount += 1

        // Post success notification (for onboarding auto-dismiss)
        NotificationCenter.default.post(name: Notification.Name("ccvv.cleanSuccess"), object: nil)

        let message = result.summary.isEmpty ? "Cleaned" : result.summary
        showFeedback(success: true, message: message)
    }

    func presentTablePicker(extraction: CcvvCore.TableExtractionResult) {
        if let existing = tablePickerWindow {
            existing.close()
            tablePickerWindow = nil
        }

        let picker = TableCellPickerWindowController(
            extraction: extraction,
            onCopy: { [weak self] rawCell, row, column in
                self?.copyPickedTableCell(rawCell: rawCell, row: row, column: column)
            },
            onClose: { [weak self] in
                self?.tablePickerWindow = nil
            }
        )
        tablePickerWindow = picker
        picker.showWindow(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    func copyPickedTableCell(rawCell: String, row: Int, column: Int) {
        let result = CcvvCore.shared.transformWithResult(rawCell)
        let cleanedCell = result.cleanedText

        let pb = NSPasteboard.general
        isWritingBack = true
        pb.clearContents()
        guard pb.setString(cleanedCell, forType: .string) else {
            isWritingBack = false
            showFeedback(success: false, message: "Failed to copy selected cell")
            return
        }
        writeBackChangeCount = pb.changeCount
        isWritingBack = false

        CcvvCore.shared.recordHistory(raw: rawCell, cleaned: cleanedCell, storeRaw: true)
        transformCount += 1

        let location = "Copied cell r\(row + 1)c\(column + 1)"
        let summary = result.summary == "No changes needed" ? location : "\(location) · \(result.summary)"
        showFeedback(success: true, message: summary)
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

        // Use a closure-based action
        class DismissTarget: NSObject {
            let window: NSWindow
            init(window: NSWindow) { self.window = window }
            @objc func dismiss() {
                UserDefaults.standard.set(true, forKey: "ccvv_onboarding_done")
                window.close()
            }
            @objc func tryIt() {
                if !AXIsProcessTrusted() {
                    NSWorkspace.shared.open(
                        URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")!
                    )
                }
                UserDefaults.standard.set(true, forKey: "ccvv_onboarding_done")
                window.close()
            }
        }
        let target = DismissTarget(window: window)

        let tryButton = NSButton(title: "Try it now", target: target, action: #selector(DismissTarget.tryIt))
        tryButton.frame = NSRect(x: 190, y: 20, width: 100, height: 32)
        tryButton.bezelStyle = .rounded
        contentView.addSubview(tryButton)

        let dismissButton = NSButton(title: "Get Started", target: target, action: #selector(DismissTarget.dismiss))
        dismissButton.frame = NSRect(x: 300, y: 20, width: 100, height: 32)
        dismissButton.bezelStyle = .rounded
        dismissButton.keyEquivalent = "\r"
        contentView.addSubview(dismissButton)

        // Keep reference alive
        objc_setAssociatedObject(window, "dismissTarget", target, .OBJC_ASSOCIATION_RETAIN)

        // Store window reference for auto-dismiss
        self.onboardingWindow = window

        // Auto-dismiss on successful clean
        onboardingCleanObserver = NotificationCenter.default.addObserver(
            forName: Notification.Name("ccvv.cleanSuccess"),
            object: nil, queue: .main
        ) { [weak self] _ in
            UserDefaults.standard.set(true, forKey: "ccvv_onboarding_done")
            self?.onboardingWindow?.close()
            self?.onboardingWindow = nil
            if let token = self?.onboardingCleanObserver {
                NotificationCenter.default.removeObserver(token)
                self?.onboardingCleanObserver = nil
            }
        }

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

// MARK: - Table Cell Picker

class CellPickerTableView: NSTableView {
    var onConfirm: (() -> Void)?
    var onCancel: (() -> Void)?
    var onColumnStep: ((Int) -> Void)?

    override func keyDown(with event: NSEvent) {
        switch event.keyCode {
        case 36, 76: // Return / Enter
            onConfirm?()
        case 53: // Escape
            onCancel?()
        case 123: // Left arrow
            onColumnStep?(-1)
        case 124: // Right arrow
            onColumnStep?(1)
        default:
            super.keyDown(with: event)
        }
    }
}

class TableCellPickerWindowController: NSWindowController, NSTableViewDataSource, NSTableViewDelegate, NSWindowDelegate {
    private let extraction: CcvvCore.TableExtractionResult
    private let onCopy: (String, Int, Int) -> Void
    private let onClose: () -> Void
    private var rows: [[String]]
    private var filteredIndices: [Int]
    private var activeColumn = 0

    private var searchField: NSSearchField!
    private var tableView: CellPickerTableView!
    private var selectionLabel: NSTextField!

    init(
        extraction: CcvvCore.TableExtractionResult,
        onCopy: @escaping (String, Int, Int) -> Void,
        onClose: @escaping () -> Void
    ) {
        self.extraction = extraction
        self.onCopy = onCopy
        self.onClose = onClose
        self.rows = extraction.rows
        self.filteredIndices = Array(extraction.rows.indices)

        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 980, height: 640),
            styleMask: [.titled, .closable],
            backing: .buffered,
            defer: false
        )
        window.title = "Pick Table Cell"
        window.center()
        super.init(window: window)
        window.delegate = self
        setupUI()
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    private func setupUI() {
        guard let contentView = window?.contentView else { return }

        searchField = NSSearchField(frame: NSRect(x: 16, y: 604, width: 948, height: 26))
        searchField.placeholderString = "Search cells..."
        searchField.target = self
        searchField.action = #selector(searchChanged(_:))
        contentView.addSubview(searchField)

        let meta = NSTextField(labelWithString: "Format: \(extraction.format ?? "unknown") · Confidence: \(String(format: "%.2f", extraction.confidence))")
        meta.font = NSFont.systemFont(ofSize: 11, weight: .medium)
        meta.textColor = .secondaryLabelColor
        meta.frame = NSRect(x: 16, y: 582, width: 948, height: 16)
        contentView.addSubview(meta)

        if let warning = extraction.warnings.first, !warning.isEmpty {
            let warningLabel = NSTextField(labelWithString: warning)
            warningLabel.font = NSFont.systemFont(ofSize: 11)
            warningLabel.textColor = .systemOrange
            warningLabel.frame = NSRect(x: 16, y: 564, width: 948, height: 16)
            contentView.addSubview(warningLabel)
        }

        let scrollView = NSScrollView(frame: NSRect(x: 16, y: 56, width: 948, height: 500))
        scrollView.hasVerticalScroller = true
        scrollView.hasHorizontalScroller = true
        scrollView.autohidesScrollers = true

        tableView = CellPickerTableView(frame: scrollView.bounds)
        tableView.dataSource = self
        tableView.delegate = self
        tableView.allowsMultipleSelection = false
        tableView.allowsColumnSelection = true
        tableView.allowsColumnReordering = false
        tableView.rowHeight = 24
        tableView.target = self
        tableView.action = #selector(tableClicked)
        tableView.doubleAction = #selector(cellDoubleClicked)
        tableView.onConfirm = { [weak self] in self?.confirmSelection() }
        tableView.onCancel = { [weak self] in self?.closePicker() }
        tableView.onColumnStep = { [weak self] step in self?.stepActiveColumn(step: step) }

        let colCount = max(rows.map(\.count).max() ?? 0, 2)
        for idx in 0..<colCount {
            let col = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("col_\(idx)"))
            col.title = "Col \(idx + 1)"
            col.width = 220
            tableView.addTableColumn(col)
        }

        scrollView.documentView = tableView
        contentView.addSubview(scrollView)

        selectionLabel = NSTextField(labelWithString: "Double-click a cell to copy · Enter copies selected row at active column · Esc closes")
        selectionLabel.font = NSFont.systemFont(ofSize: 11)
        selectionLabel.textColor = .secondaryLabelColor
        selectionLabel.frame = NSRect(x: 16, y: 32, width: 700, height: 16)
        contentView.addSubview(selectionLabel)

        let doneButton = NSButton(title: "Done", target: self, action: #selector(closePicker))
        doneButton.frame = NSRect(x: 886, y: 18, width: 78, height: 28)
        doneButton.bezelStyle = .rounded
        doneButton.keyEquivalent = "\u{1b}"
        contentView.addSubview(doneButton)

        tableView.reloadData()
        if !filteredIndices.isEmpty {
            tableView.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false)
        }
        updateSelectionLabel()
    }

    @objc private func searchChanged(_ sender: NSSearchField) {
        let query = sender.stringValue.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        if query.isEmpty {
            filteredIndices = Array(rows.indices)
        } else {
            filteredIndices = rows.indices.filter { idx in
                rows[idx].contains { cell in
                    cell.lowercased().contains(query)
                }
            }
        }
        tableView.reloadData()
        if !filteredIndices.isEmpty {
            tableView.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false)
        }
        updateSelectionLabel()
    }

    @objc private func tableClicked() {
        if tableView.clickedColumn >= 0 {
            activeColumn = tableView.clickedColumn
            updateSelectionLabel()
        }
    }

    @objc private func cellDoubleClicked() {
        let displayRow = tableView.clickedRow
        let clickedColumn = tableView.clickedColumn >= 0 ? tableView.clickedColumn : activeColumn
        copyCell(displayRow: displayRow, column: clickedColumn)
    }

    @objc private func closePicker() {
        close()
    }

    private func confirmSelection() {
        let displayRow = tableView.selectedRow
        copyCell(displayRow: displayRow, column: activeColumn)
    }

    private func stepActiveColumn(step: Int) {
        let maxColumn = max(0, tableView.tableColumns.count - 1)
        activeColumn = min(maxColumn, max(0, activeColumn + step))
        updateSelectionLabel()
    }

    private func copyCell(displayRow: Int, column: Int) {
        guard displayRow >= 0, displayRow < filteredIndices.count else { return }
        let sourceRow = filteredIndices[displayRow]
        let col = min(max(0, column), max(0, tableView.tableColumns.count - 1))
        let rowCells = rows[sourceRow]
        let value = col < rowCells.count ? rowCells[col] : ""
        onCopy(value, sourceRow, col)
        close()
    }

    private func updateSelectionLabel() {
        let rowText: String
        if tableView.selectedRow >= 0, tableView.selectedRow < filteredIndices.count {
            rowText = "row \(filteredIndices[tableView.selectedRow] + 1)"
        } else {
            rowText = "no row selected"
        }
        selectionLabel.stringValue = "Active column: \(activeColumn + 1), \(rowText)"
    }

    func numberOfRows(in tableView: NSTableView) -> Int {
        filteredIndices.count
    }

    func tableViewSelectionDidChange(_ notification: Notification) {
        updateSelectionLabel()
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard row >= 0, row < filteredIndices.count else { return nil }
        guard let tableColumn = tableColumn else { return nil }

        let sourceRow = filteredIndices[row]
        let colIdx = tableView.tableColumns.firstIndex(of: tableColumn) ?? 0
        let rowCells = rows[sourceRow]
        let text = colIdx < rowCells.count ? rowCells[colIdx] : ""

        let cell = NSTextField(labelWithString: text)
        cell.font = NSFont.systemFont(ofSize: 11)
        cell.lineBreakMode = .byTruncatingTail
        cell.toolTip = text
        return cell
    }

    func windowWillClose(_ notification: Notification) {
        onClose()
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

        let documentView = NSView(frame: NSRect(x: 0, y: 0, width: 420, height: 520))
        scrollView.documentView = documentView
        contentView.addSubview(scrollView)

        var yOffset: CGFloat = 492

        // Section: Text Cleanup
        yOffset = addSectionHeader("Text Cleanup", to: documentView, y: yOffset)
        yOffset = addToggle("Clean whitespace and line breaks", feature: "whitespace_cleanup", to: documentView, y: yOffset)
        yOffset = addToggle("Normalize encoding (quotes, dashes, invisible chars)", feature: "normalize_unicode", to: documentView, y: yOffset)
        yOffset = addToggle("Remove hidden agent artifacts", feature: "agent_strip", to: documentView, y: yOffset)
        yOffset -= 10

        // Section: Formatting
        yOffset = addSectionHeader("Formatting", to: documentView, y: yOffset)
        yOffset = addToggle("Detect and structure JSON/table/code content", feature: "structural_detection", to: documentView, y: yOffset)
        yOffset = addToggle("Open cell picker when table content is detected", feature: "table_cell_picker", to: documentView, y: yOffset)
        yOffset = addToggle("Auto-wrap code-like tokens in backticks (shell risk)", feature: "auto_wrapper", to: documentView, y: yOffset)
        yOffset = addToggle("Apply custom cleanup rules from config", feature: "user_rules", to: documentView, y: yOffset)
        yOffset -= 10

        // Section: URLs
        yOffset = addSectionHeader("URLs", to: documentView, y: yOffset)
        yOffset = addToggle("Remove tracking parameters from URLs", feature: "url_cleaning", to: documentView, y: yOffset)
        yOffset -= 10

        // Section: Privacy
        yOffset = addSectionHeader("Privacy", to: documentView, y: yOffset)
        yOffset = addToggle("Skip cleanup when sensitive content is detected", feature: "sensitive_filter", to: documentView, y: yOffset)
        yOffset -= 10

        // Section: Feedback
        yOffset = addSectionHeader("Feedback", to: documentView, y: yOffset)
        yOffset = addToastPrefControl(to: documentView, y: yOffset)
        let applyHint = NSTextField(labelWithString: "Changes apply immediately")
        applyHint.font = NSFont.systemFont(ofSize: 10)
        applyHint.textColor = .tertiaryLabelColor
        applyHint.frame = NSRect(x: 30, y: 52, width: 220, height: 16)
        documentView.addSubview(applyHint)

        // Buttons
        let buttonRowY: CGFloat = 12
        let openConfigButton = NSButton(title: "Open Config File", target: self, action: #selector(openConfigFile))
        openConfigButton.frame = NSRect(x: 20, y: buttonRowY, width: 150, height: 32)
        openConfigButton.bezelStyle = .rounded
        documentView.addSubview(openConfigButton)

        let resetButton = NSButton(title: "Reset to Defaults", target: self, action: #selector(resetToDefaults))
        resetButton.frame = NSRect(x: 180, y: buttonRowY, width: 150, height: 32)
        resetButton.bezelStyle = .rounded
        documentView.addSubview(resetButton)

        let doneButton = NSButton(title: "Done", target: self, action: #selector(closePreferences))
        doneButton.frame = NSRect(x: 340, y: buttonRowY, width: 70, height: 32)
        doneButton.bezelStyle = .rounded
        doneButton.keyEquivalent = "\r"
        documentView.addSubview(doneButton)
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
            if CcvvCore.shared.reloadConfig() {
                log("Preferences: reloaded active config")
            }
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
            # table_cell_picker = true
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
        _ = CcvvCore.shared.reloadConfig()
        // Refresh the window
        if let window = self.window {
            window.contentView?.subviews.forEach { $0.removeFromSuperview() }
            setupUI()
        }
    }

    @objc func closePreferences() {
        window?.close()
    }
}

// MARK: - History View Controller

class HistoryViewController: NSViewController, NSMenuItemValidation {
    var tableView: NSTableView!
    var searchField: NSSearchField!
    var entries: [(preview: String, cleanedText: String, rawText: String?, contentType: String, timestamp: String)] = []

    override func loadView() {
        let containerWidth: CGFloat = 720
        let containerHeight: CGFloat = 560
        let bottomBarHeight: CGFloat = 36
        let searchHeight: CGFloat = 28

        let container = NSView(frame: NSRect(x: 0, y: 0, width: containerWidth, height: containerHeight))
        self.view = container

        // Search field
        searchField = NSSearchField(frame: NSRect(x: 10, y: containerHeight - 35, width: containerWidth - 20, height: searchHeight))
        searchField.placeholderString = "Search history..."
        searchField.target = self
        searchField.action = #selector(searchChanged(_:))
        container.addSubview(searchField)

        // Hint label
        let hintLabel = NSTextField(labelWithString: "Double-click Org icon to copy original · Double-click Cleaned Up to copy cleaned")
        hintLabel.font = NSFont.systemFont(ofSize: 10)
        hintLabel.textColor = .tertiaryLabelColor
        hintLabel.frame = NSRect(x: 10, y: 11, width: containerWidth - 110, height: 16)
        container.addSubview(hintLabel)

        let doneButton = NSButton(title: "Done", target: self, action: #selector(closeHistoryPopover))
        doneButton.frame = NSRect(x: containerWidth - 82, y: 7, width: 72, height: 24)
        doneButton.bezelStyle = .rounded
        doneButton.keyEquivalent = "\r"
        container.addSubview(doneButton)

        // Scroll view with table
        let scrollView = NSScrollView(
            frame: NSRect(
                x: 0,
                y: bottomBarHeight,
                width: containerWidth,
                height: containerHeight - searchHeight - bottomBarHeight - 10
            )
        )
        scrollView.hasVerticalScroller = true
        scrollView.autoresizingMask = [.width, .height]

        tableView = NSTableView()
        tableView.delegate = self
        tableView.dataSource = self

        let rawCol = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("raw"))
        rawCol.title = "Org"
        rawCol.width = 26
        tableView.addTableColumn(rawCol)

        let previewCol = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("preview"))
        previewCol.title = "Cleaned Up"
        previewCol.width = 468
        tableView.addTableColumn(previewCol)

        let typeCol = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("type"))
        typeCol.title = "Type"
        typeCol.width = 70
        tableView.addTableColumn(typeCol)

        let timeCol = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("time"))
        timeCol.title = "Time"
        timeCol.width = 128
        tableView.addTableColumn(timeCol)

        tableView.rowHeight = 24

        tableView.target = self
        tableView.doubleAction = #selector(rowDoubleClicked)

        // Right-click context menu for copy options
        let contextMenu = NSMenu()
        contextMenu.addItem(NSMenuItem(title: "Copy Cleaned", action: #selector(contextCopyCleaned(_:)), keyEquivalent: ""))
        contextMenu.addItem(NSMenuItem(title: "Copy Original", action: #selector(contextCopyRaw(_:)), keyEquivalent: ""))
        tableView.menu = contextMenu

        scrollView.documentView = tableView
        container.addSubview(scrollView)

        loadHistory(query: nil)
    }

    @objc func closeHistoryPopover() {
        if let popover = (NSApp.delegate as? AppDelegate)?.historyPopover {
            popover.close()
        }
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
                let cleaned = (item["cleaned_text"] as? String) ?? preview
                let raw = item["raw_text"] as? String
                let ct = (item["content_type"] as? String) ?? "unknown"
                let ts: String
                if let epoch = item["created_at"] as? Int {
                    ts = relativeTimeFromDate(Date(timeIntervalSince1970: TimeInterval(epoch)))
                } else if let epochDouble = item["created_at"] as? Double {
                    ts = relativeTimeFromDate(Date(timeIntervalSince1970: epochDouble))
                } else {
                    ts = ""
                }
                entries.append((preview: String(preview.prefix(80)), cleanedText: cleaned, rawText: raw, contentType: ct, timestamp: ts))
            }
        }

        tableView?.reloadData()
    }

    @objc func searchChanged(_ sender: NSSearchField) {
        loadHistory(query: sender.stringValue)
    }

    @objc func rowDoubleClicked() {
        let row = tableView.clickedRow >= 0 ? tableView.clickedRow : tableView.selectedRow
        guard row >= 0, row < entries.count else { return }

        let clickedCol = tableView.clickedColumn
        if clickedCol >= 0, clickedCol < tableView.tableColumns.count {
            let columnId = tableView.tableColumns[clickedCol].identifier.rawValue
            if columnId == "raw" {
                copyRawEntry(row, source: "double click")
            } else {
                copyCleanedEntry(row, source: "double click")
            }
        } else {
            copyCleanedEntry(row, source: "double click")
        }

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

    func badgeLabel(for contentType: String) -> String {
        switch contentType.lowercased() {
        case "prose": return "TEXT"
        case "mixed": return "TEXT"
        default: return contentType.uppercased()
        }
    }

    func badgeColor(for contentType: String) -> NSColor {
        switch contentType.lowercased() {
        case "url": return .systemBlue
        case "code": return .systemGreen
        case "json": return .systemOrange
        case "table": return .systemPurple
        case "prose", "mixed": return .systemGray
        default: return .systemGray
        }
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard row < entries.count else { return nil }
        let entry = entries[row]

        switch tableColumn?.identifier.rawValue {
        case "type":
            let wrapper = NSView(frame: NSRect(x: 0, y: 0, width: 50, height: 18))
            wrapper.wantsLayer = true
            wrapper.layer?.cornerRadius = 4
            wrapper.layer?.masksToBounds = true
            wrapper.layer?.backgroundColor = badgeColor(for: entry.contentType).cgColor

            let label = NSTextField(labelWithString: badgeLabel(for: entry.contentType))
            label.font = NSFont.systemFont(ofSize: 8, weight: .bold)
            label.textColor = .white
            label.alignment = .center
            label.isBordered = false
            label.drawsBackground = false
            label.frame = NSRect(x: 0, y: 1, width: 50, height: 14)
            wrapper.addSubview(label)
            return wrapper
        case "raw":
            let wrapper = NSView(frame: NSRect(x: 0, y: 0, width: 24, height: 20))
            wrapper.toolTip = entry.rawText != nil ? "copy original version" : "Original text not available"
            let imageView = NSImageView(frame: NSRect(x: 4, y: 2, width: 16, height: 16))
            imageView.imageScaling = .scaleProportionallyUpOrDown
            imageView.contentTintColor = entry.rawText != nil ? .secondaryLabelColor : .quaternaryLabelColor
            imageView.image = NSImage(systemSymbolName: "doc.on.doc", accessibilityDescription: "Original")
            imageView.toolTip = wrapper.toolTip
            wrapper.addSubview(imageView)
            return wrapper
        default:
            let cell = NSTextField(labelWithString: "")
            cell.lineBreakMode = .byTruncatingTail
            cell.font = NSFont.systemFont(ofSize: 11)
            switch tableColumn?.identifier.rawValue {
            case "preview":
                cell.stringValue = entry.preview
                cell.toolTip = entry.cleanedText
            case "time": cell.stringValue = entry.timestamp
            default: break
            }
            return cell
        }
    }

    func copyCleanedEntry(_ row: Int, source: String) {
        guard row >= 0, row < entries.count else { return }
        let pb = NSPasteboard.general
        pb.clearContents()
        pb.setString(entries[row].cleanedText, forType: .string)
        log("History: copied cleaned text via \(source)")
    }

    func copyRawEntry(_ row: Int, source: String) {
        guard row >= 0, row < entries.count else { return }
        guard let raw = entries[row].rawText else {
            log("History: no raw text available for row \(row)")
            return
        }
        let pb = NSPasteboard.general
        pb.clearContents()
        pb.setString(raw, forType: .string)
        log("History: copied original text via \(source)")
    }

    func validateMenuItem(_ menuItem: NSMenuItem) -> Bool {
        if menuItem.action == #selector(contextCopyRaw(_:)) {
            let row = tableView.clickedRow
            guard row >= 0, row < entries.count else { return false }
            return entries[row].rawText != nil
        }
        if menuItem.action == #selector(contextCopyCleaned(_:)) {
            let row = tableView.clickedRow
            return row >= 0 && row < entries.count
        }
        return true
    }

    @objc func contextCopyCleaned(_ sender: NSMenuItem) {
        let row = tableView.clickedRow
        copyCleanedEntry(row, source: "context menu")
        if let popover = (NSApp.delegate as? AppDelegate)?.historyPopover {
            popover.close()
        }
    }

    @objc func contextCopyRaw(_ sender: NSMenuItem) {
        let row = tableView.clickedRow
        copyRawEntry(row, source: "context menu")
        if let popover = (NSApp.delegate as? AppDelegate)?.historyPopover {
            popover.close()
        }
    }
}

// MARK: - Entry Point

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.run()
