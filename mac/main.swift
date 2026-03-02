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
    var lastCmdCAt: Date = .distantPast
    var menu: NSMenu!
    let doubleCopyWindowSeconds: TimeInterval = 0.45
    let postCopySettleDelaySeconds: TimeInterval = 0.15

    func applicationDidFinishLaunching(_: Notification) {
        setupStatusItem()
        registerEventTap()
    }

    func setupStatusItem() {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        if let button = statusItem.button {
            button.title = "[cc]"
            button.font = NSFont.monospacedSystemFont(ofSize: 12, weight: .medium)
            button.action = #selector(statusItemClicked(_:))
            button.target = self
            button.sendAction(on: [.leftMouseUp, .rightMouseUp])
        }

        menu = NSMenu()

        let versionItem = NSMenuItem(title: "ccvv v\(appVersion)", action: nil, keyEquivalent: "")
        versionItem.isEnabled = false
        menu.addItem(versionItem)
        menu.addItem(NSMenuItem.separator())

        let quitItem = NSMenuItem(
            title: "Quit",
            action: #selector(quitApp),
            keyEquivalent: "q"
        )
        quitItem.target = self
        menu.addItem(quitItem)
    }

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

    func registerEventTap() {
        let eventMask: CGEventMask = 1 << CGEventType.keyDown.rawValue

        guard let tap = CGEvent.tapCreate(
            tap: .cgSessionEventTap,
            place: .headInsertEventTap,
            options: .listenOnly,
            eventsOfInterest: eventMask,
            callback: { _, _, event, refcon -> Unmanaged<CGEvent>? in
                let delegate = Unmanaged<AppDelegate>.fromOpaque(refcon!).takeUnretainedValue()
                delegate.handleCGKeyEvent(event)
                return Unmanaged.passRetained(event)
            },
            userInfo: Unmanaged.passUnretained(self).toOpaque()
        ) else {
            log("Failed to create event tap — Accessibility permission missing?")
            return
        }

        eventTap = tap
        let source = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0)
        CFRunLoopAddSource(CFRunLoopGetCurrent(), source, .commonModes)
        CGEvent.tapEnable(tap: tap, enable: true)
        log("CGEventTap registered")
    }

    func handleCGKeyEvent(_ event: CGEvent) {
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
            log("double Cmd+C detected (interval=\(String(format: "%.2f", elapsed))s)")
            DispatchQueue.main.asyncAfter(deadline: .now() + postCopySettleDelaySeconds) { [weak self] in
                self?.performClean()
            }
        } else {
            lastCmdCAt = now
        }
    }

    func performClean() {
        let pb = NSPasteboard.general

        log("performClean triggered")
        log("  clipboard types: \(pb.types?.map(\.rawValue) ?? [])")

        guard let text = extractClipboardTextWithStyleHints(pb) else {
            log("  no text extracted from clipboard")
            showFeedback(success: false)
            return
        }

        let rawClip = pb.string(forType: .string) ?? ""
        log("  raw clipboard lines:")
        for (i, line) in rawClip.components(separatedBy: "\n").prefix(30).enumerated() {
            log("    [\(i)] (\(line.count)ch): \(String(line.prefix(120)))")
        }
        log("  extracted (\(text.count) chars): \(String(text.prefix(500)))")

        let originalText = pb.string(forType: .string) ?? text
        let cleaned = ccvv(text)

        log("  cleaned (\(cleaned.count) chars): \(String(cleaned.prefix(500)))")

        pb.clearContents()
        guard pb.setString(cleaned, forType: .string) else {
            pb.clearContents()
            _ = pb.setString(originalText, forType: .string)
            showFeedback(success: false)
            return
        }
        showFeedback(success: true)
    }

    func applicationWillTerminate(_: Notification) {
        if let eventTap {
            CGEvent.tapEnable(tap: eventTap, enable: false)
        }
    }

    func showFeedback(success: Bool) {
        guard success, let button = statusItem.button else { return }
        let originalTitle = button.title
        button.title = " ✓ "
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) {
            button.title = originalTitle
        }
    }

    @objc func quitApp() {
        NSApplication.shared.terminate(nil)
    }
}

// MARK: - Entry Point

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.run()
