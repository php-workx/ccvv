# Linux v1 Remediation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Close all current gaps between `specs/technical_linux_v1.md` and the live `ccvv-linux` implementation, then harden runtime, protocol, packaging, and validation coverage for v1.

**Architecture:** Keep `ccvv-linux` as a headless daemon that owns clipboard orchestration and history writes, with UI functionality isolated in socket-driven tray sidecars. Keep `ccvv-lib` as the transformation engine.

**Tech Stack:** Rust, `x11rb`, `wayland-client`, `wayland-protocols`, `zbus`/`ashpd`, `ksni`, `interprocess` or std UDS, plus current `ccvv-lib`.

---

## Current State vs Spec (March 9, 2026)

| Area | Spec target | Current state | Gap |
|---|---|---|---|
| X11 backend | Functional event-driven clipboard integration | Stubbed `Unavailable` path | Critical |
| Wayland backend | `ext-data-control-v1` + `wlr-data-control-unstable-v1` + seat-aware detection | Stubbed and limited fallback only | Critical |
| Backend selection | Runtime capability probing and session-aware fallback | Env-based heuristic and GNOME heuristic | High |
| Detector state | Per-seat/selection-state, ownership-state-aware suppression | Single-channel generic hash window | High |
| Tray menu/state | Sidecar action matrix incl. Restore/Diagnostics/backend label | Partial, missing key items | Medium |
| Clipboard acquisition | text/html first, plain-text fallback, 2 MiB cap | Helper exists, no backend caller |
| Autostart/lifecycle | `.desktop`, optional systemd user unit policy | Not implemented | High |
| Packaging | `.deb/.rpm/PKGBUILD/tarball`, checks | Not implemented | High |
| CI matrix | X11/Wayland integration and packaging jobs | Only general Rust CI | Medium |
| Protocol tests | Integration coverage for compositor behavior | Unit tests for control/socket only | Medium |

---

## Execution Strategy

Use this in short slices: one focused task, then commit. For each task:

1. write a failing test
2. run it to confirm failure
3. implement minimal fix
4. run full local checks
5. commit

All commands below are scoped to `core` unless noted.

---

### Task 1: Convert backend trait and control flow to event-driven architecture

**Files**
- Modify: `core/ccvv-linux/src/backend/mod.rs`
- Modify: `core/ccvv-linux/src/app.rs`
- Modify: `core/ccvv-linux/src/detection.rs`

**Step 1: Write failing tests**
- Add unit tests that enforce:
  - `read_snapshot` is event-driven-capable (`BackendStream`) not polling-only.
  - Detector consumes channel-specific snapshots and emits outcomes only for the same `seat_id + selection_kind`.

**Step 2: Run tests**
- `cargo test -p ccvv-linux backend`
Expected: FAIL (missing API and behavior).

**Step 3: Implement minimal change**
- Add stream-like API to `ClipboardBackend` for event snapshots.
- Keep `read_snapshot` for compatibility but make main loop consume events via backend stream.
- Update `DetectionState` usage in `app.rs` to pass snapshot metadata consistently.

**Step 4: Run checks**
- `cargo test -p ccvv-linux`
Expected: PASS for existing tests after updates.

**Step 5: Commit**
- `git add core/ccvv-linux/src/backend/mod.rs core/ccvv-linux/src/app.rs core/ccvv-linux/src/detection.rs`
- `git commit -m "refactor: add event-first backend snapshot flow"`

---

### Task 2: Implement real X11 backend skeleton (XFixes + selection ownership)

**Files**
- Rewrite: `core/ccvv-linux/src/backend/x11.rs`
- Modify: `core/ccvv-linux/Cargo.toml`

**Step 1: Write failing tests**
- Add unit tests for source metadata:
  - `SelectionNotify` owner and selection mapping.
  - Backend reports automatic capability when X11 init succeeds.
- Add placeholder integration test structure under `core/ccvv-linux/tests/x11_integration.rs`.

**Step 2: Run tests**
- `cargo test -p ccvv-linux x11 -- --nocapture`
Expected: FAIL until integration harness exists.

**Step 3: Implement**
- Connect to X server with `x11rb`.
- Detect compositor clipboard changes via `XFixesSelectSelectionInput`.
- Implement ownership state tracking with `Listening → Owning → SelectionClear`.
- Ensure all self-owned events are ignored in `Owning`.
- Return `ClipboardSnapshot` with `selection_kind = Clipboard` and populated `backend_serial`/`acquired_plain_text`.

**Step 4: Run checks**
- `xvfb-run -a cargo test -p ccvv-linux x11 -- --nocapture`

**Step 5: Commit**
- `git add core/ccvv-linux/Cargo.toml core/ccvv-linux/src/backend/x11.rs core/ccvv-linux/tests/x11_integration.rs`
- `git commit -m "feat: implement x11 event backend"`

---

### Task 3: Implement X11 read/write + INCR and clipboard-manager interop

**Files**
- Modify: `core/ccvv-linux/src/backend/x11.rs`
- Modify: `core/ccvv-linux/src/detection.rs`
- Modify: `core/ccvv-linux/src/app.rs`

**Step 1: Write failing tests**
- Add tests for:
  - INCR receive/send path
  - `CLIPBOARD_MANAGER`/`SAVE_TARGETS` handshake acceptance
- Add `run` integration assertions for no infinite loop on own write.

**Step 2: Run tests**
- `cargo test -p ccvv-linux x11_integration -- --nocapture`
Expected: FAIL.

**Step 3: Implement**
- Add incremental transfer handling with size safety and `read_complete` semantics.
- Add clipboard-manager target handling.
- Harden self-write suppression using owner-window checks first, timestamp as secondary guard.
- Respect `PRIMARY` as optional only and never trigger automatic clean from PRIMARY unless explicitly configured.

**Step 4: Run checks**
- `cargo test -p ccvv-linux x11_integration -- --nocapture`

**Step 5: Commit**
- `git add core/ccvv-linux/src/backend/x11.rs core/ccvv-linux/src/detection.rs core/ccvv-linux/src/app.rs`
- `git commit -m "feat: add x11 transfer and clipboard-manager handling"`

---

### Task 4: Add Wayland backend protocol probing and seat-aware event handling

**Files**
- Rewrite: `core/ccvv-linux/src/backend/wayland.rs`
- Add: `core/ccvv-linux/src/backend/seat_state.rs` (or equivalent helper module)
- Add: `core/ccvv-linux/tests/wayland_integration.rs`
- Modify: `core/ccvv-linux/Cargo.toml`

**Step 1: Write failing tests**
- Test seat enumeration (mock/adapter-level tests if direct protocol mocking is used).
- Test independent state isolation for `seat0/seat1`.
- Test fallback when only `wlr-data-control-unstable-v1` is exposed.

**Step 2: Run tests**
- `cargo test -p ccvv-linux wayland -- --nocapture`

**Step 3: Implement**
- Probe `ext-data-control-v1` first, then `wlr-data-control-unstable-v1`.
- Bind every advertised `wl_seat`, create per-seat devices and streams.
- Read/write text targets through protocol offers only.
- Populate snapshot with per-seat identifiers.

**Step 4: Run checks**
- `cargo test -p ccvv-linux wayland -- --nocapture`
Expected: PASS in supported compositor dev environments; document OS-limited.

**Step 5: Commit**
- `git add core/ccvv-linux/src/backend/wayland.rs core/ccvv-linux/src/backend/seat_state.rs core/ccvv-linux/tests/wayland_integration.rs core/ccvv-linux/Cargo.toml`
- `git commit -m "feat: implement wayland seat-aware backend"`

---

### Task 5: Implement GNOME/Mutter limited mode with explicit portal clean action

**Files**
- Add: `core/ccvv-linux/src/clipboard/gnome.rs`
- Add: `core/ccvv-linux/src/hotkey/portal.rs`
- Modify: `core/ccvv-linux/src/app.rs`
- Modify: `core/ccvv-linux/src/backend/wayland.rs`
- Modify: `core/ccvv-linux/src/cli`/`main.rs` (if backend override flags needed)

**Step 1: Write failing tests**
- Tests for:
  - limited mode startup when no native Wayland clipboard manager available.
  - one-shot portal command routes directly to `handle_snapshot` through `CleanNow`.

**Step 2: Run tests**
- `cargo test -p ccvv-linux gnome -- --nocapture`

**Step 3: Implement**
- Add runtime capability probe:
  - if no automatic manager, transition backend to `Limited`.
  - if `org.freedesktop.portal.GlobalShortcuts` unavailable, disable explicit path clean.
- Register explicit hotkey command path (single-action clean, no double-tap semantics).
- Keep CLI/manual status clean actions working against limited backend.

**Step 4: Run checks**
- `cargo test -p ccvv-linux gnome -- --nocapture`

**Step 5: Commit**
- `git add core/ccvv-linux/src/clipboard/gnome.rs core/ccvv-linux/src/hotkey/portal.rs core/ccvv-linux/src/app.rs core/ccvv-linux/src/backend/wayland.rs`
- `git commit -m "feat: add wayland limited mode and portal hotkey path"`

---

### Task 6: Add backend-aware status and control behavior

**Files**
- Modify: `core/ccvv-linux/src/app.rs`
- Modify: `core/ccvv-linux/src/control/socket.rs`
- Modify: `core/ccvv-linux/src/ui_protocol.rs`

**Step 1: Write failing tests**
- Add tests that validate:
  - `CleanNow` is rejected only when clipboard automation unavailable and no portal path exists.
  - status snapshot includes capability and backend mode transitions.
  - `LastCleanSucceeded` updates for failed `write_plain_text` in all backends.

**Step 2: Run tests**
- `cargo test -p ccvv-linux control -- --nocapture`

**Step 3: Implement**
- Expand status model if needed with explicit disabled reasons (optional).
- Ensure capability gating is authoritative before `CleanNow`.
- Ensure pause/resume/quit behavior remains socket-only, no raw clipboard payload exposure.

**Step 4: Run checks**
- `cargo test -p ccvv-linux control -- --nocapture`

**Step 5: Commit**
- `git add core/ccvv-linux/src/app.rs core/ccvv-linux/src/control/socket.rs core/ccvv-linux/src/ui_protocol.rs`
- `git commit -m "feat: align runtime status and control semantics with capability model"`

---

### Task 7: Upgrade tray sidecar UI contract and menu actions

**Files**
- Modify: `core/ccvv-linux/src/tray.rs`
- Modify: `core/ccvv-linux/src/bin/ccvv-tray-sni.rs`
- Modify: `core/ccvv-linux/src/bin/ccvv-indicator-legacy.rs`

**Step 1: Write failing tests**
- Add unit tests for:
  - icon/title mapping for limited + error + paused.
  - command actions requiring backend gating (`Clean Now` disabled in explicit unsupported state).
  - menu model includes `Restore Last Cleaned Item`, `Run Diagnostics`, backend label.

**Step 2: Run tests**
- `cargo test -p ccvv-linux tray -- --nocapture`

**Step 3: Implement**
- Add menu entries from spec:
  - `Restore Last Cleaned Item`
  - `Run Diagnostics`
  - `Open Config Directory`
- Wire menu action states from runtime `BackendCapability`.
- For `ccvv-indicator-legacy`, preserve current socket protocol and add optional no-op placeholder for missing features if GTK mode is constrained.

**Step 4: Run checks**
- `cargo test -p ccvv-linux tray -- --nocapture`

**Step 5: Commit**
- `git add core/ccvv-linux/src/tray.rs core/ccvv-linux/src/bin/ccvv-tray-sni.rs core/ccvv-linux/src/bin/ccvv-indicator-legacy.rs`
- `git commit -m "feat: align tray state/actions with backend capability"`

---

### Task 8: Add integration test structure and execution harness

**Files**
- Add: `core/ccvv-linux/tests/integration_mod.rs`
- Add: `core/ccvv-linux/tests/x11_integration.rs`
- Add: `core/ccvv-linux/tests/wayland_integration.rs`
- Add: `core/ccvv-linux/tests/gnome_limited_integration.rs`

**Step 1: Write failing tests**
- Add placeholder tests that clearly skip when compositor tools are missing (to keep local runs green).
- Add failure-mode tests for:
  - no tray host
  - foreign socket listener
  - session D-Bus missing
  - mixed-content fallback path.

**Step 2: Run tests**
- `cargo test -p ccvv-linux --test x11_integration -- --ignored` (or equivalent skip gating)

**Step 3: Implement**
- Add helper harness for X11 and Wayland session setup, clipboard writers, and state assertions.
- Keep strict headless assumptions in tests and separate CI-only jobs for compositor execution.

**Step 4: Run checks**
- `cargo test -p ccvv-linux --tests`

**Step 5: Commit**
- `git add core/ccvv-linux/tests`
- `git commit -m "test: add linux compositor integration harness and placeholders"`

---

### Task 9: Add packaging and autostart artifacts

**Files**
- Add: `packaging/linux/ccvv.desktop`
- Add: `packaging/linux/systemd-user/ccvv.service`
- Add: packaging specs for `.deb`, `.rpm`, `PKGBUILD`, and tarball helpers
- Modify: `core/Cargo.toml`, optional: root release docs

**Step 1: Write failing validation**
- Add scripts/tests that assert:
  - `.desktop` has `X-GNOME-Autostart-enabled=true`, `NoDisplay=true`
  - systemd unit is disabled by package defaults
  - optional xembed helper is separate.

**Step 2: Run checks**
- `cargo test -p ccvv-linux --features tray` (sanity after packaging metadata changes).

**Step 3: Implement**
- Add metadata and install layout.
- Add `ccvv-linux`, `ccvv-tray-sni`, and optional `ccvv-indicator-legacy` targets in package manifests.
- Add optional `flaked` reproducible shell script for dev (not mandatory for v1 release gating).

**Step 4: Run checks**
- Run local containerized builds for each format (or at minimum lint/build smoke).

**Step 5: Commit**
- `git add packaging core/Cargo.toml core/deny.toml`
- `git commit -m "chore: add linux packaging and autostart artifacts"`

---

### Task 10: Expand CI for Linux-specific validation matrix

**Files**
- Add/modify: `.github/workflows/linux-desktop.yml` (new workflow)
- Modify: `.github/workflows/pr-checks.yml`
- Add optional shell helpers under `core/` or root scripts

**Step 1: Write failing checks**
- Add matrix jobs in CI config for:
  - unit suite
  - X11 integration
  - Sway integration
  - KWin and Hyprland smoke tests
  - GNOME limited-mode smoke
  - packaging builds
  - cargo-deny check.

**Step 2: Run checks**
- `cargo test -p ccvv-linux` locally.
- `./.github/workflows` YAML lint/syntax by `act` or CI dry-run where possible.

**Step 3: Implement**
- Keep compositor jobs optional/CI-only with environment bootstrapping.
- Route failing attribution to task-specific suites (`x11`, `wayland`, `legacy-tray`, `runtime`).

**Step 4: Validate**
- Push branch and run workflow.
- Confirm failures are isolated and actionable.

**Step 5: Commit**
- `git add .github/workflows`
- `git commit -m "ci: add linux daemon and compositor validation matrix"`

---

### Task 11: Update docs/specs and close remaining known limitations

**Files**
- Modify: `specs/technical_linux_v1.md`
- Modify: `README.md` or Linux install doc if present
- Add: `specs/reviews/` note for implemented-vs-deferred deltas

**Step 1: Write a reconciliation section**
- Document which behavior is explicitly limited:
  - mixed-content text-only overwrite
  - GNOME automatic limitation
  - session-owned clipboard durability.

**Step 2: Run checks**
- No code tests needed; run docs linting if present.

**Step 3: Implement**
- Add a “v1 known limitations” section in README/spec and a concise “what changed since last review” note.

**Step 4: Validate**
- Confirm file references and paths for docs are accurate.

**Step 5: Commit**
- `git add specs/technical_linux_v1.md README.md`
- `git commit -m "docs: sync linux spec with implemented behavior and limitations"`

---

## Suggested backlog (non-blocking, defer if needed)

1. Add `ccvv-linux` command to restore last cleaned item directly from history (and tray shortcut).
2. Add Nix flake for reproducible dev packaging.
3. Add optional TUI config/history viewer for Linux (not in v1 scope).
4. Add performance and long-copy fuzz tests around HTML extraction.
5. Add explicit docs for migration risk of process-owned clipboard ownership on Linux.

---

## Suggested execution mode

Plan complete and saved to `docs/plans/2026-03-09-linux-v1-remediation-plan.md`.

**Option A:** Subagent-driven in this session (recommended): dispatch a fresh subagent per task, review outputs, then continue.
**Option B:** Parallel execution in a separate session using `executing-plans`.
