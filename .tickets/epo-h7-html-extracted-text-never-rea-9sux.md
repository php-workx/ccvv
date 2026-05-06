---
id: epo-h7-html-extracted-text-never-rea-9sux
title: 'H7: HTML-extracted text never reaches the cleaning pipeline'
type: bug
status: open
parent: epo-linux-v1-spec-gap-audit-2026-05--gm5i
priority: 90
tags:
    - audit
    - linux-v1
    - critical
description: 'Trace: backend/x11.rs and backend/wayland.rs both populate acquired_html, but app.rs::handle_snapshot calls pipeline.run(&snapshot.acquired_plain_text) — the html field is never read. Result: M1''s inline-backtick code (<code>/<kbd>/<samp>/<tt>) is dead in production. Same story for any future HTML-aware semantic hints. Fix is ~15 lines in handle_snapshot plus a fixture-style test that drives a synthetic snapshot through the run path.'
intent: Wire HTML acquisition through to the transform pipeline so spec §6.5 actually runs in production.
acceptance_criteria:
    - When ClipboardSnapshot.acquired_html is Some(html), handle_snapshot extracts plain text via clipboard::html::extract_plain_text_from_html and feeds the result to the pipeline.
    - When extraction fails or the cap is exceeded, fall back to acquired_plain_text without surfacing an error.
    - End-to-end test asserts that an HTML snapshot containing <code>cargo build</code> ends up wrapped in inline backticks AFTER the pipeline runs.
created: "2026-05-06T20:56:33Z"
extended_status: open
---
Trace: backend/x11.rs and backend/wayland.rs both populate acquired_html, but app.rs::handle_snapshot calls pipeline.run(&snapshot.acquired_plain_text) — the html field is never read. Result: M1's inline-backtick code (<code>/<kbd>/<samp>/<tt>) is dead in production. Same story for any future HTML-aware semantic hints. Fix is ~15 lines in handle_snapshot plus a fixture-style test that drives a synthetic snapshot through the run path.

## Acceptance criteria

- When ClipboardSnapshot.acquired_html is Some(html), handle_snapshot extracts plain text via clipboard::html::extract_plain_text_from_html and feeds the result to the pipeline.
- When extraction fails or the cap is exceeded, fall back to acquired_plain_text without surfacing an error.
- End-to-end test asserts that an HTML snapshot containing <code>cargo build</code> ends up wrapped in inline backticks AFTER the pipeline runs.
