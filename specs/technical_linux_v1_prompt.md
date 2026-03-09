# Prompt for Linux Technical Spec

You are writing the technical specification for Linux support in ccvv, a clipboard sanitizer that cleans text (strips formatting, `ANSI` codes, tracking `URLs`, normalizes whitespace) when users trigger it. It currently ships as a `macOS` menu bar app (Swift) with a Rust core library (`core/ccvv-lib`) that handles all text transformation, config parsing (`TOML`), and history (`SQLite`). There is also a Rust `CLI` (`core/ccvv-cli`) that links the same library directly.

Target distros: Ubuntu (22.04+), Debian (12+), Fedora (39+), Arch Linux.
Target display servers: `X11` and Wayland (both must be first-class).

## What exists today

- Rust core library (ccvv-lib): text transform pipeline (8 stages), `TOML` config, `SQLite` history, C `FFI` surface. Already built and tested. This is the engine -- the Linux shell calls into it.
- Rust `CLI` (ccvv-cli): stdin/stdout pipe transforms, history access, diagnostics. Already built. Works anywhere Rust compiles.
- `macOS` app (Swift/AppKit): `CGEventTap` for Cmd+C double-tap detection, `NSStatusItem` menu bar icon, `NSPasteboard` clipboard access, `NSAttributedString` rich text extraction. This is macOS-only and will `NOT` be ported.
- Config: `~/.ccvv/config.toml`. Schema defined in `specs/functional_v1.md` §6.
- Security invariant: No network access, ever. No fancy-regex. Clipboard content is untrusted input.

## What the spec must cover

Write a technical specification (same depth and structure as an `RFC`) covering:

1. Architecture: How the Linux daemon/tray app is structured. It should reuse ccvv-lib (link directly as a Rust binary, no `FFI` needed since we're staying in Rust). Define the crate structure -- is it a new binary crate in the workspace (`core/ccvv-linux`), or does the `CLI` grow a daemon mode, or something else? Justify the choice.
2. Clipboard integration:
   - `X11`: `XFixes` selection-change events for clipboard monitoring. Detail which selections to monitor (`CLIPBOARD` vs `PRIMARY`), how to detect double-copy (same content hash within 300ms window), and how to write back cleaned text.
   - Wayland: Wayland's security model prevents global clipboard snooping. Specify exactly what `IS` possible (`e.g`., wl-clipboard integration, compositor-specific protocols like wlr-data-control-unstable-v1), what requires compositor support, and what falls back to `manual/CLI-only` mode. Be precise about which compositors support what (GNOME/Mutter, KDE/KWin, Sway/wlroots, Hyprland).
   - Handle the X11-on-Wayland case (`XWayland`).
3. Double-tap detection:
   - On `X11`: How to detect rapid repeated Ctrl+C. Options include `XFixes` clipboard-change events (polling changecount like `macOS`), `XRecord` extension for keystroke monitoring, or `XGrab`. Evaluate tradeoffs (permissions, reliability, conflicts).
   - On Wayland: Whether keyboard monitoring is possible at all (spoiler: generally not without compositor cooperation). Specify the fallback strategy (clipboard-change polling where available, manual trigger, `CLI`).
   - Adaptive timing (same 150-500ms window, same algorithm as `macOS`).
4. System tray / status icon:
   - `StatusNotifierItem` (`SNI`) via D-Bus -- the modern standard (`KDE`, most `DEs`).
   - `XEmbed` fallback for older environments (`e.g`., some XFCE/i3/polybar setups).
   - Specify icon format, states (active/paused/success flash/no permission), and menu structure.
   - Evaluate whether to use libappindicator3, ksni crate, or raw D-Bus. Consider: no GTK/Qt runtime dependency for the core daemon.
5. Autostart & lifecycle:
   - `XDG` autostart (.desktop file in `~/.config/autostart/`).
   - Systemd user service as an alternative.
   - Single-instance enforcement (how: `PID` file, D-Bus name, abstract Unix socket?).
   - Graceful degradation: what happens if no display server, no tray, headless `SSH` session.
6. Packaging & distribution:
   - .deb (Ubuntu/Debian), .rpm (Fedora), `AUR` `PKGBUILD` (Arch), and optionally `AppImage` or Flatpak.
   - Evaluate Snap vs Flatpak vs neither (consider sandbox restrictions on clipboard access -- this is critical for ccvv).
   - Homebrew Linux (brew install ccvv on Linuxbrew) -- we already have a Cask for `macOS`.
   - Binary tarball as fallback.
   - `CI`: `GitHub` Actions matrix for building across distros.
7. Dependencies & build:
   - List exact system library dependencies (`e.g`., libxcb, libxfixes-dev, libdbus-1-dev, wayland-client).
   - Feature flags: x11, wayland, tray -- so users can compile without `X11` or without tray support.
   - Static vs dynamic linking strategy.
   - Cross-compilation considerations (build on Ubuntu, target all distros).
8. Permissions & security:
   - What permissions are needed on each display server.
   - Wayland's security model implications (no global keyboard grab, limited clipboard access).
   - Flatpak/Snap sandbox portals (`org.freedesktop.portal.Clipboard` if it exists, or why it doesn't).
   - File permissions for config and history `DB` (same owner-only model as `macOS`: 0600/0700).
9. Rich text extraction:
   - On `macOS`, Stage 1 uses `NSAttributedString` to infer inline code from monospace fonts.
   - What's the Linux equivalent? Options: parse `HTML` from text/html clipboard `MIME` type, use xclip -o -t text/html, or skip rich text inference on Linux entirely. Evaluate.
10. Testing strategy:
   - How to test clipboard integration without a live display server (`Xvfb`, wlheadless, mocking).
   - Integration test matrix: `X11`, Wayland (Sway), `XWayland`.
   - `CI` strategy for distro-specific testing (Docker containers).

## Constraints

- Reuse ccvv-lib for `ALL` text transformation. Do not reimplement any cleaning logic. The Linux shell is a thin platform layer over the shared Rust library.
- No network access. Same `deny.toml` ban list applies.
- No `GTK` or Qt runtime dependency for the core daemon. A tray icon integration may optionally use libappindicator, but the app must function (`CLI` + clipboard monitoring) without any `GUI` toolkit.
- The `CLI` already works on Linux. This spec is about the daemon/tray component and the packaging -- not reimplementing the `CLI`.
- Keep the spec grounded in what actually works today on these distros. Don't spec features that require kernel patches or compositor changes that don't exist yet. Flag anything that's aspirational vs shipping.

## Deliverable

A single Markdown document (`specs/technical_linux_v1.md`) structured with numbered sections, decision tables, architecture diagrams (`ASCII`), and explicit "Decision" callouts where tradeoffs were made. Same tone and rigor as the existing `specs/technical_v1.md`.
