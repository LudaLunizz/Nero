# Security policy

## Reporting a vulnerability

Please report it **privately**, not in a public issue.

This repository is private, so the GitHub report form is not available: contact the maintainer, [LudaLunizz](https://github.com/LudaLunizz), directly.

Please include:

- what happens and why you think it is a security problem,
- the steps to reproduce it,
- the Nero version (the commit or release) and your system.

You will get an acknowledgement as soon as I see the report. For a verified problem I aim to have a fix released within about 90 days, and I will tell you if it will take longer. If you want to be credited in the release notes, say so.

## Supported versions

Only the latest release (or the latest `main`, while there is no release) gets security fixes.

## What is in scope

- Both parts run inside GNOME Shell with its full rights over every window, so a bug can affect the whole session.
- `apply.sh` downloads the upstream Tiling Shell 18.0 zip from GitHub and refuses it if the sha256 does not match; `--install` replaces files in your extensions folder (old copy kept).
- Nothing reads credentials or talks to the network at runtime.

## What is not a vulnerability by itself

- Windows being moved or resized: that is what it does.
- Problems in upstream Tiling Shell that the patch does not touch: report those to [domferr/tilingshell](https://github.com/domferr/tilingshell).
- An extension update from extensions.gnome.org overwriting the patch.

## Disclosure

Please give me the time above before you publish details. When a fix is released I will publish a GitHub security advisory that names the affected versions.
