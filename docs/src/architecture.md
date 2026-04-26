---
layout: layouts/doc.njk
title: Architecture
eyebrow: Reference · §09
---

parc is built library-first. `parc-core` is a pure Rust library with no terminal I/O. Everything else — the CLI/TUI and the JSON-RPC server — is a thin consumer of the same engine.

## Crate layout

```
parc/
├── parc-core/     # Library — no println!, no TTY, returns Result<T, ParcError>
├── parc-cli/      # CLI binary — terminal formatting, $EDITOR, clap
└── parc-server/   # JSON-RPC 2.0 server (stdio / Unix socket)
```

### parc-core

The library. Contains the data model, the Markdown frontmatter parser, the schema engine, the search DSL parser and compiler, the SQLite + FTS5 index, the history snapshot engine, the wiki-link resolver, and the lifecycle hook dispatcher.

Rules:

- No `println!`, no `print!`, no TTY assumptions
- All operations return `Result<T, ParcError>` with structured error variants
- Takes a `VaultPath` as input — never assumes a location, never reads `$HOME`
- No global state — every operation takes the vault and config it needs

This is what makes the CLI/TUI and server so thin. Neither re-implements business logic; they just translate between their respective transports and `parc-core`.

### parc-cli

The `parc` binary. Built on `clap` (derive). Adds terminal-specific concerns:

- Markdown rendering with [`termimad`](https://crates.io/crates/termimad)
- Diff rendering with [`similar`](https://crates.io/crates/similar)
- `$EDITOR` invocation for `parc new` and `parc edit`
- TTY detection and a built-in terminal UI (`parc tui`)
- JSON output mode (`--json`) for scripts

Every CLI command is a thin function that calls `parc-core` and formats the result.

### parc-server

The `parc-server` binary, also reachable as `parc server`. A JSON-RPC 2.0 server with two transports: newline-delimited JSON over stdio, or the same protocol over a Unix domain socket. Twenty methods covering the full core API — see [JSON-RPC server]({{ '/json-rpc/' | url }}).

The server is built on `tokio`. Each request handler is a thin wrapper around a `parc-core` call. Errors from `parc-core` are mapped to JSON-RPC error codes.

## Files first, index second

The Markdown files in `<vault>/fragments/` are the source of truth. The SQLite index in `<vault>/index.db` is fully derivable from them — `parc reindex` rebuilds it from scratch in seconds for a vault of thousands of fragments.

This gives you four useful properties:

1. **Backups are simple.** `tar` the vault, you have everything that matters. Restore on any machine, run `parc reindex`, you're back.
2. **Git transport works.** Pull a collaborator's changes, run `parc reindex`, see their fragments.
3. **External edits are safe.** Edit fragments by hand, with vim, with a script — `parc reindex` catches up.
4. **Recovery is local.** A corrupted index is a single command to fix; there is no remote anything to coordinate with.

## The search pipeline

A query string flows through these stages:

```
"type:todo #backend due:this-week" 
        │
        ▼  parc_core::search::parser
   SearchQuery AST
        │
        ▼  parc_core::search::compiler
  FTS5 MATCH + SQL WHERE
        │
        ▼  rusqlite (FTS5)
   Result rows
        │
        ▼  parc_core::search::resolver
  Fragment summaries
        │
        ▼  consumer (CLI/TUI / RPC)
```

The parser produces an AST that the consumer can inspect (`parc search ... --explain`). The compiler is the only stage that knows about SQL — every transformation above it is pure data.

## Plugins

Plugins are WebAssembly modules loaded into a `wasmtime` sandbox. The runtime is gated behind the `wasm-plugins` cargo feature so default builds carry zero overhead. Plugin manifest types and the management subcommands work without the feature; only loading and execution require it.

Plugins talk to parc through a `parc_host` namespace exposing fragment CRUD, search, logging, and output. The capability set declared in each plugin's manifest is enforced at link time — a plugin without `write_fragments` cannot import the corresponding host function at all, not just at runtime.

## Why this shape

The library-first design exists because parc has multiple front doors (CLI/TUI, JSON-RPC, and plugins). Putting all logic in `parc-core` and keeping the binaries thin means:

- A bug fix in the search compiler benefits every frontend at once
- New frontends are cheap to add because they share the same core operations
- Tests for the engine don't need to spin up a binary or a server
- The JSON-RPC surface and the CLI surface are guaranteed to be in sync, because both are calling the same functions

It also makes the codebase pleasant to work in. Almost every interesting question has a single answer — read the function in `parc-core`.
