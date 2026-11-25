# Contributing

Issues and pull requests are welcome. This file is the setup and the exact
commands; the [README](README.md) says what Nero does.

## Setup

| Path | What |
|---|---|
| `keep-onscreen/` | standalone GNOME Shell extension (plain JS, no build) |
| `tilingshell-patch/tilingshell-18.0.patch` | the patch set against Tiling Shell 18.0 |
| `tilingshell-patch/apply.sh` | downloads 18.0 (sha256-checked), patches into `build/`, `--install` copies it over your installed copy |
| `tests/` | pure-logic tests for the patched modules |

Needs: `curl`, `unzip`, `patch`, `glib-compile-schemas`, `gjs` and `node`.
To change the patch: edit files in `build/tilingshell@ferrarodomenico.com/`, then regenerate the patch with
`diff -ruN` against a fresh unzip of the 18.0 release (leave `metadata.json` and `schemas/gschemas.compiled` out).

## Before you open a pull request

These are what CI runs (`.github/workflows/`):

```sh
./tilingshell-patch/apply.sh
./tests/run.sh
node --check keep-onscreen/keep-onscreen@LudaLunizz/extension.js
```

## Conventions

- **GitHub Actions are pinned to a full commit SHA** with the version in a
  comment; Dependabot proposes updates.

## Issues and pull requests

Open an issue first for anything beyond a typo, saying what you want and why.
Work on a branch named `<issue number>-short-name`, and start the pull request
description with `Closes #<number>`. Wait for CI to be green; the maintainer
merges with a rebase (linear history).
