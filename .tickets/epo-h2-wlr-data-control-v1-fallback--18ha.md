---
id: epo-h2-wlr-data-control-v1-fallback--18ha
title: 'H2: wlr-data-control-v1 fallback path covered in CI'
type: task
status: open
parent: epo-linux-v1-spec-gap-audit-2026-05--gm5i
priority: 75
tags:
    - audit
    - linux-v1
    - testing
description: Spec §6.3.1 + §13.3 require BOTH ext-data-control-v1 and zwlr-data-control-unstable-v1 to be exercised. Current Sway harness exposes only ext-data-control. Fallback is implemented but unverified end-to-end.
intent: Add a CI lane that exercises the legacy zwlr-data-control-unstable-v1 protocol path so the fallback is provably alive.
acceptance_criteria:
    - Either a second wlroots harness image with only zwlr-data-control advertised, or an env-driven test mode that forces the fallback selection.
    - Test asserts WaylandBackend::probe_support reports Automatic via WaylandProtocol::WlrDataControl on that image.
    - Round-trip clipboard read/write succeeds against the wlr path.
created: "2026-05-06T20:57:17Z"
extended_status: open
---
Spec §6.3.1 + §13.3 require BOTH ext-data-control-v1 and zwlr-data-control-unstable-v1 to be exercised. Current Sway harness exposes only ext-data-control. Fallback is implemented but unverified end-to-end.

## Acceptance criteria

- Either a second wlroots harness image with only zwlr-data-control advertised, or an env-driven test mode that forces the fallback selection.
- Test asserts WaylandBackend::probe_support reports Automatic via WaylandProtocol::WlrDataControl on that image.
- Round-trip clipboard read/write succeeds against the wlr path.
