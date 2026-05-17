# cargo-slim

`cargo-slim` is an early Rust CLI for explaining binary size and building conservative shrink plans.

The command is `cargoslim`.

## Current surface

This first version intentionally starts small:

- `cargoslim inspect <path>` reports the size of one binary or file.
- `cargoslim --help` shows the available command surface.

Planned work includes binary attribution, Cargo dependency and feature context, release-profile checks, and diff-based reporting. The goal is to explain size with evidence before suggesting changes.

## Install from source

```sh
cargo install --path .
```

## Usage

```sh
cargoslim inspect target/release/my-binary
```

Example output:

```text
path: target/release/my-binary
size: 4218880 bytes (4.02 MiB)
```

## Status

This repository is in the initial scaffold stage. The current implementation does not perform symbol, section, dependency, or feature attribution yet.
