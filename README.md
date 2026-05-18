# cargo-slim

`cargo-slim` is an early Rust CLI for explaining binary size and building conservative shrink plans.

The command is `cargoslim`.

## Current surface

This first version intentionally starts small:

- `cargoslim inspect <path>` reports file size, object format, architecture, debug-section presence, and section sizes when the file is a recognized object.
- `cargoslim inspect --limit <n> <path>` limits section output and reports how many sections were omitted.
- `cargoslim inspect --manifest-path Cargo.toml <path>` adds Cargo package, workspace, lockfile, direct dependency, explicit release-profile facts, and conservative suggestions.
- `cargoslim inspect --json <path>` emits the report as JSON, using exact byte counts for sizes.
- `cargoslim diff <old> <new>` compares file size and object section sizes between two binaries.
- `cargoslim diff --json --limit <n> <old> <new>` emits exact byte deltas and the largest section deltas as JSON.
- `cargoslim --help` shows the available command surface.

Current suggestions are intentionally narrow. They come from concrete release-profile settings, duplicate package versions in `Cargo.lock`, and direct dependency declarations where default-feature behavior is visible. Planned work includes binary attribution and deeper Cargo dependency and feature context. The goal is to explain size with evidence before suggesting changes.

## Install from source

```sh
cargo install --path .
```

## Usage

```sh
cargoslim inspect target/release/my-binary
```

Limit section output when a binary has many sections:

```sh
cargoslim inspect --limit 10 target/release/my-binary
```

Include Cargo project context when inspecting a binary:

```sh
cargoslim inspect --manifest-path Cargo.toml target/release/my-binary
```

Compare two binaries:

```sh
cargoslim diff target/release/my-binary.old target/release/my-binary
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
cargo:
  manifest: /path/to/project/Cargo.toml
  package root: /path/to/project
  workspace root: /path/to/project
  package: my-binary 0.1.0 (edition 2021)
  lockfile: /path/to/project/Cargo.lock (32 packages)
  release profile: /path/to/project/Cargo.toml
    strip: symbols
    debug: false
    lto: thin
    codegen-units: 1
    panic: abort
    opt-level: z
```

When `inspect` sees concrete opportunities, it prints ranked suggestions:

```text
suggestions:
  1. Strip symbols from release binaries
     confidence: medium
     evidence: [profile.release].strip is not set.
     tradeoff: Stripping symbols makes ad hoc debugging harder unless symbols are preserved separately.
     action: Set [profile.release] strip = "symbols" or strip = true, then inspect the resulting binary.
```

For scripts and snapshot tests:

```sh
cargoslim inspect --json --limit 10 target/release/my-binary
cargoslim diff --json --limit 10 target/release/my-binary.old target/release/my-binary
```

## Status

This repository is in the initial scaffold stage. The current implementation does not perform symbol or crate-level size attribution yet.
