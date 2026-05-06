---
id: epo-h1-real-x11-round-trip-integrati-vama
title: 'H1: real X11 round-trip integration tests under Xvfb'
type: task
status: open
parent: epo-linux-v1-spec-gap-audit-2026-05--gm5i
priority: 80
tags:
    - audit
    - linux-v1
    - testing
description: 'Files: tests/x11_integration.rs (rewrite). Currently 5 negative tests + 1 ignored round-trip — does not exercise any happy path. Fixing this is the single highest-confidence move for external review of the X11 backend.'
intent: Replace the negative-only x11_integration suite with the spec §13.2 scenarios (own CLIPBOARD with helper, double-copy inside/outside window, write-back ignored, INCR receive/send, clipboard-manager interop).
acceptance_criteria:
    - Test helper opens a second RustConnection, owns CLIPBOARD with a known payload, and the daemon-driven X11Backend can read it back round-trip.
    - 'Self-write suppression scenario: daemon writes, observes own change, asserts detector does NOT trigger.'
    - Identical-text double-copy inside the timing window triggers detection; outside the window does not.
    - 'INCR scenario: write a >256KiB payload, assert successful round-trip via INCR.'
    - All scenarios run under xvfb-run in CI without flakiness.
created: "2026-05-06T20:57:17Z"
extended_status: open
---
Files: tests/x11_integration.rs (rewrite). Currently 5 negative tests + 1 ignored round-trip — does not exercise any happy path. Fixing this is the single highest-confidence move for external review of the X11 backend.

## Acceptance criteria

- Test helper opens a second RustConnection, owns CLIPBOARD with a known payload, and the daemon-driven X11Backend can read it back round-trip.
- Self-write suppression scenario: daemon writes, observes own change, asserts detector does NOT trigger.
- Identical-text double-copy inside the timing window triggers detection; outside the window does not.
- INCR scenario: write a >256KiB payload, assert successful round-trip via INCR.
- All scenarios run under xvfb-run in CI without flakiness.
