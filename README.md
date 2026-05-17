# cargo-slim

`cargo-slim` is an early Rust CLI for explaining binary size and building conservative shrink plans.

The command is `cargoslim`.

## Current surface

This first version intentionally starts small:

- `cargoslim inspect <path>` reports file size, object format, architecture, debug-section presence, and section sizes when the file is a recognized object.
- `cargoslim inspect --json <path>` emits the same report as JSON.
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
object: Elf
architecture: X86_64
endianness: Little
entry: 0x7b20
debug symbols: no
sections:
  .text: 383104 bytes at 0x7b20
  .rodata: 65440 bytes at 0x65fa0
```

For scripts and snapshot tests:

```sh
cargoslim inspect --json target/release/my-binary
```

## Status

This repository is in the initial scaffold stage. The current implementation does not perform symbol, dependency, feature, or shrink-suggestion attribution yet.
