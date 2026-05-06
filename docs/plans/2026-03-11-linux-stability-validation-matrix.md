# Linux Stability Validation Matrix

Use this matrix to record the manual evidence required by the Linux stability gate in [2026-03-11-linux-stability-hardening-plan.md](/Users/runger/workspaces/ccvv/docs/plans/2026-03-11-linux-stability-hardening-plan.md).

Rollout rule:

- Any failed row or untested high-risk scenario keeps Linux at `internal dogfood only`.
- Linux may be described as `technical beta` only when the required rows are complete and the CI gate is green.

## Run Metadata

| Field | Value |
|---|---|
| Build SHA | not yet executed against any tagged build |
| Validation date | pending |
| Tester | pending — must be a human on a real Linux desktop session |
| Notes | Automated regression suite + CI gate already green at 05d8afb (2026-05-06). This matrix is the *human* half of the gate and cannot be filled from CI: it requires copying real content (browser, terminal, IDE, PDF reader, mail client) on each compositor and recording the outcome row by row. |

## Environment Matrix

| Environment | Plain text copy | Rich text copy | Code block copy | Malformed/odd HTML | Clean-now behavior | Self-write behavior | Overall pass/fail | Notes |
|---|---|---|---|---|---|---|---|---|
| X11 | | | | | | | | |
| wlroots Wayland | | | | | | | | |
| GNOME Wayland limited mode | | | | | | | | |

## Per-Environment Details

### X11

- Environment:
- Build SHA / date:
- Result:
- Notes:

### wlroots Wayland

- Environment:
- Build SHA / date:
- Result:
- Notes:

### GNOME Wayland limited mode

- Environment:
- Build SHA / date:
- Result:
- Notes:

## Completion Criteria

- `Pass` means the scenario behaved as expected and no unexpected formatting/detection regression was observed.
- `Fail` means the scenario regressed, behaved inconsistently, or produced output that should block wider Linux rollout.
- `N/A` should be used sparingly and only with an explanatory note.
