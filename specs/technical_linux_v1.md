# Technical Specification: ccvv Linux v1

**Status:** Draft
**Scope:** Linux desktop daemon/tray v1 - Rust core library, native X11/Wayland shell
**Target distros:** Ubuntu 22.04+, Debian 12+, Fedora 39+, Arch Linux
**Target display servers:** X11 and Wayland
**Reference:** [`specs/functional_v1.md`](functional_v1.md)
**Previous:** [`specs/technical_v1.md`](technical_v1.md)

---

## 1. Architecture Overview

ccvv on Linux is a native Rust desktop daemon that links `ccvv-lib` directly and adds only the platform shell: clipboard integration, lifecycle management, packaging, and optional UI sidecars. Stage 1 (rich-text extraction) is platform-specific — it handles Linux clipboard formats including `text/html` with inline-code semantic hints (`<code>`, `<kbd>`, `<samp>`, `<tt>`) that are converted to backtick-wrapped plain text. Stages 2–8 (normalize, whitespace, agent-strip, structural-detection, URL, auto-wrapper, user-rules) all live in `ccvv-lib` with no Linux-specific transform logic.

```
┌──────────────────────────────────────────────────────────────────┐
│                         ccvv-lib (Rust)                         │
│                                                                  │
│  Transform Pipeline  |  TOML Config  |  History (SQLite)        │
└───────────────┬───────────────────────────────┬──────────────────┘
                │ direct link                    │ direct link
        ┌───────┴────────┐               ┌───────┴────────┐
        │ ccvv CLI       │               │ ccvv-linux     │
        │ core/ccvv-cli  │               │ core/ccvv-linux│
        │ stdin/stdout   │               │ headless daemon│
        └────────────────┘               └───────┬────────┘
                                                  │ runtime socket
                         ┌────────────────────────┼────────────────────────┐
                         │                        │                        │
                    X11 backend              Wayland backend         Optional UI sidecars
                  XFixes + selections      ext/wlr data-control     SNI / legacy tray
```

> **Decision:** Add a new workspace Linux crate, `core/ccvv-linux`, with a headless daemon binary and optional tray sidecars, instead of growing daemon mode inside `ccvv-cli`.
>
> **Rationale:** The CLI and the desktop daemon have different startup models, dependencies, feature flags, and packaging concerns. Keeping them separate preserves the CLI as a small, headless-safe tool while allowing the Linux daemon to carry display-server code and session lifecycle logic without forcing tray/runtime UI code into the critical clipboard path.

### 1.1 Process Model

`ccvv-linux` is a single user-session daemon with four responsibilities:

1. Detect clipboard change events from the active display backend.
2. Decide whether a change is a double-copy candidate.
3. Read clipboard text, run `ccvv-lib`, and write cleaned text back.
4. Publish status and accept local control commands over a private runtime socket.

Optional UI sidecars connect to the same runtime socket and are replaceable. If a sidecar crashes or no tray host exists, the daemon continues running.

The daemon is single-instance per desktop session. It is not a system service.

### 1.2 Non-Goals

- No Linux FFI layer. The daemon links `ccvv-lib` directly.
- No GTK or Qt runtime dependency for the core daemon.
- No Linux-only transform stages that diverge from `ccvv-lib` after acquisition.
- No kernel modules, compositor patches, or privileged helpers.
- No global keyboard snooping on Wayland.

---

## 2. Platform Scope & Capability Matrix

Linux cannot be described as one platform. `ccvv-linux` therefore defines capability by display server and compositor, not by distro alone.

### 2.1 Shipping Capability Matrix

| Environment | Clipboard monitor | Automatic double-copy | Manual clean from tray | Notes |
|-------------|-------------------|-----------------------|------------------------|-------|
| X11 session | Yes | Yes | Yes | Primary implementation uses `XFixes` selection events |
| Wayland + KDE/KWin | Yes | Yes | Yes | Use `ext-data-control-v1` when available |
| Wayland + Sway/wlroots | Yes | Yes | Yes | Prefer `ext-data-control-v1`; fall back to `wlr-data-control-unstable-v1` on older releases |
| Wayland + Hyprland | Yes | Yes | Yes | Prefer `ext-data-control-v1`; fall back to `wlr-data-control-unstable-v1` on older releases |
| Wayland + GNOME/Mutter | No native monitor | No | Yes, explicit only | Primary path is a portal-managed `GlobalShortcuts` hotkey; normal app `Ctrl+C` remains unobservable |
| XWayland app under supported Wayland compositor | Yes | Yes | Yes | Observed at compositor clipboard layer, not by attaching to XWayland directly |
| Headless / SSH / no display server | No | No | No | Use `ccvv` CLI only |

### 2.2 Shipping vs Deferred

| Capability | Status | Reason |
|------------|--------|--------|
| Native X11 daemon | Shipping | Mature event model; no extra privileges beyond X server access |
| Native Wayland daemon on KWin, Sway, Hyprland | Shipping | Existing compositor protocols make clipboard-manager behavior possible today |
| Native Wayland daemon on GNOME/Mutter | Limited in v1 | No shipping native clipboard-manager protocol for background automatic mode |
| Portal-based GNOME/Mutter explicit hotkey | Shipping | Most GNOME-native path for explicit invocation without a Shell extension |
| Optional legacy XEmbed tray helper | Shipping, optional package/feature | Needed only for older X11 tray hosts |

> **Decision:** Treat Wayland as a first-class backend, but make capability runtime-detected and compositor-specific.
>
> **Rationale:** Wayland does not provide a uniform clipboard-manager capability across compositors. Shipping one native backend that probes `ext-data-control-v1` first and `wlr-data-control-unstable-v1` second is realistic today. Pretending GNOME/Mutter has the same capability would be false.

---

## 3. Threat Model & Security Architecture

Linux changes the threat model materially relative to macOS:

- On X11, any same-display client is already in a weak isolation domain.
- On Wayland, clipboard-manager behavior is explicitly permissioned by compositor protocol.
- On both, clipboard content remains untrusted input.

### 3.1 Assets

| Asset | Location | Sensitivity | Threat |
|-------|----------|-------------|--------|
| Raw clipboard text | Memory, optional history DB | Critical | Secret leakage, unintended persistence |
| Cleaned clipboard text | Clipboard, history DB | High | Same-user clipboard snooping |
| Config | `~/.ccvv/config.toml` | High | Integrity attack via rewrite rules |
| History DB | `~/.ccvv/history.db` | High | Same-user read, backup leakage |
| Local control socket | `$XDG_RUNTIME_DIR/ccvv.sock` | Medium | Same-user command injection if permissions are wrong |

### 3.2 Privacy Invariants

1. `ccvv-linux` MUST NOT make network connections.
2. `ccvv-linux` MUST NOT reimplement transform logic outside `ccvv-lib`.
3. `ccvv-linux` MUST treat clipboard data as untrusted input.
4. `ccvv-linux` MUST NOT persist raw clipboard text by default.
5. `ccvv-linux` MUST create `~/.ccvv/` with `0700` and files with `0600`.
6. `ccvv-linux` MUST degrade to manual or CLI-only mode rather than requesting broader permissions than the backend naturally provides.

### 3.3 X11 Trust Boundary

On X11, the X server already permits broad clipboard and input observation among same-session clients. `ccvv-linux` does not meaningfully widen that boundary; it participates in an already weak isolation model.

Implications:

- No extra OS permission prompt is required.
- Clipboard monitoring via `XFixes` is not a privilege escalation in the X11 model.
- Global key capture mechanisms such as `XRecord` increase risk and are therefore avoided.

### 3.4 Wayland Trust Boundary

On Wayland, background clipboard access is not generally available. A compositor must explicitly expose a clipboard-manager protocol such as `ext-data-control-v1` or `wlr-data-control-unstable-v1`.

Implications:

- Automatic mode exists only where the compositor exposes clipboard-manager capability.
- No passive global keyboard snooping is possible for a normal client.
- GNOME/Mutter explicit mode may use a portal-managed global shortcut, but passive automatic mode does not.
- A long-lived session for passive observation of ordinary copy events would expand the trust boundary and is therefore not part of the v1 shipping path.

---

## 4. Repository Layout

```
ccvv/
├── core/
│   ├── Cargo.toml
│   ├── deny.toml
│   ├── ccvv-lib/
│   ├── ccvv-cli/
│   └── ccvv-linux/
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs
│           ├── app.rs
│           ├── single_instance.rs
│           ├── detection.rs
│           ├── control/
│           │   ├── mod.rs
│           │   └── socket.rs
│           ├── clipboard/
│           │   ├── mod.rs
│           │   ├── html.rs
│           │   ├── targets.rs
│           │   └── gnome.rs
│           ├── hotkey/
│           │   ├── mod.rs
│           │   └── portal.rs
│           ├── backend/
│           │   ├── mod.rs
│           │   ├── x11.rs
│           │   ├── wayland.rs
│           │   └── none.rs
│           ├── ui_protocol.rs
│           └── bin/
│               ├── ccvv-tray-sni.rs
│               └── ccvv-indicator-legacy.rs
```

### 4.1 UI Sidecars

The daemon owns all clipboard, history, config, and backend state. UI binaries are dumb frontends over the runtime socket:

- `ccvv-tray-sni`: modern StatusNotifierItem sidecar
- `ccvv-indicator-legacy`: optional legacy XEmbed/AppIndicator sidecar

Both sidecars:

- subscribe to daemon status updates
- issue local commands such as `Pause`, `Resume`, `CleanNow`, and `Quit`
- contain no clipboard/backend logic

`ccvv-indicator-legacy` may link `libayatana-appindicator` and GTK3. The primary daemon does not.

> **Decision:** Keep tray code out of the main daemon binary and isolate both SNI and legacy tray support in socket-connected sidecars.
>
> **Rationale:** Tray stacks are the least reliable part of Linux desktop integration. Isolating them keeps the clipboard daemon alive if the D-Bus menu layer, GTK stack, or tray host misbehaves.

---

## 5. Linux Runtime Architecture

### 5.1 Backend Trait

The daemon abstracts display-server behavior behind one trait:

```rust
trait ClipboardBackend {
    fn capability(&self) -> BackendCapability;
    fn subscribe(&mut self) -> Result<BackendStream>;
    fn read_snapshot(&mut self) -> Result<ClipboardSnapshot>;
    fn write_plain_text(&mut self, text: &str) -> Result<WriteToken>;
    fn source_name(&self) -> &'static str;
}
```

`ClipboardSnapshot` contains:

- `seat_id`
- `selection_kind`: `Clipboard` or `Primary`
- `acquired_plain_text`
- `acquired_html`, optional
- `timestamp`
- `backend_serial`
- `is_self_write`, derived from `WriteToken`

`ccvv-lib` receives only text plus config/history calls. Backend-specific serials and protocol objects never leak into the shared library.

Acquisition is platform-specific; transformation is not. Stage 1 clipboard extraction may differ by platform, but the shared transform pipeline continues to own cleanup behavior.

### 5.2 Main Loop

The daemon main loop is:

1. Resolve config from `~/.ccvv/config.toml`.
2. Bind the private control socket in `$XDG_RUNTIME_DIR`.
3. Select backend from runtime environment.
4. Publish status over the runtime socket and optionally supervise one tray sidecar for the current session.
5. Consume clipboard events.
6. For a confirmed trigger, prepare the history entry through `ccvv-lib`.
7. Write cleaned text back using the same backend and verify backend ownership/offer installation.
8. Commit history and update published daemon state.

### 5.3 Crash Consistency & Two-Phase Commit

Linux keeps the same high-level invariant as the existing desktop shell:

- never publish cleaned clipboard state without a recoverable history record
- never commit history for a clipboard publish that failed

Protocol:

1. **Prepare:** Build cleaned text and stage an uncommitted history entry.
2. **Publish:** Install cleaned clipboard ownership/offer on the active backend.
3. **Commit:** Mark the history entry committed only after publish succeeds.
4. **Recover:** On startup, delete abandoned uncommitted entries.

Linux-specific caveat:

- X11 and Wayland clipboard ownership is process-backed. If the daemon crashes after publish, clipboard contents can disappear even though the history entry is committed. This is acceptable because the history entry remains restorable, but it MUST be documented as a platform limitation and mitigated where possible through clipboard-manager interop.

### 5.4 Backend Selection

Order:

1. If `WAYLAND_DISPLAY` is set and Wayland backend initializes successfully, use Wayland.
2. Else if `DISPLAY` is set and X11 backend initializes successfully, use X11.
3. Else run in `none` backend, exposing only diagnostics and explicit CLI handoff.

> **Decision:** Prefer Wayland when both `WAYLAND_DISPLAY` and `DISPLAY` are present.
>
> **Rationale:** In a Wayland session, `DISPLAY` usually refers to XWayland. Attaching the daemon to XWayland would provide only a partial clipboard view and would miss native Wayland clipboard ownership changes.

### 5.5 Singleton & Local Control

Single-instance enforcement and second-instance command forwarding use a Unix domain socket:

```
$XDG_RUNTIME_DIR/ccvv.sock
```

Properties:

- created with `0600`
- owned by the current user
- removed and rebound if stale
- used only for local commands such as `pause`, `resume`, `clean-now`, and `quit`

> **Decision:** Use a private runtime socket for singleton/control, not mutable session-bus methods.
>
> **Rationale:** The session bus is already required for tray hosting, but it is too broad a surface for mutating commands. The runtime socket keeps same-user control explicit and permission-bounded.

---

## 6. Clipboard Integration

### 6.1 Acquisition Contract

All backends normalize clipboard acquisition to the same order:

1. Try `text/html` if available and if HTML extraction is enabled.
2. Try UTF-8 plain text.
3. Fallback to legacy plain-text targets.
4. If no text target exists, skip.

If both text and non-text targets exist, ccvv acquires only the text path. When ccvv later republishes cleaned output, it becomes the new owner and offers text targets only.

Acquisition does not sanitize. It only converts clipboard offers into a plain-text candidate and optional HTML side channel.

This is an explicit product tradeoff, not an implementation accident: mixed-media clipboard copies become text-only after ccvv sanitizes them.

### 6.2 X11 Backend

#### 6.2.1 Protocol Choice

The X11 backend uses:

- `XFixesSelectSelectionInput` for selection-change notification
- A hidden daemon-owned window for clipboard ownership and `SelectionRequest` handling
- Standard X selections for read/write
- ICCCM timestamped ownership semantics
- `INCR` receive/send support for large transfers

The daemon subscribes to:

- `CLIPBOARD`: always
- `PRIMARY`: optional, disabled by default

#### 6.2.2 `CLIPBOARD` vs `PRIMARY`

| Selection | Default | Used for double-copy | Reason |
|-----------|---------|----------------------|--------|
| `CLIPBOARD` | On | Yes | Explicit copy target on Linux desktops |
| `PRIMARY` | Off | No | Changes on selection highlight; too noisy for copy semantics |

If `PRIMARY` support is enabled later, it is tracked as a separate channel. It does not share timing state with `CLIPBOARD`, and it MUST NOT participate in automatic double-copy sanitization.

> **Decision:** Ship `CLIPBOARD` monitoring only by default. Do not treat `PRIMARY` as equivalent to copy.
>
> **Rationale:** `PRIMARY` is selection-driven, not copy-driven. Folding it into the same detector would create frequent false triggers when users merely highlight text.

#### 6.2.3 X11 Double-Copy Detection

Algorithm:

1. Receive `XFixesSelectionNotify` for `CLIPBOARD`.
2. If `owner == daemon_hidden_window`, ignore the event unconditionally and remain in `Owning` state.
3. Request the best available text target from the new owner.
4. Support `INCR` if the owner requests incremental transfer.
5. Compute `hash = SHA-256(acquired plain text bytes)`.
6. Compare `(seat_id=default, hash, timestamp, owner class)` to the previous `CLIPBOARD` event.
7. Suppress known clipboard-manager save/restore churn where ownership changes but content and timestamp chain indicate the same underlying copy.
8. If the same hash arrives within the active window, trigger sanitization.
9. Else update baseline and wait for the next event.

The X11 backend is therefore clipboard-event driven, not key-event driven.

The X11 detector uses an ownership state machine:

- `Listening`: another client owns `CLIPBOARD`; detector evaluates incoming events
- `Owning`: ccvv owns `CLIPBOARD`; all self-owner `XFixesSelectionNotify` events are ignored
- `SelectionClear`: another owner takes `CLIPBOARD`; transition back to `Listening`

Timestamps remain useful for stale-request handling and diagnostics, but self-write suppression is keyed first on ownership, not timestamp equality.

#### 6.2.4 X11 Clipboard-Manager Interop

When a `CLIPBOARD_MANAGER` selection owner exists, `ccvv-linux` MUST interoperate with it:

1. advertise the same text targets it can serve
2. respond correctly to `SAVE_TARGETS` if requested
3. document degraded durability when no clipboard manager is present

This does not eliminate all crash-loss scenarios, but it prevents ccvv from being uniquely worse than other X11 clipboard owners.

#### 6.2.5 Writing Back on X11

Write-back steps:

1. Store cleaned text in daemon memory.
2. Call `SetSelectionOwner` for `CLIPBOARD` using the hidden window and the current server timestamp.
3. Serve `SelectionRequest` targets for `TARGETS`, `UTF8_STRING`, `TEXT`, `STRING`, `text/plain;charset=utf-8`, and `INCR` for large replies.
4. Reject stale requests outside the ownership interval.
5. Transition the detector to `Owning` state and mark a write-back token for diagnostics and stale-request correlation.

When ccvv takes ownership on X11, it intentionally drops all non-text targets from the previous owner. This is a known fidelity loss and MUST be documented in user-facing Linux limitations.

### 6.3 Wayland Backend

#### 6.3.1 Shipping Protocol Strategy

The backend probes in this order:

1. `ext-data-control-v1`
2. `wlr-data-control-unstable-v1`
3. No automatic clipboard-manager capability

The daemon does not require a regular toplevel window. It enumerates all advertised `wl_seat` globals, binds the compositor-provided data-control manager for each seat, subscribes to selection changes, and reads/writes clipboard offers through that interface.

#### 6.3.2 Compositor Matrix

| Compositor | v1 behavior | Backend path |
|------------|-------------|--------------|
| KDE/KWin | Full automatic support | `ext-data-control-v1` |
| Sway | Full automatic support | `ext-data-control-v1` on current releases, fallback `wlr-data-control-unstable-v1` for older releases |
| Hyprland | Full automatic support | `ext-data-control-v1` on current releases, fallback `wlr-data-control-unstable-v1` for older releases |
| GNOME/Mutter | Explicit manual support only | Portal clipboard session for `Clean Clipboard Now`; no native automatic monitor path |

This matrix is capability-based, not branding-based. At runtime the code trusts actual protocol advertisement, not desktop name strings.

#### 6.3.3 Wayland Double-Copy Detection

Where data-control is available, the Wayland detector mirrors X11 conceptually, but it is per seat:

1. Discover every compositor-advertised `wl_seat` and bind a data-control device for each seat `S`.
2. Receive a new regular clipboard selection event for seat `S`.
3. Request plain text from the offer for seat `S`.
4. Hash the acquired plain text.
5. Compare with the previous selection event for the same seat `S`.
6. Same hash within the active window triggers sanitization only on seat `S`.
7. Writes from the daemon's own active data source for seat `S` are ignored once on that same seat.

The detector uses clipboard cadence, not `Ctrl+C` observation.

Seat state is independent. The daemon does not assume a single default seat name or a fixed seat count.

#### 6.3.4 GNOME/Mutter Explicit Hotkey Mode

When no supported native clipboard-manager protocol is advertised but `org.freedesktop.portal.GlobalShortcuts` is available, `ccvv-linux` enters GNOME explicit-hotkey mode:

- the daemon registers a dedicated ccvv shortcut such as `Super+Alt+C`
- the portal prompts the user to approve that shortcut
- a single activation triggers `Clean Clipboard Now`
- ordinary app `Ctrl+C` remains unobservable and is never treated as a trigger

The hotkey handler does not monitor application keystrokes. It only receives activation events for the specific ccvv shortcut granted by the portal.

#### 6.3.5 Unsupported Wayland Sessions

If no supported data-control protocol is advertised:

- The daemon starts in limited mode.
- Tray state shows "hotkey/manual only" if the GNOME explicit-hotkey path is available, otherwise "CLI only".
- Ordinary automatic detection is disabled.
- Native clipboard read/write from the host daemon is disabled.
- `Clean Clipboard Now` routes through the same explicit GNOME path when available.
- `ccvv` CLI still works for pipe-based workflows.
- Users may still bind an explicit compositor shortcut to a wrapper command, but that remains outside automatic daemon mode.

This is the v1 behavior for GNOME/Mutter Wayland sessions.

Why ordinary `Ctrl+C, Ctrl+C` still does not ship here:

- a background daemon would need continuous observation of either clipboard changes or key presses
- GNOME/Mutter does not expose a native background clipboard-manager protocol for that
- the portal hotkey path only reports activation of the dedicated ccvv shortcut, not arbitrary application copy commands

> **Decision:** Do not emulate automatic Wayland support via a permanent XWayland-side X11 client.
>
> **Rationale:** That would observe only the XWayland clipboard bridge, not the full compositor clipboard state, and would create backend split-brain behavior in mixed native/legacy app sessions.

### 6.4 XWayland

When the session itself is Wayland and the compositor exposes supported data-control:

- Copies from native Wayland apps are observed through the Wayland backend.
- Copies from XWayland apps are also observed after the compositor bridges clipboard ownership.
- ccvv continues to operate as a Wayland daemon. It does not attach separately to XWayland.

The X11 backend is used only for real X11 sessions.

### 6.5 Rich Text Extraction

macOS Stage 1 uses `NSAttributedString`. Linux has no equivalent cross-desktop rich-text API. The Linux replacement is MIME-driven platform acquisition, not a separate transform pipeline:

1. If the clipboard offers `text/html`, read at most `2 MiB` of that payload.
2. Extract plain text while preserving semantic hints:
   - `<code>`, `<kbd>`, `<samp>`, `<tt>` -> inline backticks
   - `<pre>` and `<pre><code>` -> preserve line breaks and indentation only; do NOT emit Markdown fences
   - spans with explicit monospace `font-family` -> inline code only when unambiguous
3. Pass the resulting plain text to `ccvv-lib`.
4. If HTML is absent, exceeds the cap, or parse fails, use plain text without rich-text inference.

The platform layer MUST NOT emit Markdown tables or fenced code blocks. Structural formatting remains the responsibility of `ccvv-lib`.

| Option | Pros | Cons | Decision |
|--------|------|------|----------|
| Parse `text/html` | Works on browsers, editors, office apps; no GUI toolkit | Heuristic, not perfect | Chosen |
| Parse RTF | Better parity with some apps | No standard cross-desktop clipboard path; more parser surface | Rejected for v1 |
| Skip inference entirely | Simplest | Clear parity loss vs macOS | Rejected |

> **Decision:** Implement HTML-based rich-text inference only. Do not chase RTF parity in Linux v1.
>
> **Rationale:** `text/html` is widely offered and can be parsed in pure Rust. It captures most of the value without introducing platform-specific toolkit dependencies, while keeping extraction bounded and clearly separated from shared transform logic.

---

## 7. Double-Tap Detection

### 7.1 Behavioral Definition

On Linux, "double-tap" means:

- Two clipboard writes of the same acquired text
- To the same logical clipboard channel
- Within the active timing window

This is intentionally different from macOS keystroke detection. On Linux/X11 and supported Wayland compositors, clipboard events are the correct primitive. In GNOME/Mutter explicit mode, the dedicated portal hotkey is not treated as "double-tap"; it is a direct manual trigger.

### 7.2 Timing Window

The timing window is Linux-specific because clipboard write arrival time is not the same thing as keystroke timing:

- Default: `400ms`
- Configurable fixed range: `150ms` to `600ms`
- Adaptive mode: same median-plus-spread algorithm already specified for macOS, but trained only on Linux-confirmed triggers

Adaptive timing records only confirmed double-copy triggers. Self-writes and known clipboard-manager churn are filtered before timing comparison so machine-speed duplicate events do not count as human double-copy input.

### 7.3 Detector State

Per selection channel, store:

- `seat_id`
- `last_hash`
- `last_timestamp`
- `last_backend_serial`
- `last_source_kind`

No clipboard content is persisted for timing purposes beyond the hash.

### 7.4 X11 Detection Options

| Mechanism | Pros | Cons | Decision |
|-----------|------|------|----------|
| `XFixes` selection events | Event-driven, no keylogging, works for menu copy and programmatic copy | Detects clipboard writes, not literal `Ctrl+C` keys | Chosen |
| `XRecord` | Precise keystroke timing | Global key capture surface, extension may be absent, worse privacy story | Rejected |
| `XGrabKey` / `XGrabKeyboard` | Explicit hotkey capture | Conflicts with apps and window manager shortcuts | Rejected |

### 7.5 Wayland Detection Options

| Mechanism | Feasible for v1 | Notes |
|-----------|-----------------|-------|
| Passive global keyboard monitoring | No | Not available to normal Wayland clients |
| `keyboard-shortcuts-inhibit` | No | Designed for a focused surface suppressing compositor shortcuts, not background observation |
| Clipboard event cadence via data-control | Yes, compositor-dependent | Shipping automatic mode |
| `org.freedesktop.portal.GlobalShortcuts` | Maybe, manual-only | Useful for explicit hotkeys, not passive double-copy observation |
| On-demand `org.freedesktop.portal.Clipboard` session | Yes, manual-only | Shipping only for explicit `Clean Clipboard Now` on unsupported native backends |
| Compositor user keybind running `ccvv` | Yes, manual-only | Supported workaround, not automatic mode |

> **Decision:** On Wayland, do not attempt to monitor `Ctrl+C` at all.
>
> **Rationale:** The only robust, non-invasive shipping path is clipboard-event cadence where the compositor exposes it. Keyboard monitoring would either be impossible or would require a larger trust boundary than this product should take on.

---

## 8. Tray / Status Icon

### 8.1 Primary Standard: `StatusNotifierItem`

The modern tray implementation uses a dedicated SNI sidecar on the session D-Bus:

- well-known item name owned by `ccvv-tray-sni`, not the daemon
- menu and icon state mirrored from daemon status received over the runtime socket
- daemon remains tray-agnostic and survives if the sidecar crashes

This is the modern Linux tray path for KDE and most non-GNOME desktop environments.

### 8.2 Legacy Fallback: XEmbed

Some older environments still expect XEmbed-based tray icons. For those cases:

- ship an optional `ccvv-indicator-legacy` helper
- enable it only when the user installs the legacy-tray package or builds with `--features xembed`
- keep it out of the core daemon binary
- connect it to the same runtime socket protocol used by `ccvv-tray-sni`

### 8.3 Implementation Choice

| Option | Pros | Cons | Decision |
|--------|------|------|----------|
| `libappindicator3` / Ayatana as primary | Broad legacy compatibility | Pulls GTK runtime into the main daemon path | Rejected |
| `ksni` crate (or equivalent SNI helper) in a sidecar | Removes raw `dbusmenu` risk from v1, keeps daemon headless | Less direct control than hand-rolled D-Bus | Chosen |
| Raw D-Bus via `zbus` for full `StatusNotifierItem` + `dbusmenu` | No extra abstraction | High protocol complexity and menu/rendering risk for v1 | Deferred |

> **Decision:** Ship SNI through a dedicated sidecar using `ksni` or an equivalent mature helper; keep toolkit-backed tray code optional and separate.

### 8.4 Icon States

Icon assets are symbolic monochrome SVGs plus raster fallbacks:

| State | Meaning |
|-------|---------|
| `active` | Backend live and listening |
| `paused` | Detection suspended by user |
| `success` | 300ms flash after successful clean |
| `limited` | Session running but clipboard integration unavailable |
| `error` | Config or backend failure requiring attention |

### 8.5 Menu Structure

The menu is intentionally shallow:

1. `Clean Clipboard Now`
2. `Pause` / `Resume`
3. `Restore Last Cleaned Item`
4. `Open Config Directory`
5. `Run Diagnostics`
6. `Backend: X11` / `Backend: Wayland` / `Backend: Limited` (disabled label)
7. `Quit`

Full history browsing remains a CLI concern in Linux v1 (`ccvv history`).

`Clean Clipboard Now` is enabled only when the current backend can actually read/write the clipboard:

- always on X11
- on Wayland when native data-control is available
- on limited native sessions only if the GNOME explicit-hotkey path is available

### 8.6 No Tray Host Present

If no tray watcher is present:

- the daemon continues running
- the local runtime socket remains available
- tray sidecars may exit or stay idle
- desktop notifications are optional
- a single warning is logged; the daemon does not exit

This matters on vanilla GNOME Shell, where tray support is not guaranteed.

---

## 9. Autostart & Lifecycle

### 9.1 XDG Autostart

Primary launch path:

```
/etc/xdg/autostart/ccvv.desktop
```

Properties:

- `Type=Application`
- `Exec=ccvv-linux`
- `OnlyShowIn=` omitted
- `X-GNOME-Autostart-enabled=true`
- `NoDisplay=true`

System packages install the desktop file under `/etc/xdg/autostart/`. Per-user installs may instead copy the same file to `~/.config/autostart/ccvv.desktop`.

### 9.2 systemd User Service

Optional advanced alternative:

```
<distro-user-unit-dir>/ccvv.service
```

Recommended unit characteristics:

- not enabled by packages by default
- `Restart=on-abnormal`, not `on-failure`
- daemon exits cleanly if neither `WAYLAND_DISPLAY` nor `DISPLAY` is set
- documented only for desktops that import graphical-session environment into the user manager
- wlroots compositors require an explicit `systemctl --user import-environment DISPLAY WAYLAND_DISPLAY XDG_CURRENT_DESKTOP XAUTHORITY DBUS_SESSION_BUS_ADDRESS` step at login if this mode is used

Package policy:

- XDG autostart is the default first-party launch path
- packages MUST NOT enable both XDG autostart and a systemd user unit by default
- if the user enables the systemd unit manually, docs MUST instruct them to disable the XDG autostart entry to avoid a startup race

### 9.3 Single-Instance Enforcement

Single instance is enforced by binding the runtime socket:

```
$XDG_RUNTIME_DIR/ccvv.sock
```

Advantages over PID files and mutable session-bus methods:

- session-scoped automatically
- no stale file cleanup
- same mechanism also exposes local second-instance command forwarding

If the socket is already owned by a healthy daemon, the new process forwards its requested action to the existing instance and exits.

> **Decision:** Use the runtime socket as the singleton lock and local control channel; use D-Bus only for tray sidecars and portal integration.

### 9.4 Headless and Degraded Sessions

| Environment | Behavior |
|------------|----------|
| No display server | Exit daemon with diagnostic; CLI still works |
| No tray host | Run daemon without tray |
| No session D-Bus | Run daemon without tray or portal hotkey support; local socket still works |
| Unsupported Wayland compositor | Start limited mode; show limited state if tray exists |
| SSH with X11 forwarding | Not supported as a primary mode; use CLI |

---

## 10. Packaging & Distribution

### 10.1 First-Party Artifacts

First-party Linux distribution targets:

- `.deb` for Ubuntu and Debian
- `.rpm` for Fedora
- `PKGBUILD` for AUR
- compressed binary tarball for generic fallback

Optional later artifact:

- `AppImage`, only after tray and Wayland behavior are validated across target distros

### 10.2 Packaging Decision Table

| Format | Ship in v1 | Notes |
|--------|------------|-------|
| `.deb` | Yes | Primary for Ubuntu/Debian |
| `.rpm` | Yes | Primary for Fedora |
| `AUR` `PKGBUILD` | Yes | Source-driven Arch install path |
| Tarball | Yes | Lowest-common-denominator fallback |
| `flake.nix` | Optional | Useful for reproducible dev/build environments and Nix-based installs; not a first-party desktop integration path |
| AppImage | Maybe later | Good fallback once desktop integration is proven |
| Flatpak | No first-party v1 | Sandboxed clipboard/tray behavior is too capability-dependent |
| Snap | No first-party v1 | Strict confinement does not improve the core clipboard problem; classic confinement defeats the point |

### 10.3 Flatpak vs Snap

Flatpak:

- portals now expose clipboard-related APIs
- but clipboard access is tied to portal/backend semantics, not guaranteed native daemon behavior
- tray integration, autostart, and background clipboard-manager behavior are weaker than host install

Snap:

- can grant `x11` and `wayland` interfaces
- does not change compositor limitations on Wayland
- background desktop daemons and tray integration remain less natural than native packages
- `classic` confinement would erase most sandbox benefit

> **Decision:** Do not make Flatpak or Snap first-party Linux distribution channels in v1.
>
> **Rationale:** ccvv is fundamentally a desktop clipboard daemon. The host-installed package is the least surprising and most capable deployment model.

### 10.4 Homebrew on Linux

Homebrew/Linuxbrew may ship the CLI and daemon binaries, but it is not the primary Linux integration story. Formula support is acceptable as an extra distribution channel, not as the reference install path.

### 10.5 Nix / Flakes

A repository `flake.nix` is acceptable as an optional developer and community distribution aid:

- it may provide a reproducible Rust/Linux build environment
- it may expose convenient `nix run` / `nix develop` entry points
- it does not replace first-party `.deb`, `.rpm`, `PKGBUILD`, or tarball artifacts
- it is not the reference path for validating desktop integration behavior

### 10.6 Installed Assets

Package contents:

- `ccvv` CLI binary
- `ccvv-linux` daemon binary
- `ccvv-tray-sni` binary
- `/etc/xdg/autostart/ccvv.desktop`
- tray icons
- optional systemd user unit in the distro user-unit directory
- optional legacy tray helper package

---

## 11. Dependencies & Build

### 11.1 Rust Crates

New crate dependencies are Linux-shell only. `ccvv-lib` remains the only transform engine.

Expected crate families:

- `x11rb` for X11 + XFixes
- `wayland-client`, `wayland-protocols`, and generated protocol bindings
- `zbus` or `ashpd` for portal and local D-Bus integration where needed
- `ksni` or equivalent SNI helper for the modern tray sidecar
- `interprocess` or equivalent for the runtime socket
- `html5ever` or equivalent pure-Rust HTML parser *(note: v1 ships a hand-rolled state-machine parser in `core/ccvv-linux/src/clipboard/html.rs` instead of pulling `html5ever` as a dependency; the hand-rolled parser covers the v1 extraction contract — inline-code semantic hints, `<pre>` whitespace preservation, entity decoding, and a 2 MiB cap — and adding `html5ever` is a deferred dependency-weight decision)*
- `clap` only if the daemon exposes direct subcommands

### 11.2 Feature Flags

```toml
[features]
default = ["x11", "wayland", "tray"]
x11 = []
wayland = []
tray = []
xembed = []
```

Semantics:

- `x11`: compile X11 backend
- `wayland`: compile Wayland backend
- `tray`: compile SNI tray sidecar support
- `xembed`: compile optional legacy tray helper

Implementation note: an earlier draft listed an `html-extract` feature for HTML rich-text extraction. It was removed in implementation because HTML extraction (`core/ccvv-linux/src/clipboard/html.rs`) is small enough to ship unconditionally and the flag did not actually gate any runtime work. The acquisition layer still falls back to plain text whenever HTML is absent, oversized, or malformed.

### 11.3 System Dependencies

The core daemon intentionally avoids GTK, Qt, and `libdbus` as required dependencies.

| Capability | Runtime / build dependency | Notes |
|------------|----------------------------|-------|
| Base daemon | glibc, libgcc_s/libstdc++ | No desktop toolkit required |
| X11 backend | X server with XFixes; no client-side Xlib/libxcb dependency | `x11rb` keeps protocol bindings in Rust |
| Wayland backend | `libwayland-client` | Required only when `wayland` feature is enabled |
| SQLite in `ccvv-lib` | none extra | Already bundled through `rusqlite` |
| SNI tray sidecar | session D-Bus daemon | Separate from the daemon critical path |
| Legacy XEmbed helper | `libayatana-appindicator` + GTK3 | Optional package/feature only |

Example build packages:

| Distro family | Base packages | Optional legacy tray packages |
|---------------|---------------|------------------------------|
| Ubuntu/Debian | `build-essential`, `pkg-config`, `libwayland-dev` | `libayatana-appindicator3-dev` |
| Fedora | `gcc`, `pkgconf-pkg-config`, `wayland-devel` | package name pinned in RPM spec for the target Fedora release |
| Arch | `base-devel`, `pkgconf`, `wayland` | `libayatana-appindicator` |

### 11.4 Static vs Dynamic Linking

Strategy:

- Statically link Rust dependencies.
- Keep display-stack libraries dynamically linked.
- Keep SQLite bundled through existing `ccvv-lib` configuration.
- Do not target fully static `musl` builds for the daemon.

> **Decision:** Ship glibc-targeted dynamic desktop builds, not fully static desktop binaries.
>
> **Rationale:** Wayland, D-Bus, and optional tray integration are naturally dynamic desktop stack dependencies. Forcing full static linkage would increase risk and reduce distro compatibility.

### 11.5 Cross-Compilation

Cross-compilation policy:

- build release tarballs on Ubuntu 22.04 to set the minimum glibc floor
- build native `.deb` and `.rpm` artifacts in distro-specific containers
- validate runtime on actual compositor test jobs, not just compile jobs

Compiling on one distro is not sufficient validation for Linux desktop behavior.

---

## 12. Permissions & Security

### 12.1 Display-Server Permissions

| Backend | Permission model | ccvv v1 behavior |
|---------|------------------|------------------|
| X11 | Access is implied by connection to X server | Automatic clipboard monitoring allowed |
| Wayland data-control | Capability granted by compositor protocol | Automatic mode only where protocol exists |
| Wayland without data-control | No background clipboard-manager capability | Limited mode only |
| Portal-managed global shortcut | Explicit user/session grant | Explicit hotkey mode only on unsupported native backends |

### 12.2 Wayland Security Implications

Wayland specifically prevents several things that X11 historically allowed:

- no passive global keyboard capture by arbitrary background client
- no reliable background clipboard manager without compositor cooperation
- no reason to request broader input-capture behavior for this product

These are features, not bugs. The spec follows them.

### 12.3 Sandboxed Clipboard APIs

The relevant portal interface is `org.freedesktop.portal.GlobalShortcuts`.

They are not the shipping path for Linux v1 because:

1. they introduce a larger consent model than the host daemon needs
2. backend support is still not equivalent to host-native clipboard-manager protocols
3. it does not solve the "passive double-copy" problem for ordinary application copy commands

The one exception is GNOME/Mutter explicit mode. There, a portal-managed dedicated ccvv shortcut is acceptable because the user has explicitly granted and invoked that shortcut.

### 12.4 File Permissions

The daemon enforces:

- `~/.ccvv/` -> `0700`
- `config.toml` -> `0600`
- `history.db` -> `0600`
- temporary files -> `0600`

If permissions are broader, the daemon warns and repairs them when safe.

### 12.5 Control Surface

Mutable commands do NOT travel over the session bus.

The session bus is used only for:

- portal interfaces such as `org.freedesktop.portal.GlobalShortcuts`
- tray sidecars that own `org.kde.StatusNotifierItem` exports

Local command forwarding uses the runtime socket and supports:

- `Pause`
- `Resume`
- `CleanNow`
- `Quit`
- `GetStatus`
- `SubscribeStatus`

The daemon does not expose raw clipboard contents over D-Bus or the runtime socket.

### 12.6 Network Isolation Enforcement

The Linux daemon inherits the same no-network policy as the rest of the workspace:

1. `core/deny.toml` MUST ban network-capable crates and features.
2. CI MUST run `cargo deny check bans advisories licenses sources`.
3. New Linux-only dependencies MUST be reviewed for hidden network stacks.
4. Portal, Wayland, X11, and D-Bus integrations MUST remain local IPC only.

---

## 13. Testing Strategy

### 13.1 Unit Tests

Unit tests cover:

- detector timing state machine
- per-seat detector isolation
- backend self-write suppression and ownership-state transitions
- HTML extraction heuristics
- HTML size-cap fallback
- config and degraded-mode resolution
- sidecar socket protocol parsing

These tests use mocked backend events and do not need a live display server.

### 13.2 X11 Integration Tests

Run under `Xvfb`:

1. start an isolated X server
2. run `ccvv-linux` with `DISPLAY` set
3. use a small test helper to own `CLIPBOARD`
4. emit two identical clipboard writes inside and outside the timing window
5. assert sanitize/no-sanitize behavior
6. assert write-back events are ignored
7. assert `INCR` receive/send for large clipboard payloads
8. assert behavior with a cooperating clipboard manager present

### 13.3 Wayland Integration Tests

Run under headless Sway:

1. start a private user bus
2. launch Sway on headless backend
3. run `ccvv-linux` with `WAYLAND_DISPLAY`
4. drive clipboard writes with a Wayland test helper
5. assert data-control detection and write-back behavior
6. run once with `ext-data-control-v1`
7. run once with `wlr-data-control-unstable-v1` fallback where supported

GNOME/Mutter behavior is verified as a limited-mode startup test plus explicit portal-manual-clean test, not as an automatic-mode test.

### 13.4 XWayland Integration Tests

Use the same Sway job with XWayland enabled:

1. generate clipboard writes from X11 clients
2. verify the Wayland backend observes the compositor-bridged clipboard changes
3. verify no separate X11 backend is started

### 13.5 KWin and Hyprland Integration Tests

Separate jobs validate:

- KWin with `ext-data-control-v1`
- Hyprland on a current release exposing `ext-data-control-v1`

These are required because compositor-specific behavior is part of the shipping contract.

### 13.6 Packaging and Distro Tests

Containerized jobs validate:

- installability of `.deb` on Ubuntu 22.04 and Debian 12
- installability of `.rpm` on Fedora 39+
- buildability of AUR package metadata on Arch
- presence and correctness of `/etc/xdg/autostart/ccvv.desktop`
- buildability of the optional `xembed` helper package where enabled

These jobs do not replace compositor integration tests.

### 13.7 Failure-Mode Tests

Explicit test cases:

- no tray host
- no session D-Bus
- Wayland compositor without data-control
- config permission mismatch
- daemon already running
- portal clipboard unavailable
- multi-seat event isolation
- mixed-content clipboard overwrite becomes text-only

---

## 14. CI Strategy

### 14.1 Build Matrix

GitHub Actions matrix:

| Job | Purpose |
|-----|---------|
| `cargo test -p ccvv-lib` | Shared engine regression |
| `cargo test -p ccvv-cli` | CLI regression |
| `cargo test -p ccvv-linux --features x11,wayland,tray,html-extract` | Linux daemon unit tests |
| X11 large-transfer integration | `INCR` end-to-end |
| Ubuntu package build | `.deb` artifact |
| Fedora package build | `.rpm` artifact |
| Arch package lint/build | `PKGBUILD` validation |
| X11 integration | `Xvfb` end-to-end |
| Sway integration | headless Sway end-to-end |
| KWin integration | `ext-data-control-v1` end-to-end |
| Hyprland integration | current-release smoke/integration |
| GNOME limited-mode integration | startup + portal-hotkey path |
| XWayland integration | bridged session test |
| SNI sidecar build/smoke test | tray sidecar + socket protocol build check |
| Optional `xembed` build | legacy helper compile/package check |

### 14.2 Release Validation

Before Linux release:

1. verify all packaging artifacts install
2. verify X11 automatic mode
3. verify Sway automatic Wayland mode
4. verify KWin automatic Wayland mode
5. verify Hyprland automatic Wayland mode
6. verify GNOME/Mutter limited-mode UX, diagnostics, and portal-managed hotkey path
7. verify no network-capable dependencies enter the graph

---

## 15. Implementation Phases

### Phase 1: Crate Skeleton & Control Plane

- add `core/ccvv-linux`
- implement runtime-socket singleton/control and shared sidecar protocol
- add limited `none` backend

### Phase 2: X11 MVP

- implement XFixes backend
- implement X11 ownership state machine and write-back guard
- wire `ccvv-lib` calls
- ship autostart assets and the modern SNI sidecar

### Phase 3: Wayland Automatic Mode

- implement `ext-data-control-v1`
- implement `wlr-data-control-unstable-v1`
- add compositor capability reporting
- enumerate seats and make detector state seat-aware

### Phase 4: GNOME/Mutter Explicit Hotkey Mode

- implement portal-managed dedicated ccvv hotkey
- gate menu enablement on actual backend/manual capability
- make the dedicated hotkey trigger immediate `Clean Clipboard Now`

### Phase 5: Packaging & CI

- `.deb`, `.rpm`, `PKGBUILD`, tarball
- compositor integration jobs

### Phase 6: Optional Legacy Tray

- add `xembed` helper
- package separately

---

## 16. Accepted Risks & Known Limitations

### 16.1 Accepted Risks

| Risk | Severity | Mitigation |
|------|----------|------------|
| X11 already permits broad same-display observation | High | Documented platform reality; no extra key capture added |
| GNOME/Mutter Wayland lacks ordinary automatic mode | Medium | Clear limited-mode UX and explicit portal-backed ccvv hotkey |
| HTML rich-text inference is heuristic | Low | Conservative parsing; fallback to plain text |
| No tray host in some environments | Low | Daemon continues without tray |
| Clipboard durability remains process-backed on some backends | Medium | Two-phase commit plus clipboard-manager interop where available; history remains restorable |
| Mixed-content copies become text-only after sanitize | Medium | Explicitly documented limitation; ccvv republishes text targets only |

### 16.2 Known Limitations

- Linux double-copy detection is clipboard-event based, not keystroke based.
- `PRIMARY` selection is not part of default automatic mode.
- Even if `PRIMARY` monitoring is enabled later, it is never used for automatic double-copy sanitization.
- GNOME/Mutter Wayland does not ship ordinary `Ctrl+C, Ctrl+C` detection in v1.
- The GNOME/Mutter portal hotkey is a single-action manual clean trigger, not a double-activation gesture.
- Sanitizing a mixed-content clipboard copy replaces it with text-only targets.
- Full graphical history browser is not part of Linux v1.
- Flatpak and Snap are not first-party distribution targets.

---

## 17. v1 Scope Exclusions

Deferred beyond Linux v1:

- passive portal-driven observation of ordinary copy events on GNOME/Mutter
- global Wayland keyboard monitoring
- RTF parsing and font-attribute parity with macOS
- raw hand-rolled `dbusmenu` implementation in the daemon
- full GUI preferences window
- Linux-specific transform stages outside `ccvv-lib`
- system-wide root service or multi-user daemon mode
