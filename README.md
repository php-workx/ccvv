# [ ccvv ]_

**Double the tap, half the mess.**

You copy text from a terminal, a PDF, or an AI agent. You paste it. It’s a disaster of ANSI escape codes, hard line breaks, phantom spaces, and tracking URLs.

`ccvv` is a lightweight, cross-platform background utility that intelligently sanitizes and formats your clipboard exactly when you need it—just by double-tapping your standard copy or paste shortcuts.

---

## ⚡ The Core Workflow

It lives in your system tray and stays out of your way until you summon it.

* **Smart Copy (`Cmd/Ctrl + C, C`):** Intercepts your clipboard, strips the garbage, applies smart formatting (like Markdown wrapping), and saves the clean `text/plain` result back to your clipboard.

---

## 🧠 The "Smarts" (Enabled by Default)

`ccvv` isn't just a plain-text stripper. It understands context:

* **Agent & CLI Cleaner:** Strips ANSI terminal color codes, removes hidden zero-width characters, and instantly deletes artifact markers (like Claude Code's `⏺`).
* **Whitespace & Line Break Manager:** Fixes copied PDF and email text. Strips leading/trailing whitespace, unwraps hard line breaks into flowing paragraphs, but **preserves** intentional structure (double newlines, lists, and fenced code blocks).
* **Platform Rich-Text Inference:** On macOS, `ccvv` infers inline code from rich-text monospace fonts/colors and converts them to standard Markdown backticks (` ` `) before stripping the rich payload.

---

## 🚀 Installation

`ccvv` is a single, compiled binary with virtually zero memory overhead.

**macOS**
```bash
brew install ccvv
```

---

## 🤝 Contributing

**License:** MIT