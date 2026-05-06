---
id: epo-h4-xwayland-integration-test-ov2d
title: 'H4: XWayland integration test'
type: task
status: open
parent: epo-linux-v1-spec-gap-audit-2026-05--gm5i
priority: 65
tags:
    - audit
    - linux-v1
    - testing
description: Currently zero coverage for the XWayland bridge despite spec §6.4 + §13.4 calling it out as a required scenario.
intent: Verify the Wayland backend observes XWayland-bridged clipboard changes (spec §13.4).
acceptance_criteria:
    - Inside the existing Sway harness, run an X11 client that owns CLIPBOARD.
    - Assert the daemon's WaylandBackend (NOT a separately-attached X11Backend) observes the bridged change.
    - Assert no second X11Backend was started.
created: "2026-05-06T20:57:17Z"
extended_status: open
---
Currently zero coverage for the XWayland bridge despite spec §6.4 + §13.4 calling it out as a required scenario.

## Acceptance criteria

- Inside the existing Sway harness, run an X11 client that owns CLIPBOARD.
- Assert the daemon's WaylandBackend (NOT a separately-attached X11Backend) observes the bridged change.
- Assert no second X11Backend was started.
