# Prompt to Review the Linux Technical Specification

You are reviewing the Linux technical specification for ccvv:

- Primary target: `specs/technical_linux_v1.md`
- Supporting references:
  - `specs/technical_v1.md`
  - `specs/functional_v1.md`
  - `specs/technical_linux_v1_prompt.md`
  - relevant repository state under `core/`, `mac/`, and packaging/workflow files if present

This is a **combined adversarial review**. You must review the Linux spec in two ways at the same time:

1. **As a standalone RFC / technical specification**
   - Is it internally consistent?
   - Is it technically rigorous?
   - Does it clearly define behavior, constraints, invariants, failure modes, and non-goals?
   - Does it make defensible tradeoffs?

2. **As a spec for this actual repository and current Linux platform reality**
   - Does it align with the existing ccvv architecture and constraints?
   - Does it preserve the shared-engine model (`ccvv-lib` owns all transform logic)?
   - Does it remain consistent with the existing macOS/core technical and functional specs where it should?
   - Are the Linux/X11/Wayland/compositor claims grounded in what actually works today?
   - Are packaging, lifecycle, permissions, dependency, and test assumptions realistic for the listed target distros?

Your review should be **rigid, skeptical, and technically brutal**. Do not be charitable. Look for:

- incorrect protocol assumptions
- underspecified invariants
- contradictions
- lifecycle holes
- packaging mistakes
- dependency inaccuracies
- stale compositor claims
- unsupported or hand-wavy portal assumptions
- security model gaps
- crash-consistency/data-loss windows
- places where the spec drifts away from repo reality
- places where implementation teams would make incompatible choices because the spec is ambiguous

## Context

ccvv is a clipboard sanitizer. Today it consists of:

- a Rust core library: `core/ccvv-lib`
- a Rust CLI: `core/ccvv-cli`
- a macOS menu bar app in Swift/AppKit

Important existing constraints:

- `ccvv-lib` owns **all** text transformation logic
- Linux support must be a thin platform shell over the shared Rust core
- No network access, ever
- Clipboard content is untrusted input
- No fancy-regex
- No GTK or Qt runtime dependency for the **core daemon**
- Linux target distros: Ubuntu 22.04+, Debian 12+, Fedora 39+, Arch Linux
- Linux target display servers: X11 and Wayland

## Your Job

Review `specs/technical_linux_v1.md` for:

### A. RFC Quality

- Is the spec complete enough to implement without guesswork?
- Are all key decisions justified?
- Are invariants explicit where they need to be?
- Are failure modes, degraded modes, and non-goals clearly defined?
- Are shipping vs deferred claims clearly separated?
- Are the decision tables actually actionable?
- Are the testing and CI sections sufficient for the promises made elsewhere?

### B. Repository Alignment

Compare the Linux spec against:

- `specs/technical_v1.md`
- `specs/functional_v1.md`
- current workspace layout under `core/`
- existing architectural boundaries between `ccvv-lib`, `ccvv-cli`, and platform shells

Check especially for:

- violation of shared-engine ownership
- divergence from existing config/history/security assumptions
- loss of crash-consistency guarantees already present elsewhere
- platform-specific behavior that should instead live in `ccvv-lib`
- repo structure proposals that don’t match how this codebase is organized

### C. Linux Platform Reality

Audit every substantive Linux/platform claim for realism, including:

- X11 selection ownership and `XFixes` behavior
- ICCCM requirements
- `INCR` handling
- `CLIPBOARD_MANAGER` / `SAVE_TARGETS`
- `CLIPBOARD` vs `PRIMARY`
- Wayland data-control protocols
- seat semantics
- compositor-specific claims:
  - GNOME/Mutter
  - KDE/KWin
  - Sway / wlroots
  - Hyprland
- XWayland behavior
- D-Bus tray behavior
- `StatusNotifierItem`
- `XEmbed`
- `GlobalShortcuts` and any portal assumptions
- autostart and systemd user service assumptions
- distro packaging assumptions
- dependency/build assumptions

If a claim is plausible but under-specified, call that out. If a claim is wrong or stale, call that out directly.

### D. Security / Reliability / Data Integrity

Assume a hostile reviewer mindset. Look for:

- ways clipboard contents can be lost
- ways history can become inconsistent
- ways the daemon can accidentally process its own writes
- ways same-session clients can abuse control surfaces
- permissions that are broader than necessary
- sandbox/portal claims that are technically inaccurate
- file-permission or autostart behavior that is unsafe or not packageable
- hidden persistence assumptions on X11 or Wayland

### E. Testability / Release Risk

Evaluate whether the promised support matrix is actually test-covered.

Look for:

- supported environments with no explicit validation path
- feature flags or optional binaries that can silently rot
- missing compositor-specific jobs
- missing package/install validation
- assumptions that require real desktop sessions but are only covered by container builds

## Review Method

You must not just summarize the doc. You must hunt for flaws.

For each issue, ask:

1. Is this technically correct?
2. Is this fully specified?
3. Is this consistent with the rest of the document?
4. Is this consistent with the rest of the repo?
5. Would two competent implementers build the same thing from this text?
6. Would this still work under crash, restart, multi-process, or multi-seat conditions?
7. Is this supported on the claimed desktop/compositor/distro today?

## Output Format

Output **findings first**, ordered by severity.

For each finding, use this format:

- **Severity**: `Critical`, `High`, `Medium`, or `Low`
- **Location**: exact file and line reference(s)
- **Issue**: one-sentence description
- **Why it matters**: concrete technical consequence
- **What is missing or wrong**: precise explanation
- **Suggested correction**: what the spec should say instead

Then provide:

### Open Questions

Only include questions that materially block a correct implementation.

### Overall Assessment

Answer these explicitly:

1. Is this spec implementable as written?
2. Would you approve it for implementation?
3. What are the top 3 risks most likely to produce a failed or misleading implementation?

## Severity Rubric

- **Critical**: likely to cause data loss, incorrect shipped behavior, or a fundamentally wrong architecture
- **High**: major unsupported claim, security gap, lifecycle hole, or implementation trap
- **Medium**: ambiguity, stale platform claim, weak validation, or important missing detail
- **Low**: polish, clarity, naming, or secondary completeness issue

## Expectations

- Be terse but precise.
- Do not praise the document.
- Do not soften findings.
- Do not assume intent where the text is ambiguous.
- If the spec is wrong, say it is wrong.
- If a section is underspecified, say exactly what implementation-critical detail is missing.
- Prefer primary-source or official-platform reasoning where possible.
- If a platform detail is current-state-sensitive, make that explicit.

## Specific Areas to Pressure-Test

You should be especially suspicious of:

- GNOME/Mutter support claims
- portal usage claims
- X11 ownership/write-back semantics
- crash consistency between clipboard publish and history commit
- runtime control surfaces
- autostart/systemd packaging assumptions
- differences between “double copy gesture” and “double clipboard write”
- any Linux platform-layer behavior that looks like it belongs in `ccvv-lib`
- any release promise that is not matched by CI/release validation

## Deliverable

A findings-first review of `specs/technical_linux_v1.md` that is strong enough to block a weak spec from moving into implementation.
