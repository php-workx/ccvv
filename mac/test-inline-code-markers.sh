#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
SOURCE="$ROOT_DIR/mac/main.swift"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

TEST_FILE="$TMP_DIR/inline-code-marker-tests.swift"

{
    printf 'import Foundation\n\n'
    awk '/^func isCodeFenceLine/ { emit = 1 } /^func extractClipboardTextWithStyleHints/ { emit = 0 } emit { print }' "$SOURCE"
    awk '/^func addInlineCodeMarkersToPlainText/ { emit = 1 } /^func isMonospacedFont/ { emit = 0 } emit { print }' "$SOURCE"
    awk '/^func wrapInBackticks/ { emit = 1 } /^\/\/ MARK: - CcvvCore/ { emit = 0 } emit { print }' "$SOURCE"
    cat <<'SWIFT'

func assertEqual(_ actual: String, _ expected: String, _ label: String) {
    if actual != expected {
        fputs("FAIL: \(label)\nexpected: \(expected)\nactual:   \(actual)\n", stderr)
        exit(1)
    }
}

let proseTitle = "Improve widget recovery flows with browser-stable session IDs, pending-session UI, cancel-and-retry behavior, heartbeat polling, and cooldown protection"

assertEqual(
    addInlineCodeMarkersToPlainText("Use session IDs and API responses, not UI copy."),
    "Use session IDs and API responses, not UI copy.",
    "plain technical nouns and acronyms are not inline code"
)

assertEqual(
    wrapInBackticks(proseTitle),
    proseTitle,
    "styled prose runs are not wrapped as one inline code span"
)

assertEqual(
    wrapInBackticks("git status"),
    "`git status`",
    "styled command spans with spaces remain inline code"
)

assertEqual(
    addInlineCodeMarkersToPlainText("Pass sessionId, session_id, --retry, and config.yaml."),
    "Pass `sessionId`, `session_id`, `--retry`, and `config.yaml`.",
    "machine-facing tokens still get inline code markers"
)
SWIFT
} > "$TEST_FILE"

swift "$TEST_FILE"
