# Linux Stability Hardening Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Raise Linux quality from “feature-complete but churn-heavy” to “beta-safe for external technical users” by closing regression gaps in copy detection, content extraction, formatting, and compositor/runtime validation.

**Architecture:** Treat this as a stabilization stream, not a feature stream. The work should prioritize regression prevention, deterministic test coverage, and a release gate that blocks broad rollout unless copy flows remain correct across X11, wlroots Wayland, and GNOME limited mode. Runtime behavior stays in `core/ccvv-linux`, shared transform/content behavior stays in `core/ccvv-lib`, and release readiness is enforced in CI plus a short manual matrix.

**Tech Stack:** Rust, `ccvv-linux`, `ccvv-lib`, X11/XFixes, Wayland data-control, wlroots Docker harness, GitHub Actions.

---

## Baseline Audit

Verified on 2026-03-11:

- `find core/ccvv-linux/src -type f | wc -l` = `23`
- `find core/ccvv-linux/tests -type f | wc -l` = `4`
- `find core/ccvv-lib/tests -type f | wc -l` = `32`
- `rg -n "test_.*(html|clipboard|copy|sanitize|extract|target)" core/ccvv-linux/src core/ccvv-linux/tests core/ccvv-lib/tests | wc -l` = `13`

Implication:

- The implementation surface is already non-trivial.
- Test coverage exists, but regression-specific coverage for risky clipboard-content flows is still too small relative to the risk profile.
- Stability work should focus on a narrow set of copy/content/formatting paths instead of broad architectural change.

## Release Gate

Do **not** invite broad external users until all of these are true:

1. Regression suite covers the high-risk clipboard/content paths listed below.
2. Linux CI includes the Docker wlroots harness job and it stays green.
3. Manual matrix passes on at least:
   - X11
   - wlroots Wayland
   - GNOME Wayland limited mode
4. No new copy/content/formatting regression is found during a short soak period.

If any of those fail, treat the build as “internal dogfood only”.

Current rollout status recommendation:

- `technical beta for Linux power users`

## Release Gate Checklist

Track this checklist as the ship/no-ship gate for broader Linux rollout:

- [x] Regression suite complete for the high-risk clipboard/content scenarios in this plan.
  Evidence (audited 2026-05-06 @ 05d8afb): every scenario in “High-Risk Scenarios To Freeze With Tests” has at least one named regression test. Mapping:
  1. Browser rich-text copy with plain-text fallback —
     `core/ccvv-linux/src/clipboard/mod.rs::tests::test_html_is_preferred_when_extractable`,
     `…::test_plain_text_only_ignores_extractable_html`.
  2. Code block copy preserving spacing/newlines —
     `core/ccvv-linux/src/clipboard/html.rs::tests::test_code_and_pre_blocks_preserve_spacing`,
     `…::test_pre_block_keeps_spacing_between_surrounding_blocks`.
  3. HTML fragment → useful plain text —
     `core/ccvv-linux/src/clipboard/html.rs::tests::test_extracts_plain_text_from_simple_html`,
     `…::test_extracts_entities_and_block_boundaries`.
  4. Malformed HTML fails safe —
     `core/ccvv-linux/src/clipboard/html.rs::tests::test_malformed_html_is_rejected`,
     `core/ccvv-linux/src/clipboard/mod.rs::tests::test_malformed_html_falls_back_to_plain_text`,
     `…::test_malformed_html_without_plain_target_errors`.
  5. Oversized HTML predictable fallback —
     `core/ccvv-linux/src/clipboard/html.rs::tests::test_oversized_html_is_rejected`,
     `core/ccvv-linux/src/clipboard/mod.rs::tests::test_oversized_html_falls_back_to_plain_text`.
  6. Mixed target sets, best target picked —
     `core/ccvv-linux/src/clipboard/targets.rs::tests::test_prefers_utf8_plain_targets_first`,
     `…::test_prefers_text_plain_before_legacy_targets`,
     `…::test_returns_none_when_no_text_target_exists`.
  7. Self-originated clean/write does not re-trigger detection —
     `core/ccvv-linux/src/detection.rs::tests::test_self_write_is_ignored`,
     `…::test_self_write_does_not_arm_follow_up_trigger`,
     `core/ccvv-linux/src/backend/wayland.rs::tests::test_take_self_write_flag_consumes_matching_text`,
     `core/ccvv-linux/src/backend/x11.rs::tests::test_take_self_write_flag_consumes_matching_text_once`.
  8. Multi-seat timing/detection isolation —
     `core/ccvv-linux/src/detection.rs::tests::test_separate_seats_do_not_share_timing_state`.
  9. Wayland event-stream clipboard change delivery —
     `core/ccvv-linux/tests/wayland_integration.rs::wayland_event_stream_observes_clipboard_changes_in_harness`,
     `…::wayland_harness_snapshot_matches_last_written_text` (harness-gated, runs in CI under wlroots Docker).
  10. GNOME limited-mode explicit/manual behavior —
      `core/ccvv-linux/tests/gnome_limited_integration.rs::*`,
      `core/ccvv-linux/src/hotkey/portal.rs::tests::*`.
- [x] Linux CI green, including the Linux X11 integration slice, GNOME limited-mode slice, and `Linux Stability Gate - Wayland Harness`.
  Evidence: `.github/workflows/pr-checks.yml` defines all three jobs (`linux-x11-integration`, `linux-gnome-limited`, `linux-wayland-harness`); local full preflight `just dev` passed at 2026-05-06T19:30Z @ 05d8afb (workspace tests 359 passed, 3 ignored; clippy clean; betterleaks/shellcheck/semgrep/audit clean).
- [ ] Manual validation matrix complete for X11, wlroots Wayland, and GNOME Wayland limited mode.
  Evidence: `docs/plans/2026-03-11-linux-stability-validation-matrix.md`. **Requires human execution** — copying real text from a browser, terminal, IDE, etc. into a real GNOME / wlroots / X11 desktop session and recording outcomes in the matrix. Not automatable from CI.
- [ ] Soak complete: minimum 3 active internal days with no P1/P2 copy, detect, or formatting regression.
  Evidence: **Requires human dogfooding** for at least 3 calendar days on a real Linux session with a primary clipboard workload; record any regressions back into this plan and the validation matrix.

## Files to Modify

| File | Change |
|------|--------|
| `core/ccvv-linux/src/clipboard/html.rs` | Harden extraction/formatting edge cases and add regression cases if behavior is wrong |
| `core/ccvv-linux/src/clipboard/targets.rs` | Lock down target preference and fallback behavior |
| `core/ccvv-linux/src/detection.rs` | Verify self-write, duplicate-copy, and seat-isolation behavior against new regression cases |
| `core/ccvv-linux/src/backend/x11.rs` | Add/adjust correctness tests around snapshot semantics and clipboard ownership flows if needed |
| `core/ccvv-linux/src/backend/wayland.rs` | Add/adjust correctness tests around self-write, offer selection, and seat handling if needed |
| `core/ccvv-linux/src/app.rs` | Enforce any release-gate-facing status/reporting needed for beta readiness |
| `core/ccvv-linux/tests/x11_integration.rs` | Expand real-flow integration coverage |
| `core/ccvv-linux/tests/wayland_integration.rs` | Expand real-flow integration coverage in harness mode |
| `core/ccvv-linux/tests/gnome_limited_integration.rs` | Expand limited-mode coverage and explicit failure semantics |
| `core/ccvv-lib/tests/fixture_tests.rs` | Add content-fixture regressions for formatting and extraction |
| `core/ccvv-lib/tests/integration.rs` | Add end-to-end content pipeline regression cases where shared logic is responsible |
| `.github/workflows/pr-checks.yml` | Add/adjust stability jobs and release-gate checks |
| `README.md` | Add explicit beta status / supported-environment guidance if rollout remains limited |
| `docs/plans/2026-03-11-linux-stability-hardening-plan.md` | Keep plan status current during execution |

## High-Risk Scenarios To Freeze With Tests

These are the flows that must become regression tests before broad invite:

1. Browser rich-text copy with plain-text fallback.
2. Code block copy preserving expected spacing/newlines.
3. HTML fragment copy that should sanitize to useful plain text.
4. Malformed HTML that must fail safe without corrupting output.
5. Oversized HTML payload that must fall back predictably.
6. Mixed target sets where the best text target must be selected.
7. Self-originated clean/write that must not re-trigger detection.
8. Multi-seat timing/detection isolation.
9. Wayland event-stream clipboard change delivery.
10. GNOME limited-mode explicit/manual behavior.

## Task 1: Define the stability contract and beta gate

**Files:**
- Modify: `README.md`
- Modify: `.github/workflows/pr-checks.yml`
- Modify: `docs/plans/2026-03-11-linux-stability-hardening-plan.md`

**Step 1: Write the failing release-gate checklist**

Add a checklist section to this plan with explicit pass/fail criteria:

- regression suite complete
- CI green
- manual matrix complete
- soak complete

**Step 2: Add beta positioning to docs**

Document one of these states in `README.md`:

- `internal dogfood`
- `technical beta`
- `stable`

Current recommendation: `technical beta for Linux power users`.

**Step 3: Add CI gate language**

Update workflow naming/comments so the Wayland harness job is clearly part of the Linux stability gate.

**Step 4: Verify**

Run:

```bash
rg -n "technical beta|internal dogfood|Linux Wayland Harness" README.md .github/workflows/pr-checks.yml
```

Expected:

- README contains rollout status language
- workflow contains explicit Wayland harness stability job

## Task 2: Build the clipboard-content regression suite first

**Files:**
- Modify: `core/ccvv-linux/src/clipboard/html.rs`
- Modify: `core/ccvv-linux/src/clipboard/targets.rs`
- Test: `core/ccvv-lib/tests/fixture_tests.rs`
- Test: `core/ccvv-lib/tests/integration.rs`

**Step 1: Write failing regression tests for each risky content case**

Add named tests for:

- browser-rich-text fallback
- code-block whitespace preservation
- malformed HTML rejection
- oversized HTML fallback
- mixed-target preference

Use fixture-driven inputs where possible.

**Step 2: Run only the new regression tests and verify they fail for the right reason**

Run:

```bash
cargo test --locked -p ccvv-lib fixture -- --nocapture
```

Expected:

- new tests fail because behavior is missing or wrong, not because the test is broken

**Step 3: Implement the minimum behavior changes**

Only touch extractor/target-selection logic required by the failing tests.

**Step 4: Re-run targeted tests**

Run:

```bash
cargo test --locked -p ccvv-lib fixture -- --nocapture
cargo test --locked -p ccvv-lib integration -- --nocapture
```

Expected:

- regression tests pass

## Task 3: Freeze detector semantics against regressions

**Files:**
- Modify: `core/ccvv-linux/src/detection.rs`
- Test: `core/ccvv-linux/src/detection.rs`

**Step 1: Add failing detector tests**

Add named tests for:

- self-write is ignored
- repeated copy triggers clean only when intended
- seat A and seat B do not share state
- clipboard and primary selection do not double-count

**Step 2: Run the detector tests and verify failure if coverage was missing**

Run:

```bash
cargo test --locked -p ccvv-linux detection::tests -- --nocapture
```

**Step 3: Implement minimal fixes only if needed**

Do not refactor detector architecture unless a failing test proves it is necessary.

**Step 4: Re-run detector tests**

Run:

```bash
cargo test --locked -p ccvv-linux detection::tests -- --nocapture
```

## Task 4: Lock X11 clipboard behavior with integration coverage

**Files:**
- Modify: `core/ccvv-linux/src/backend/x11.rs`
- Modify: `core/ccvv-linux/tests/x11_integration.rs`

**Step 1: Add failing X11 integration tests**

Add or expand tests for:

- read without display returns error
- basic write/read round trip when display exists
- snapshot semantics after ownership change
- selection serving path for at least one consumer flow

**Step 2: Run the X11 slice**

Run:

```bash
cargo test --locked -p ccvv-linux x11 -- --nocapture
```

For compositor-backed runs:

```bash
xvfb-run -a cargo test --locked -p ccvv-linux x11 -- --nocapture
```

**Step 3: Fix only failing behavior**

Keep changes local to X11 event handling / ownership logic.

## Task 5: Lock Wayland clipboard behavior with harness-backed regression coverage

**Files:**
- Modify: `core/ccvv-linux/src/backend/wayland.rs`
- Modify: `core/ccvv-linux/tests/wayland_integration.rs`
- Modify if needed: `linux/docker/wayland-harness/*`

**Step 1: Add failing Wayland tests for runtime behavior**

Add named tests for:

- harness event stream observes clipboard change
- self-write flag is consumed once
- preferred text MIME selection is deterministic
- limited mode CLI helper path does not regress

**Step 2: Run local Wayland tests**

Run:

```bash
cargo test --locked -p ccvv-linux wayland -- --nocapture
```

**Step 3: Run harness-backed Wayland tests**

Run:

```bash
linux/docker/wayland-harness/run.sh sh -lc '
  cd /workspace/core
  export CARGO_TARGET_DIR=/tmp/ccvv-target
  export CARGO_HOME=/tmp/ccvv-cargo-home
  cargo test --locked -p ccvv-linux wayland -- --nocapture
'
```

Expected:

- harness build succeeds
- Wayland tests pass in a live headless wlroots session

## Task 6: Freeze GNOME limited-mode behavior

**Files:**
- Modify: `core/ccvv-linux/src/hotkey/portal.rs`
- Modify: `core/ccvv-linux/tests/gnome_limited_integration.rs`
- Modify if needed: `core/ccvv-linux/src/app.rs`

**Step 1: Add failing limited-mode tests**

Add named tests for:

- no desktop hint => unavailable or CLI-only as intended
- portal probe success => explicit hotkey path available
- waiting before register errors cleanly
- unsupported environment does not pretend to be automatic

**Step 2: Run the GNOME-limited slice**

Run:

```bash
cargo test --locked -p ccvv-linux gnome -- --nocapture
```

**Step 3: Tighten behavior only where tests show ambiguity**

Do not add new GNOME features here. Only stabilize capability reporting and explicit/manual semantics.

## Task 7: Create a manual validation matrix

**Files:**
- Create: `docs/plans/2026-03-11-linux-stability-validation-matrix.md`
- Modify: `README.md`

**Step 1: Write the validation matrix**

Create a matrix with rows for:

- X11
- wlroots Wayland
- GNOME Wayland limited mode

Columns:

- plain text copy
- rich text copy
- code block copy
- malformed/odd HTML
- clean-now behavior
- self-write behavior

**Step 2: Add result fields**

Each row should record:

- environment
- build SHA / date
- pass/fail
- notes

**Step 3: Verify**

Run:

```bash
test -f docs/plans/2026-03-11-linux-stability-validation-matrix.md
```

Execution status:

- Wave 1 doc template created; fill this matrix during manual validation.

## Task 8: Run a short soak and make a ship/no-ship call

**Files:**
- Modify: `docs/plans/2026-03-11-linux-stability-hardening-plan.md`
- Modify: `README.md`

**Step 1: Define soak window**

Minimum recommendation:

- 3 days of active internal use
- no P1/P2 regression in copy/detect/format flows

**Step 2: Record outcomes**

Track:

- new regressions found
- environments affected
- whether fix was test-covered

**Step 3: Make explicit status decision**

Allowed outputs:

- `internal dogfood only`
- `technical beta`
- `ready for wider invite`

Default until evidence says otherwise: `technical beta`.

## Verification

Run these before changing rollout status:

```bash
# Shared library regression coverage
cargo test --locked -p ccvv-lib fixture -- --nocapture
cargo test --locked -p ccvv-lib integration -- --nocapture

# Linux crate full test suite
cargo test --locked -p ccvv-linux

# Targeted backend slices
cargo test --locked -p ccvv-linux x11 -- --nocapture
cargo test --locked -p ccvv-linux wayland -- --nocapture
cargo test --locked -p ccvv-linux gnome -- --nocapture

# Headless wlroots validation
linux/docker/wayland-harness/run.sh sh -lc '
  cd /workspace/core
  export CARGO_TARGET_DIR=/tmp/ccvv-target
  export CARGO_HOME=/tmp/ccvv-cargo-home
  cargo test --locked -p ccvv-linux wayland -- --nocapture
'
```

## Exit Criteria

This plan is complete only when:

1. All high-risk scenarios above are covered by tests.
2. The local Linux test suite is green.
3. The Docker wlroots harness job is green in CI.
4. The manual validation matrix is filled for the target environments.
5. A written rollout status is present in `README.md`.
6. The team can point to evidence for “technical beta” or “ready for wider invite” instead of relying on intuition.
