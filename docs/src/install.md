---
layout: layouts/doc.njk
title: Install
eyebrow: Getting started · §01
---

parc publishes prebuilt binaries on GitHub Releases. You can also install from a local checkout with `cargo`.

## Requirements

- Linux, macOS, or Windows
- Rust 1.70 or newer for source builds
- A C compiler for source builds

SQLite is bundled — no system SQLite dependency is required for the CLI or server.

## CLI

The core install. Gets you the `parc` command and everything you need to capture, search, and manage fragments.

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/luntta/parc/releases/latest/download/parc-cli-installer.sh | sh
```

On Windows, use the PowerShell installer published on the same release:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/luntta/parc/releases/latest/download/parc-cli-installer.ps1 | iex"
```

Release archives and installer scripts are published with `.sha256` checksums.

## From source

Use `cargo install` when working from a checkout:

```bash
cargo install --path parc-cli
```

## Optional server

The standalone JSON-RPC server is also released as `parc-server-installer.sh` and `parc-server-installer.ps1`.

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/luntta/parc/releases/latest/download/parc-server-installer.sh | sh
```

From source:

```bash
cargo install --path parc-server
```

## Terminal UI

The terminal UI is included in `parc-cli`:

```bash
parc tui
```

## Features

WASM plugin support is feature-gated so default builds carry zero `wasmtime` overhead.

```bash
# CLI with WASM plugin runtime
cargo install --path parc-cli --features wasm-plugins
```

Plugin manifest types and the `parc plugin list` / `parc plugin info` commands work without the feature — only runtime loading and execution require it.

## Verifying the install

```bash
parc --version
parc doctor
```

`parc doctor` checks vault health if a vault exists in the current directory or parents; otherwise it just confirms the binary runs.
