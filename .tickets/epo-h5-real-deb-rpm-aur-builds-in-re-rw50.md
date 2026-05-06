---
id: epo-h5-real-deb-rpm-aur-builds-in-re-rw50
title: 'H5: real .deb / .rpm / AUR builds in release CI'
type: task
status: open
parent: epo-linux-v1-spec-gap-audit-2026-05--gm5i
priority: 75
tags:
    - audit
    - linux-v1
    - packaging
description: Spec §10.1 describes these as first-party shipping artifacts. Today release.yml only ships a tarball plus raw @@VERSION@@-rendered metadata files. External reviewer would flag this as an obvious miss.
intent: Build actual installable packages instead of shipping just the metadata templates (spec §10.1).
acceptance_criteria:
    - release.yml builds a .deb via dpkg-deb -b in an Ubuntu container; lintian smoke passes.
    - release.yml builds a .rpm via rpmbuild -bb in a Fedora container; rpmlint smoke passes.
    - AUR PKGBUILD validated via namcap or makepkg --printsrcinfo round-trip.
    - All three artifacts uploaded to the GitHub Release alongside the existing tarball.
created: "2026-05-06T20:57:17Z"
extended_status: open
---
Spec §10.1 describes these as first-party shipping artifacts. Today release.yml only ships a tarball plus raw @@VERSION@@-rendered metadata files. External reviewer would flag this as an obvious miss.

## Acceptance criteria

- release.yml builds a .deb via dpkg-deb -b in an Ubuntu container; lintian smoke passes.
- release.yml builds a .rpm via rpmbuild -bb in a Fedora container; rpmlint smoke passes.
- AUR PKGBUILD validated via namcap or makepkg --printsrcinfo round-trip.
- All three artifacts uploaded to the GitHub Release alongside the existing tarball.
