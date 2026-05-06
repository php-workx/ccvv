---
id: epo-l2-ship-tray-icons-as-svg-assets-wrva
title: 'L2: ship tray icons as SVG assets (§10.6)'
type: task
status: open
parent: epo-linux-v1-spec-gap-audit-2026-05--gm5i
priority: 20
tags:
    - audit
    - linux-v1
    - packaging
    - design
description: Spec §8.4 + §10.6. Freedesktop names work on KDE/GNOME but break on minimal compositors.
intent: Ship symbolic monochrome SVG icons for the tray states; today we rely on freedesktop names.
acceptance_criteria:
    - linux/icons/ contains active/paused/limited/error (and future success) symbolic SVGs.
    - Makefile install target installs to ~/.local/share/icons/hicolor/scalable/apps/.
    - PKGBUILD/.deb/.rpm packaging templates include the icon files.
created: "2026-05-06T20:57:55Z"
extended_status: open
---
Spec §8.4 + §10.6. Freedesktop names work on KDE/GNOME but break on minimal compositors.

## Acceptance criteria

- linux/icons/ contains active/paused/limited/error (and future success) symbolic SVGs.
- Makefile install target installs to ~/.local/share/icons/hicolor/scalable/apps/.
- PKGBUILD/.deb/.rpm packaging templates include the icon files.
