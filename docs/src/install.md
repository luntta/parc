---
layout: layouts/doc.njk
title: Install
eyebrow: Getting started · §01
---

parc is written in Rust. Install with `cargo`.

## Requirements

- Rust 1.70 or newer
- A C compiler (for building bundled SQLite)
- Linux, macOS, or Windows

SQLite is bundled — no system dependencies required for the CLI or server.

## CLI

The core install. Gets you the `parc` command and everything you need to capture, search, and manage fragments.

```bash
cargo install --path parc-cli
```

## Optional binaries

```bash
# Standalone JSON-RPC server (also available as `parc server`)
cargo install --path parc-server

# Tauri desktop GUI
cargo install --path parc-gui
```

## Features

WASM plugin support is feature-gated so default builds carry zero `wasmtime` overhead.

```bash
# CLI with WASM plugin runtime
cargo install --path parc-cli --features wasm-plugins
```

Plugin manifest types and the `parc plugin list` / `parc plugin info` commands work without the feature — only runtime loading and execution require it.

## System dependencies for the GUI

The Tauri desktop GUI needs WebKit:

| OS | Package |
|----|---------|
| Arch Linux | `sudo pacman -S webkit2gtk-4.1` |
| Debian / Ubuntu | `sudo apt install libwebkit2gtk-4.1-dev` |
| macOS | Built in |

## Verifying the install

```bash
parc --version
parc doctor
```

`parc doctor` checks vault health if a vault exists in the current directory or parents; otherwise it just confirms the binary runs.
