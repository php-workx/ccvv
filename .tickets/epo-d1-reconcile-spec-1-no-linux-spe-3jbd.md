---
id: epo-d1-reconcile-spec-1-no-linux-spe-3jbd
title: 'D1: reconcile spec §1 ''no Linux-specific transform logic'' vs HTML extraction'
type: doc
status: open
parent: epo-linux-v1-spec-gap-audit-2026-05--gm5i
priority: 40
tags:
    - audit
    - linux-v1
    - docs
description: g3p review flagged this contradiction; partially fixed in the M3 implementation note but not in §1 itself.
intent: Spec §1 still claims no Linux-specific cleaning logic exists outside ccvv-lib; clipboard/html.rs is exactly that, and the H7 fix makes it load-bearing. Update the spec text.
acceptance_criteria:
    - Spec §1 paragraph clarifies that Stage 1 (rich-text extraction) is platform-specific; Stages 2-8 stay in ccvv-lib.
    - Spec mentions inline-code semantic hints (<code>/<kbd>/<samp>/<tt>) as part of the Linux extraction contract.
created: "2026-05-06T20:57:55Z"
extended_status: open
---
g3p review flagged this contradiction; partially fixed in the M3 implementation note but not in §1 itself.

## Acceptance criteria

- Spec §1 paragraph clarifies that Stage 1 (rich-text extraction) is platform-specific; Stages 2-8 stay in ccvv-lib.
- Spec mentions inline-code semantic hints (<code>/<kbd>/<samp>/<tt>) as part of the Linux extraction contract.
