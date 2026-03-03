# [ ccvv ]_

**Double the tap, half the mess.**

[![macOS](https://img.shields.io/badge/macOS-brew_install_ccvv-black?logo=apple)](#)

You copy text from a terminal, a PDF, or an AI agent. You paste it. It’s a disaster of ANSI escape codes, hard line breaks, phantom spaces, and tracking URLs.

`ccvv` is a lightweight utility that intelligently sanitizes and formats your clipboard exactly when you need it—just by double-tapping your standard copy or paste shortcuts.

---

## ⚡ The Core Workflow

It lives in your system tray and stays out of your way until you summon it.

* **Smart Copy (`Cmd/Ctrl + C, C`):** Intercepts your clipboard, strips the garbage, applies smart formatting (like Markdown wrapping), and saves the clean `text/plain` result back to your clipboard.
* **Smart Paste (`Cmd/Ctrl + V, V`):** Detects your active window and formats the text on the fly. (e.g., Flattens multi-line JSON into a single line if you are pasting into a terminal).

**The "Blink" Feedback:** No annoying notifications. When you double-tap, the `[cc]` system tray icon flashes to `[██]` for 300ms. A satisfying, tactile visual confirmation that the text was sanitized.

---

## 🧠 The "Smarts" (Enabled by Default)

`ccvv` isn't just a plain-text stripper. It understands context:

* **The "Blind" Paste:** Actively nukes rich-text MIME types (HTML, CSS, RTF). Guarantees a clean paste even in apps that don't support `Ctrl+Shift+V`.
* **Agent & CLI Cleaner:** Strips ANSI terminal color codes, removes hidden zero-width characters, and instantly deletes artifact markers (like Claude Code's `⏺`).
* **Whitespace & Line Break Manager:** Fixes copied PDF and email text. Strips leading/trailing whitespace, unwraps hard line breaks into flowing paragraphs, but **preserves** intentional structure (double newlines, lists, and fenced code blocks).
* **Smart URL Stripper:** Copies `https://www.github.com/repo?utm_source=slack` and transforms it into `github.com/repo`.
* **Platform Rich-Text Inference:** On macOS, `ccvv` infers inline code from rich-text monospace fonts/colors and converts them to standard Markdown backticks (` ` `) before stripping the rich payload.
* **JSON Flattener:** Prettifies messy JSON on copy, but dynamically minifies it to a single line if you double-tap paste into a CLI tool.

---

## 🚀 Installation

`ccvv` is a single, compiled binary with virtually zero memory overhead.

**macOS**
```bash
brew install ccvv
```

---

## ⚙️ Configuration

Don't want URL stripping? Want to hide the tray icon entirely? `ccvv` is fully customizable.

Click the `[cc]` icon in your tray to:
1.  **Pause:** Temporarily bypass the listener when you actually *want* rich formatting.
2.  **History:** Access a local-only, privacy-first rolling database of your last 50 raw clipboard items (just in case the Smart Copy was a little *too* smart).
3.  **Preferences:** Toggle individual "Smarts" on or off.

---

## 🤝 Contributing

**License:** MIT