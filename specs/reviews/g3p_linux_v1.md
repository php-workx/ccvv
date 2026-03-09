Findings

- Severity: High
- Location: Section `6.3.4` (GNOME/Mutter Explicit Hotkey Mode)
- Issue: The definition of "double-copy gesture" via a portal-managed hotkey is incoherent and UX-hostile.
- Why it matters: The spec states "A single activation triggers Clean Clipboard Now" but "two activations... trigger the `GNOME` variant of the double-copy gesture". If the hotkey (`e.g`., Super+Alt+C) is explicitly bound to "Clean", a single press should perform the action immediately. Requiring or detecting a "double press" on a dedicated action hotkey introduces unnecessary latency (waiting for the second press) or confusion (doing the same thing twice). The "double-tap" concept applies to overloading an existing key (Ctrl+C), not invoking a dedicated command.
- What is missing or wrong: The "double-tap" branding is being forced onto an explicit trigger mechanism where it doesn't belong.
- Suggested correction: Remove the "double activations" logic for the portal hotkey entirely. State that the portal hotkey triggers Clean Clipboard Now immediately on a single press.

- Severity: High
- Location: Section `6.2.5` (Writing Back on `X11`) and `6.1` (Acquisition Contract)
- Issue: "Blind Paste" behavior on Linux implicitly destroys non-text clipboard content without explicit warning or handling.
- Why it matters: On X11/Wayland, taking ownership of the selection replaces all previous targets. If a user copies mixed content (`e.g`., a selection in a browser containing text and an image) and ccvv cleans it, ccvv takes ownership and offers only text. The image data is strictly lost. While this matches the "clean to plain text" product goal, the spec does not explicitly address the loss of mixed-content fidelity or whether ccvv should attempt to preserve/proxy non-text targets.
- What is missing or wrong: The spec acknowledges "No text target exists, skip" but fails to specify behavior for "Text target exists `AND` other targets exist".
- Suggested correction: Explicitly specify that ccvv drops all non-text targets when taking ownership. Add a "Known Limitation" that mixed-media copies become text-only.

- Severity: Medium
- Location: Section `6.5` (Rich Text Extraction) vs Section 1 (Architecture)
- Issue: html5ever dependency in the platform layer contradicts the "No Linux-specific transform logic" goal.
- Why it matters: Section 1 claims "No Linux-specific text cleaning logic is reimplemented outside the shared library." However, Section `6.5` introduces html5ever to parse `HTML` and extract text. This is technically "extraction" (Stage 1), mirroring `macOS` `NSAttributedString`, but it represents a significant logic fork. ccvv-linux will interpret `HTML` lists/breaks/entities differently than `macOS`, potentially leading to platform-divergent output for the same web content.
- What is missing or wrong: The architectural claim in Section 1 is too absolute ("No... logic reimplemented").
- Suggested correction: Amend Section 1 to clarify that rich-text extraction (Stage 1) is platform-specific, while text transformation (Stages 2-8) is shared. Acknowledge that `HTML` parsing behavior may diverge slightly from `macOS` `AppKit` behavior.

- Severity: Medium
- Location: Section `6.2.3` (`X11` Double-Copy Detection)
- Issue: `XFixes` detection logic does not account for `CLIPBOARD` vs `PRIMARY` selection usage patterns if users enable `PRIMARY` support later.
- Why it matters: The spec disables `PRIMARY` monitoring by default (correctly), but Section `6.2.3` describes the algorithm generically. If `PRIMARY` is enabled, the "double-copy" heuristic (same hash within window) is extremely dangerous on `PRIMARY` because selecting text (one event) and then adjusting the selection (second event) happens constantly and rapidly, often with the same or substring content.
- What is missing or wrong: The algorithm does not explicitly forbid "double-copy" triggers on the `PRIMARY` channel even if monitoring is enabled.
- Suggested correction: Explicitly state that "Automatic double-copy detection `MUST` be disabled for the `PRIMARY` channel even if monitoring is enabled." `PRIMARY` should likely only be monitored for history, never for auto-sanitization.

- Severity: Low
- Location: Section `9.2` (systemd User Service)
- Issue: The recommendation "not enabled by packages by default" is vague regarding the conflict with `XDG` Autostart.
- Why it matters: If a package installs both an `XDG` autostart file and a systemd user unit, and the user enables the unit, ccvv might race on startup.
- What is missing or wrong: The spec relies on "singleton enforcement" to resolve the race, which is safe but sloppy.
- Suggested correction: Packages should likely not ship the systemd unit to enabled targets, or the unit should `ConditionEnvironment` to avoid starting if the `XDG` path is preferred. The current "singleton lock" mitigation is acceptable but explicit precedence is better.

Open Questions

1. Wayland "Daemon's Own" Writes: Section `6.3.3` says "Writes from the daemon's own active data source are ignored once." In the wlr-data-control and ext-data-control protocols, how specifically does the daemon identify its own offer? Does it rely on seat + serial correlation, or content hashing? This mechanism needs to be robust against race conditions where an external copy happens immediately after the daemon write.
2. `HTML` Parsing Limits: html5ever is a full `HTML` parser. Does the spec enforce a size limit on the `HTML` blob before parsing? Section `5.3` of the core spec limits input to 1MB, but is that enforced on the raw `HTML` or the extracted text? Parsing 100MB of `HTML` to find 1KB of text is a `DoS` vector.

Overall Assessment

1. Is this spec implementable as written? Yes, largely. The Linux-specific constraints (X11/Wayland protocols, simple tray) are well-scoped.
2. Would you approve it for implementation? Conditional Approval. The `GNOME` "double activation" logic must be fixed (simplified), and the "Blind Paste" mixed-content data loss must be explicitly accepted/documented.
3. Top 3 Risks:
    * `GNOME` `UX` Friction: The lack of native background monitoring on `GNOME` (the most popular Linux desktop) significantly degrades the "magic" value proposition. Users may find the manual hotkey workflow annoying compared to the "it just works" `macOS` experience.
    * Wayland Protocol Fragmentation: Relying on ext-data-control-v1 is the correct path, but real-world compositor support (especially older wlroots versions or NVIDIA-specific quirks) is a minefield. The "Shipping vs Deferred" matrix is optimistic.
    * X11/Wayland Clipboard Persistence: Users migrating from `macOS` (where the clipboard is `persistent/system-owned`) to Linux (where it is process-owned) may perceive "ccvv lost my copy if I closed it" as a bug, despite it being platform standard. ccvv's two-phase commit helps history, but not the live clipboard.


Revised Findings: `specs/technical_linux_v1.md`

1. The "Pure Rust" Tray Implementation Trap
 - Severity: Critical
 - Location: Section `8.3` (Implementation Choice) & `8.5` (Menu Structure)
 - Issue: The decision to implement `org.kde.StatusNotifierItem` and `com.canonical.dbusmenu` via raw zbus without a toolkit or helper library (like ksni or libdbusmenu) is a massive, under-scoped engineering risk for v1.
 - Why it matters: `com.canonical.dbusmenu` is a complex, recursive protocol requiring object path registration, layout serialization, property caching, and precise signal handling. Getting this wrong means the tray menu will simply not appear, render as a blank box, or crash the host shell (Gnome Shell/KDE Plasma) in edge cases. Doing this "from scratch" in v1 is a likely failure mode.
 - What is missing: The spec underestimates the complexity of the D-Bus menu protocol. It treats it as a simple "export properties" task, but it requires a full object-model mirror.
 - Suggested correction: Mandate the use of a crate like ksni (which wraps `StatusNotifierItem` logic) or libappindicator (via bindings) for v1. "Pure Rust via zbus" should be a post-v1 optimization, not the critical path for shipping.

2. `X11` Event Loop & Self-Triggering Races
 - Severity: High
 - Location: Section `6.2.3` (`X11` Double-Copy Detection) & `6.2.5` (Writing Back)
 - Issue: The mechanism to suppress self-generated events ("timestamp matches active write-back token") is flaky due to `X11` timestamp granularity and server re-ordering.
 - Why it matters: If ccvv writes to the clipboard, the X server emits `XFixesSelectionNotify`. If the timestamp comparison is even slightly off (`e.g`. server assigns a newer timestamp than the client requested, which can happen if `CurrentTime` is involved or if another client touches the selection atom), ccvv will see its own write as a "new copy". This creates an infinite loop: ccvv write -> event -> ccvv read -> hash match -> double-copy trigger -> ccvv write...
 - What is missing: A robust "owner check" is insufficient because race conditions exist where ownership changes momentarily.
 - Suggested correction: The suppression logic must rely primarily on Window `ID` ownership. If the `SelectionNotify` says the new owner is ccvv's own hidden window, it `MUST` be ignored unconditionally, regardless of timestamp. The spec mentions checking "owner class" but should explicitly mandate "Ignore `ALL` events where Owner == Self".

3. Wayland "Seat" Blindness
 - Severity: High
 - Location: Section `6.3.3` (Wayland Double-Copy Detection)
 - Issue: The spec assumes "receive a new regular clipboard selection event for seat S". In ext-data-control, the client must explicitly bind to available seats.
 - Why it matters: On a multi-seat system (uncommon but possible), or a system where the "default" seat isn't named seat0, hardcoding or assuming a single seat will fail. Furthermore, ccvv needs to know which seat has input focus to know if the "double copy" is relevant, but data-control doesn't strictly expose focus.
 - What is missing: A definition of how ccvv enumerates and binds to seats. Does it listen to all seats?
 - Suggested correction: Specify that ccvv must use `wl_registry` to enumerate `wl_seat` globals and bind a `data_control_device` for every available seat. It should maintain independent double-tap state per seat.

4. The "Legacy" Tray Helper Communication Gap
 - Severity: Medium
 - Location: Section `4.1` (Optional Legacy Tray Helper)
 - Issue: The spec puts the `XEmbed` helper in a separate binary but defines zero communication protocol between the main daemon and this helper.
 - Why it matters: If the main daemon (pure Rust) is running, and the `XEmbed` helper (`GTK`) is handling the tray, how does the helper know when to show the "Success" icon? How does the helper send "Pause" commands back to the daemon?
 - What is missing: An `IPC` spec for the helper. Is it another socket? D-Bus?
 - Suggested correction: Define a standard local `IPC` interface (likely the existing `ccvv.sock`) that the helper connects to. The helper should act as a dumb `UI` frontend for the daemon's state.

5. Double-Tap Timing vs. App Latency
 - Severity: Medium
 - Location: Section `7.2` (Timing Window)
 - Issue: Redefining "double-tap" as "double-write" ignores application latency.
 - Why it matters: In an `IDE` (`IntelliJ`) or Electron app, Ctrl+C might take 50-100ms to actually flush to the X server/Compositor. If the user types Ctrl+C Ctrl+C with 150ms separation, the events might arrive at the daemon with 5ms separation (bulk flush) or 300ms separation (processing lag).
 - What is missing: Acknowledgement that "clipboard write time" != "keystroke time".
 - Suggested correction: The default timing window on Linux likely needs to be wider (`e.g`., 500ms fixed) or use a "minimum separation" filter to discard "machine-speed" double-writes (which look like duplicate events) vs "human-speed" double-writes.

6. `DoS` via `HTML` Parsing
 - Severity: Medium
 - Location: Section `6.5` (Rich Text Extraction)
 - Issue: No explicit size limit on the input `HTML` blob before parsing.
 - Why it matters: html5ever is a robust parser, but parsing a 50MB `HTML` clipboard offering (`e.g`. copying a massive table from a browser) will spike `CPU` and memory, potentially freezing the daemon or triggering `OOM`.
 - What is missing: An explicit limit on the size of the text/html target payload before it is passed to the parser.
 - Suggested correction: Enforce a hard limit (`e.g`., 2MB) on the text/html atom read. If larger, drop the `HTML` and fall back to plain text immediately.


#############################

I have three concrete architectural suggestions and two feature ideas
  that would significantly improve the robustness and "Linux-native" feel of the project.


  1. Architectural: The "Headless Core + UI Sidecars" Model


  The Problem: The current spec tries to jam the D-Bus Tray, the Legacy XEmbed helper, and
  the Core Daemon logic into a complex web of "features" and optional binaries. This makes
  the ccvv-linux binary heavy and fragile.


  The Fix: Split the architecture formally.
   * ccvv-daemon (The Core): A strictly headless binary. It owns the ccvv-lib logic, the
     config, the history DB, and the X11/Wayland connection. It exposes the Unix Domain
     Socket ($XDG_RUNTIME_DIR/ccvv.sock). It has zero UI code.
   * ccvv-tray-sni (The Modern UI): A small, separate binary (or crate feature) that
     connects to the socket. It handles only StatusNotifierItem and D-Bus menus. If it
     crashes, the daemon survives.
   * ccvv-tray-legacy (The Legacy UI): The GTK/XEmbed helper. It connects to the exact same
     socket.


  Why this is better:
   * Stability: If the GUI toolkit (GTK/Qt/D-Bus) crashes or hangs (common on Linux), your
     clipboard daemon doesn't die.
   * Simplicity: The daemon logic becomes pure Rust async loop without fighting glib or dbus
     event loops.
   * Flexibility: Users on i3/Sway who don't want a tray icon can just run the daemon. Users
     who want a TUI status monitor can build one easily against the socket.

  2. Feature: "Sync PRIMARY" (The Linux Power-User Feature)


  The Context: Linux has two clipboards: CLIPBOARD (Ctrl+C) and PRIMARY (Selection). The
  spec currently ignores PRIMARY.


  The Idea: Add an opt-in feature: "Sanitize and Sync PRIMARY → CLIPBOARD".
   * Workflow: User selects dirty text (populates PRIMARY). User middle-clicks (pastes
     PRIMARY).
   * ccvv behavior: When PRIMARY changes, ccvv reads it, sanitizes it, and writes the clean
     result to CLIPBOARD.
   * Benefit: This allows users to "select to clean." They select text, then hit Ctrl+V to
     paste the clean version. It bridges the gap between the two clipboards in a way that
     enforces hygiene.

  3. UX: A TUI Config Editor (ccvv config-tui)


  The Context: You deferred the GUI Preferences window for Linux. Editing
  ~/.ccvv/config.toml by hand is fine, but discoverability of regex rules is low.


  The Idea: Since you are already in Rust, ship a lightweight Terminal User Interface (TUI)
  using ratatui.
   * Function: Visual editor for the TOML config.
   * Features: Toggle booleans with Space, edit regex rules with syntax highlighting, browse
     History with full-screen preview.
   * Why: It's much cheaper to build than a GTK/Qt window, works over SSH, fits the target
     demographic (devs), and solves the "No GUI" problem for v1.

  4. Implementation: "Atomic" Ownership State Machine

  The Problem: The "timestamp check" for suppressing self-writes is flaky.


  The Fix: Implement a strict Ownership State Machine for X11/Wayland.
   1. State: Listening. (We do not own clipboard).
   2. Event: Detect Trigger.
   3. Action: SetSelectionOwner. Transition to State: Owning.
   4. State: Owning. In this state, ignore all SelectionNotify events for the clipboard we
      own. We are the source of truth. We only serve SelectionRequest.
   5. Event: SelectionClear (someone else took ownership). Transition to State: Listening.


  Why: This removes the need for heuristic timestamp checks entirely. If we are the owner,
  we know the content is clean (because we wrote it). We only sanitize when we lose and then
  regain ownership (or see a change while not owner).

  5. Packaging: The "Nix Flake" Low-Hanging Fruit


  The Idea: Add a flake.nix to the repo root.
  Why:
   * It's a tiny text file (mostly boilerplate).
   * It instantly makes the tool "installable" on any Linux distro with Nix, bypassing the
     .deb/.rpm/AUR hell during the early alpha phase.
   * It serves as a "reproducible build environment" for your own CI, replacing complex
     Docker containers.
   * The intersection of "Rust users", "Linux users", and "Nix users" is a single circle.
     It's a high-value signal to that community.


  Summary of Recommendations
  If I were running this project, I would:
   1. Split the daemon and tray immediately to de-risk the release.
   2. Adopt the "State Machine" ownership model to fix the infinite-loop bugs before they
      happen.
   3. Ship a flake.nix for easy developer adoption.
   4. Defer the GUI entirely in favor of a ccvv config TUI or just good documentation.
