# Contributing

Issues and pull requests are welcome. This file is the setup and the exact
commands; the [README](README.md) says what gnome-window-fit does.

## Setup

TODO(secure-tree): table of the main paths and what they are, plus what is needed to build.

## Before you open a pull request

These are what CI runs (`.github/workflows/`):

```sh
TODO(secure-tree): the exact CI commands
```

## Conventions

- **GitHub Actions are pinned to a full commit SHA** with the version in a
  comment; Dependabot proposes updates.

## Issues and pull requests

Open an issue first for anything beyond a typo, saying what you want and why.
Work on a branch named `<issue number>-short-name`, and start the pull request
description with `Closes #<number>`. Wait for CI to be green; the maintainer
merges with a rebase (linear history).
