# Linux Stability Validation Matrix

Use this matrix to record the manual evidence required by the Linux stability gate in [2026-03-11-linux-stability-hardening-plan.md](/Users/runger/workspaces/ccvv/docs/plans/2026-03-11-linux-stability-hardening-plan.md).

Rollout rule:

- Any failed row or untested high-risk scenario keeps Linux at `internal dogfood only`.
- Linux may be described as `technical beta` only when the required rows are complete and the CI gate is green.

## Run Metadata

| Field | Value |
|---|---|
| Build SHA | |
| Validation date | |
| Tester | |
| Notes | |

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
