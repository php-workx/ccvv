---
id: epo-m4-tray-success-flash-icon-state-lhhi
title: 'M4: tray success-flash icon state (§8.4)'
type: task
status: open
parent: epo-linux-v1-spec-gap-audit-2026-05--gm5i
priority: 50
tags:
    - audit
    - linux-v1
    - tray
description: Daemon already tracks last_clean_succeeded; sidecar just needs a transient state machine + icon mapping.
intent: Add the 300ms success flash icon state to ccvv-tray-sni; spec §8.4 lists 5 states; impl has 4.
acceptance_criteria:
    - Tray icon transitions to a success variant for ~300ms after handle_snapshot reports last_clean_succeeded=true.
    - Test asserts icon_for_status returns the success variant when a transient flag is set.
created: "2026-05-06T20:57:17Z"
extended_status: open
---
Daemon already tracks last_clean_succeeded; sidecar just needs a transient state machine + icon mapping.

## Acceptance criteria

- Tray icon transitions to a success variant for ~300ms after handle_snapshot reports last_clean_succeeded=true.
- Test asserts icon_for_status returns the success variant when a transient flag is set.
