---
id: epo-l1-swap-hand-rolled-html-parser--8slt
title: 'L1: swap hand-rolled HTML parser for html5ever (§11.1)'
type: task
status: open
parent: epo-linux-v1-spec-gap-audit-2026-05--gm5i
priority: 20
tags:
    - audit
    - linux-v1
    - deps
description: Spec §11.1 names html5ever as the parser; current impl is a custom state machine. Both work; reviewer will flag the divergence either way. Decision needed before swapping (adds dep weight).
intent: Match spec's named dependency by switching clipboard/html.rs to html5ever, or update the spec.
acceptance_criteria:
    - 'Either: html5ever wired in with the same MAX_HTML_BYTES cap and identical-or-better extraction parity for existing tests AND the new M1 inline-code tests, OR spec §11.1 is updated to acknowledge a hand-rolled parser is shipping by design.'
created: "2026-05-06T20:57:55Z"
extended_status: open
---
Spec §11.1 names html5ever as the parser; current impl is a custom state machine. Both work; reviewer will flag the divergence either way. Decision needed before swapping (adds dep weight).

## Acceptance criteria

- Either: html5ever wired in with the same MAX_HTML_BYTES cap and identical-or-better extraction parity for existing tests AND the new M1 inline-code tests, OR spec §11.1 is updated to acknowledge a hand-rolled parser is shipping by design.
