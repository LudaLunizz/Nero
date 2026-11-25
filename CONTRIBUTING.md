# Contributing

Issues and pull requests are welcome. This file is the setup and the exact
commands; the [README](README.md) says what Nero does.

## Setup

| Path | What |
|---|---|
| `core/` | the Rust core: all placement logic, built to WebAssembly (`core/build.sh`); `core/js/nero.js` loads it in GNOME Shell |
| `keep-onscreen/` | standalone GNOME Shell extension (JS shell around the core's `fitToWorkArea`) |
| `tilingshell-patch/tilingshell-18.0.patch` | the patch set against Tiling Shell 18.0 |
| `tilingshell-patch/apply.sh` | downloads 18.0 (sha256-checked), patches into `build/`, `--install` copies it over your installed copy |
| `tests/` | logic tests through the JS wrappers into the wasm core |

Needs: `curl`, `unzip`, `patch`, `glib-compile-schemas`, `gjs`, `node` (syntax check only) and Rust
with `rustup target add wasm32-unknown-unknown`. Placement logic goes in `core/src/` (with a test
in `core/tests/`); the JS files only wire GNOME Shell to it.
To change the patch: edit files in `build/tilingshell@ferrarodomenico.com/`, then regenerate the patch with
`diff -ruN` against a fresh unzip of the 18.0 release (leave `metadata.json`, `schemas/gschemas.compiled`, `utils/nero.js` and `utils/nero_core.wasm` out).

## Before you open a pull request

These are what CI runs (`.github/workflows/`):

```sh
(cd core && cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked)
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
