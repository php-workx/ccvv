# AGENTS.md

`AGENTS.md` is the durable instruction file for this repo. Keep it short and
stable. Put ephemeral session notes elsewhere.

## What This Repo Is

`ccvv` is a clipboard sanitizer. Today the shipped product is still centered on
the macOS app in `mac/main.swift`, with Linux and Windows helper scripts. The
Rust workspace under `core/` exists for the longer-term transform engine and
CLI direction, but the current product surface is still the app and scripts you
can run today.

## Hard Rules

- Treat the current shipping app as the default reality. Do not steer simple
  maintenance tasks into speculative Rust migration work.
- Keep the macOS app flat unless the task explicitly asks for a structural
  refactor. `mac/main.swift` is intentionally a single-file app right now.
- Preserve behavior parity deliberately. If you change shared cleaning logic,
  consider the Linux and Windows implementations too.
- Keep network access at zero. If a change would introduce network behavior or a
  network dependency, stop and call it out.
- In Rust code, do not introduce `fancy-regex`; the repo relies on linear-time
  regex behavior.
- If you update a release version, keep `mac/main.swift` and `Casks/ccvv.rb` in
  sync.

## Common Commands

```bash
just build            # full macOS build
just build-mac        # rebuild app bundle without rebuilding Rust core
just build-core       # build Rust workspace crates
just test             # Rust workspace tests
just pre-commit       # fast local gate
just check-local      # broader local validation without Sonar
```

Useful direct runs:

```bash
open mac/build/ccvv.app
mac/build/ccvv
linux/ccvv
powershell -File windows/ccvv.ps1
```

## Workflows

### 1. Current macOS app changes

1. Start from `mac/main.swift` unless the task clearly belongs in `core/`.
2. Build with `just build-mac` or `just build`.
3. If behavior changed, verify the menu bar app still launches and that the
   manual clean path still works even without Accessibility permission.

### 2. Rust core changes

1. Keep Swift stage-1 rich-text extraction boundaries intact unless the task is
   specifically about that handoff.
2. Run `just build-core` and `just test`.
3. If the change affects shared transform behavior, verify the current app and
   CLI expectations still make sense.

### 3. Release-facing changes

1. If the task changes shipping behavior, check whether README, specs, or the
   Homebrew cask need an update too.
2. If the version changes, update both `mac/main.swift` and `Casks/ccvv.rb`.

## References

- `README.md` — current product surface and user-facing behavior
- `specs/functional_v1.md` — product rules and config schema
- `specs/technical_v1.md` — planned Rust architecture and security constraints
- `specs/technical_linux_v1.md` — Linux-specific runtime details
