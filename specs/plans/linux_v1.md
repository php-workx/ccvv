# Linux v1 Native Daemon Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build the native Linux desktop implementation in `core/ccvv-linux` with X11 automatic mode, Wayland automatic mode where supported, GNOME explicit mode, socket-connected tray sidecars, packaging assets, and validation coverage.

**Architecture:** Add a new Rust crate that owns the headless daemon, backend abstraction, detector state, runtime socket, and clipboard acquisition/write-back. Keep UI out of the daemon by implementing `ccvv-tray-sni` and `ccvv-indicator-legacy` as sidecars over the same local socket. Reuse `ccvv-lib` for config, pipeline, and history; do not fork transform behavior.

**Tech Stack:** Rust workspace, `ccvv-lib`, SQLite via `rusqlite`, X11 via `x11rb`, Wayland via `wayland-client` + generated protocol bindings, portal/D-Bus via `zbus` or `ashpd`, SNI sidecar via `ksni`, runtime socket via `interprocess` or `std::os::unix`.

---

## Important Interfaces

- Add workspace member `core/ccvv-linux`.
- Add binaries: `ccvv-linux`, `ccvv-tray-sni`, `ccvv-indicator-legacy`.
- Add local socket protocol with commands: `Pause`, `Resume`, `CleanNow`, `Quit`, `GetStatus`, `SubscribeStatus`.
- Keep `ccvv-lib` as the sole transform engine; only add thin helper APIs there if Linux integration cannot reuse current `Pipeline`, config, or history APIs directly.
- Intended save path: `specs/plans/linux_v1.md`.

## Implementation Tasks

### Task 1: Workspace and crate skeleton

**Files:**
- Modify: `core/Cargo.toml`
- Create: `core/ccvv-linux/Cargo.toml`
- Create: `core/ccvv-linux/src/main.rs`
- Create: `core/ccvv-linux/src/app.rs`

**Step 1: Add the workspace member**

Update the workspace members list to include `ccvv-linux`.

**Step 2: Add the new crate manifest**

Create `core/ccvv-linux/Cargo.toml` with binary targets for `ccvv-linux`, `ccvv-tray-sni`, and `ccvv-indicator-legacy`.

**Step 3: Add minimal daemon startup**

Create `main.rs` and `app.rs` with a minimal bootstrap path that can initialize and exit in `none` mode.

**Step 4: Run compile check**

Run: `cargo check -p ccvv-linux`
Expected: PASS

**Step 5: Commit**

Run:
```bash
git add core/Cargo.toml core/ccvv-linux
git commit -m "feat: scaffold linux daemon crate"
```

### Task 2: Runtime paths, bootstrap, and config/history wiring

**Files:**
- Modify: `core/ccvv-linux/src/app.rs`
- Create: `core/ccvv-linux/src/single_instance.rs`
- Create: `core/ccvv-linux/src/control/socket.rs`

**Step 1: Write failing tests for runtime path and singleton behavior**

Add tests covering runtime socket path resolution, stale socket cleanup, and second-instance detection.

**Step 2: Run tests to verify they fail**

Run: `cargo test -p ccvv-linux runtime:: -- --nocapture`
Expected: FAIL with missing modules or unimplemented behavior

**Step 3: Implement bootstrap and singleton wiring**

Resolve `~/.ccvv` and `$XDG_RUNTIME_DIR`, enforce `0600`/`0700` permissions where applicable, and initialize `ccvv-lib` config/history dependencies.

**Step 4: Re-run tests**

Run: `cargo test -p ccvv-linux runtime:: -- --nocapture`
Expected: PASS

**Step 5: Commit**

Run:
```bash
git add core/ccvv-linux/src/app.rs core/ccvv-linux/src/single_instance.rs core/ccvv-linux/src/control/socket.rs
git commit -m "feat: add linux runtime bootstrap and singleton socket"
```

### Task 3: Socket protocol and daemon state model

**Files:**
- Create: `core/ccvv-linux/src/ui_protocol.rs`
- Create: `core/ccvv-linux/src/control/mod.rs`
- Modify: `core/ccvv-linux/src/control/socket.rs`

**Step 1: Write failing protocol tests**

Cover command decoding, status snapshots, and one subscriber receiving updates.

**Step 2: Run tests to verify they fail**

Run: `cargo test -p ccvv-linux control:: -- --nocapture`
Expected: FAIL

**Step 3: Implement the protocol**

Define commands `Pause`, `Resume`, `CleanNow`, `Quit`, `GetStatus`, `SubscribeStatus`; ensure raw clipboard contents never cross the socket.

**Step 4: Re-run tests**

Run: `cargo test -p ccvv-linux control:: -- --nocapture`
Expected: PASS

**Step 5: Commit**

Run:
```bash
git add core/ccvv-linux/src/ui_protocol.rs core/ccvv-linux/src/control
git commit -m "feat: add linux control socket protocol"
```

### Task 4: Backend-neutral detector and `none` backend

**Files:**
- Create: `core/ccvv-linux/src/detection.rs`
- Create: `core/ccvv-linux/src/backend/mod.rs`
- Create: `core/ccvv-linux/src/backend/none.rs`

**Step 1: Write failing unit tests**

Cover same-hash timing windows, pause/resume state, and `none` backend limited-mode status.

**Step 2: Run tests to verify they fail**

Run: `cargo test -p ccvv-linux detection:: -- --nocapture`
Expected: FAIL

**Step 3: Implement detector state and backend trait**

Add Linux timing defaults, backend capability reporting, and two-phase history orchestration hooks.

**Step 4: Re-run tests**

Run: `cargo test -p ccvv-linux detection:: -- --nocapture`
Expected: PASS

**Step 5: Commit**

Run:
```bash
git add core/ccvv-linux/src/detection.rs core/ccvv-linux/src/backend
git commit -m "feat: add linux detector core and none backend"
```

### Task 5: HTML/plain-text acquisition module

**Files:**
- Create: `core/ccvv-linux/src/clipboard/mod.rs`
- Create: `core/ccvv-linux/src/clipboard/html.rs`
- Create: `core/ccvv-linux/src/clipboard/targets.rs`

**Step 1: Write failing unit tests**

Cover plain-text only, valid HTML, oversize HTML fallback, malformed HTML fallback, and code/pre extraction semantics.

**Step 2: Run tests to verify they fail**

Run: `cargo test -p ccvv-linux clipboard:: -- --nocapture`
Expected: FAIL

**Step 3: Implement acquisition order and HTML cap**

Prefer `text/html`, cap reads at `2 MiB`, extract plain text conservatively, and fall back to plain text when needed.

**Step 4: Re-run tests**

Run: `cargo test -p ccvv-linux clipboard:: -- --nocapture`
Expected: PASS

**Step 5: Commit**

Run:
```bash
git add core/ccvv-linux/src/clipboard
git commit -m "feat: add linux clipboard acquisition and html extraction"
```

### Task 6: X11 backend MVP

**Files:**
- Create: `core/ccvv-linux/src/backend/x11.rs`
- Modify: `core/ccvv-linux/src/clipboard/targets.rs`
- Create: `core/ccvv-linux/tests/x11_integration.rs`

**Step 1: Write failing unit and integration tests**

Cover XFixes subscription, ownership-state suppression, same-text double copy inside/outside the window, and text-only republish.

**Step 2: Run tests to verify they fail**

Run: `cargo test -p ccvv-linux x11 -- --nocapture`
Expected: FAIL

**Step 3: Implement X11 read/write behavior**

Use a hidden owner window, ignore all self-owned events, and keep `PRIMARY` out of automatic sanitization.

**Step 4: Re-run tests under Xvfb**

Run: `xvfb-run -a cargo test -p ccvv-linux x11 -- --nocapture`
Expected: PASS

**Step 5: Commit**

Run:
```bash
git add core/ccvv-linux/src/backend/x11.rs core/ccvv-linux/tests/x11_integration.rs
git commit -m "feat: implement linux x11 clipboard backend"
```

### Task 7: X11 durability and large-transfer interop

**Files:**
- Modify: `core/ccvv-linux/src/backend/x11.rs`
- Modify: `core/ccvv-linux/tests/x11_integration.rs`

**Step 1: Write failing tests for `INCR` and clipboard-manager interop**

Cover large transfers, `SAVE_TARGETS`, and degraded durability diagnostics without a clipboard manager.

**Step 2: Run tests to verify they fail**

Run: `xvfb-run -a cargo test -p ccvv-linux x11_integration -- --nocapture`
Expected: FAIL

**Step 3: Implement `INCR` and interop support**

Add incremental receive/send handling and `CLIPBOARD_MANAGER` cooperation.

**Step 4: Re-run tests**

Run: `xvfb-run -a cargo test -p ccvv-linux x11_integration -- --nocapture`
Expected: PASS

**Step 5: Commit**

Run:
```bash
git add core/ccvv-linux/src/backend/x11.rs core/ccvv-linux/tests/x11_integration.rs
git commit -m "feat: add x11 large-transfer and clipboard-manager interop"
```

### Task 8: Wayland automatic backend with `ext-data-control-v1`

**Files:**
- Create: `core/ccvv-linux/src/backend/wayland.rs`
- Create or modify: `core/ccvv-linux/build.rs`
- Create: `core/ccvv-linux/tests/wayland_integration.rs`

**Step 1: Write failing tests for seat discovery and per-seat state**

Cover multi-seat enumeration, same-seat timing isolation, and self-write suppression.

**Step 2: Run tests to verify they fail**

Run: `cargo test -p ccvv-linux wayland -- --nocapture`
Expected: FAIL

**Step 3: Implement `ext-data-control-v1` support**

Enumerate all `wl_seat` globals, bind a data-control device per seat, and track detector state per seat.

**Step 4: Re-run tests under a headless compositor**

Run: `cargo test -p ccvv-linux wayland -- --nocapture`
Expected: PASS locally where supported, otherwise PASS in CI headless Sway job

**Step 5: Commit**

Run:
```bash
git add core/ccvv-linux/src/backend/wayland.rs core/ccvv-linux/build.rs core/ccvv-linux/tests/wayland_integration.rs
git commit -m "feat: implement wayland ext-data-control backend"
```

### Task 9: Wayland fallback and GNOME limited mode

**Files:**
- Modify: `core/ccvv-linux/src/backend/wayland.rs`
- Create: `core/ccvv-linux/src/hotkey/portal.rs`
- Create: `core/ccvv-linux/src/clipboard/gnome.rs`

**Step 1: Write failing tests for fallback and limited mode**

Cover `wlr-data-control-unstable-v1`, unsupported compositor startup, and portal hotkey triggering immediate `CleanNow`.

**Step 2: Run tests to verify they fail**

Run: `cargo test -p ccvv-linux gnome -- --nocapture`
Expected: FAIL

**Step 3: Implement fallback and portal behavior**

Probe `ext-data-control-v1` first, then `wlr-data-control-unstable-v1`, and use a single-action portal hotkey in limited mode.

**Step 4: Re-run tests**

Run: `cargo test -p ccvv-linux gnome -- --nocapture`
Expected: PASS

**Step 5: Commit**

Run:
```bash
git add core/ccvv-linux/src/backend/wayland.rs core/ccvv-linux/src/hotkey/portal.rs core/ccvv-linux/src/clipboard/gnome.rs
git commit -m "feat: add wayland fallback and gnome limited mode"
```

### Task 10: Modern tray sidecar

**Files:**
- Create: `core/ccvv-linux/src/bin/ccvv-tray-sni.rs`
- Modify: `core/ccvv-linux/src/ui_protocol.rs`

**Step 1: Write failing tests for status-to-icon and menu action mapping**

Cover `Pause`, `Resume`, `CleanNow`, `Quit`, and backend/limited-state labels.

**Step 2: Run tests to verify they fail**

Run: `cargo test -p ccvv-linux tray -- --nocapture`
Expected: FAIL

**Step 3: Implement the SNI sidecar**

Use `ksni` or equivalent, connect to the local socket, mirror daemon state, and keep sidecar crashes isolated from the daemon.

**Step 4: Re-run tests and smoke build**

Run: `cargo test -p ccvv-linux tray -- --nocapture && cargo check -p ccvv-linux --features tray`
Expected: PASS

**Step 5: Commit**

Run:
```bash
git add core/ccvv-linux/src/bin/ccvv-tray-sni.rs core/ccvv-linux/src/ui_protocol.rs
git commit -m "feat: add linux sni tray sidecar"
```

### Task 11: Legacy tray helper

**Files:**
- Create: `core/ccvv-linux/src/bin/ccvv-indicator-legacy.rs`
- Modify: `core/ccvv-linux/Cargo.toml`

**Step 1: Write a failing feature-gated build check**

Ensure the helper is only present behind `xembed` and does not affect default builds.

**Step 2: Run the gated build to verify it fails**

Run: `cargo check -p ccvv-linux --features xembed`
Expected: FAIL

**Step 3: Implement the helper**

Connect it to the same socket protocol as the SNI sidecar and keep all GTK/AppIndicator dependencies out of default features.

**Step 4: Re-run the gated build**

Run: `cargo check -p ccvv-linux --features xembed`
Expected: PASS

**Step 5: Commit**

Run:
```bash
git add core/ccvv-linux/src/bin/ccvv-indicator-legacy.rs core/ccvv-linux/Cargo.toml
git commit -m "feat: add optional legacy tray helper"
```

### Task 12: Packaging, autostart, and distro metadata

**Files:**
- Modify: `core/Cargo.toml`
- Modify: `core/deny.toml`
- Create: packaging assets and metadata under Linux packaging paths used by the repo

**Step 1: Write packaging validation checks**

Cover XDG autostart presence, systemd user unit non-default policy, and artifact metadata for `.deb`, `.rpm`, AUR, tarball, and optional `flake.nix`.

**Step 2: Run validation checks to verify they fail**

Run: packaging validation command or CI job locally where available
Expected: FAIL

**Step 3: Add packaging assets**

Make XDG autostart the default launch path; keep the systemd user service optional and disabled by default.

**Step 4: Re-run validation**

Run: packaging validation command or CI job locally where available
Expected: PASS

**Step 5: Commit**

Run:
```bash
git add core/Cargo.toml core/deny.toml packaging specs/technical_linux_v1.md
git commit -m "feat: add linux packaging and autostart assets"
```

### Task 13: CI and integration matrix

**Files:**
- Create or modify: CI workflow files
- Modify: `core/ccvv-linux/tests/x11_integration.rs`
- Modify: `core/ccvv-linux/tests/wayland_integration.rs`

**Step 1: Add failing CI coverage**

Introduce jobs for unit tests, X11/Xvfb, Wayland/headless compositor paths, GNOME limited-mode smoke tests, sidecar smoke tests, optional legacy-helper build, and `cargo deny`.

**Step 2: Run the local subset**

Run: `cargo test -p ccvv-lib && cargo test -p ccvv-cli && cargo test -p ccvv-linux`
Expected: PASS locally for unit scope; compositor/package jobs may remain CI-only

**Step 3: Wire CI jobs**

Add the staged Linux matrix and ensure failures are easy to attribute by subsystem.

**Step 4: Validate CI config**

Run: CI linter or dry-run validation where available
Expected: PASS

**Step 5: Commit**

Run:
```bash
git add .github core/ccvv-linux/tests
git commit -m "ci: add linux daemon validation matrix"
```

### Task 14: End-to-end hardening and docs sync

**Files:**
- Modify: `specs/technical_linux_v1.md`
- Modify: Linux install or README docs if they exist

**Step 1: Compare implementation against the spec**

List all remaining mismatches: mixed-content overwrite behavior, no-tray behavior, GNOME limitations, and optional Nix support.

**Step 2: Fix or document each mismatch**

Prefer code fixes for real behavior gaps; document only deliberate product limitations.

**Step 3: Re-run full validation**

Run: `cargo test -p ccvv-lib && cargo test -p ccvv-cli && cargo test -p ccvv-linux`
Expected: PASS, plus CI matrix green

**Step 4: Commit**

Run:
```bash
git add specs docs
git commit -m "docs: align linux implementation with technical spec"
```

## Test Plan

- Unit tests first for detector state, socket protocol, HTML extraction, and history/two-phase commit behavior.
- X11 integration next under `Xvfb`, including `INCR`, self-write suppression, and clipboard-manager interop.
- Wayland integration after that, split between automatic data-control mode and GNOME limited/manual mode.
- Sidecar tests stay protocol-focused; they do not own clipboard logic.
- Full regression gate: `cargo test -p ccvv-lib`, `cargo test -p ccvv-cli`, `cargo test -p ccvv-linux`, then compositor/package jobs.

## Assumptions and defaults

- Prefer zero `ccvv-lib` semantic changes; only add thin helper APIs if the Linux daemon would otherwise duplicate core orchestration.
- Defer performance tuning, full GUI preferences/history UI, and raw hand-rolled `dbusmenu` work until after the daemon and modern sidecar path are stable.
- Keep implementation order strict: daemon/control plane first, X11 second, Wayland automatic third, GNOME limited mode fourth, packaging/legacy tray last.

Plan complete and saved to `specs/plans/linux_v1.md`. Two execution options:

1. **Subagent-Driven (this session)** - I dispatch fresh subagent per task, review between tasks, fast iteration
2. **Parallel Session (separate)** - Open new session with executing-plans, batch execution with checkpoints
