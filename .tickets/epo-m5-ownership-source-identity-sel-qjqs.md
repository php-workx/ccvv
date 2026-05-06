---
id: epo-m5-ownership-source-identity-sel-qjqs
title: 'M5: ownership/source-identity self-write suppression (g3p §6.2.3 + §6.3.3)'
type: task
status: open
parent: epo-linux-v1-spec-gap-audit-2026-05--gm5i
priority: 60
tags:
    - audit
    - linux-v1
    - robustness
description: 'Today both backends rely on string equality between the last-written text and the next observed text. Brittle: rapid same-text user copies after a clean would be incorrectly suppressed; identical-text writes from a different app would also be suppressed once. Spec/g3p review explicitly call out ownership-first as the right primary.'
intent: Replace text-equality self-write suppression with ownership/identity-based checks so we can never re-trigger our own write.
acceptance_criteria:
    - 'X11: watch loop ignores XfixesSelectionNotify when the new owner window equals the daemon''s selection-owner window, regardless of timestamp or text.'
    - 'Wayland: track the data-source object identity; ignore selection events whose source matches our last write''s source.'
    - Existing text-based take_self_write_flag remains as a defense-in-depth secondary check, not the primary.
    - Tests cover same-text-different-source (must trigger) and same-source-same-text (must not trigger).
created: "2026-05-06T20:57:17Z"
extended_status: open
---
Today both backends rely on string equality between the last-written text and the next observed text. Brittle: rapid same-text user copies after a clean would be incorrectly suppressed; identical-text writes from a different app would also be suppressed once. Spec/g3p review explicitly call out ownership-first as the right primary.

## Acceptance criteria

- X11: watch loop ignores XfixesSelectionNotify when the new owner window equals the daemon's selection-owner window, regardless of timestamp or text.
- Wayland: track the data-source object identity; ignore selection events whose source matches our last write's source.
- Existing text-based take_self_write_flag remains as a defense-in-depth secondary check, not the primary.
- Tests cover same-text-different-source (must trigger) and same-source-same-text (must not trigger).
