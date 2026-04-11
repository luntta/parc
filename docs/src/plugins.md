---
layout: layouts/doc.njk
title: Plugins
eyebrow: Reference · §06
---

Extend parc with WebAssembly plugins. Plugins are `.wasm` binaries packaged with TOML manifests, installed into `<vault>/plugins/`. They can hook into the fragment lifecycle, validate fragments, render custom output, and add new top-level CLI subcommands.

Plugins require the `wasm-plugins` cargo feature:

```bash
cargo install --path parc-cli --features wasm-plugins
```

Plugin manifest types and the management subcommands work without the feature — only runtime loading and execution require it. The default builds carry zero `wasmtime` overhead.

## Plugin commands

```bash
parc plugin list                              # list installed plugins
parc plugin info <name>                       # show plugin details
parc plugin install <path> [--manifest file]  # install a plugin
parc plugin remove <name> [--force]           # remove a plugin
```

`install` accepts either a directory containing `plugin.toml` and a `.wasm` file or a path directly to the manifest with `--manifest`.

## Manifest format

```toml
# .parc/plugins/my-plugin/plugin.toml
[plugin]
name = "my-plugin"
version = "0.1.0"
description = "Does something useful"
author = "alice"
wasm = "my-plugin.wasm"

[capabilities]
read_fragments = true
write_fragments = false
hooks = ["post-create", "pre-update"]
validate = ["todo"]
render = ["note"]
extend_cli = ["my-command"]

[config]
# Optional plugin-specific defaults — overridden by <vault>/config.yml#plugins.my-plugin
default_setting = "value"
```

## Capability sandbox

Plugins run in a `wasmtime` sandbox. They can only do what their manifest declares. parc verifies the requested capabilities at install time and again at load time.

| Capability | Effect |
|------------|--------|
| `read_fragments` | Plugin can call `parc_host::fragment_get`, `fragment_list`, `fragment_search` |
| `write_fragments` | Plugin can call `fragment_create`, `fragment_update`, `fragment_delete` |
| `hooks = [...]` | Plugin is invoked for these lifecycle events |
| `validate = [...]` | Plugin's `validate(type, fragment)` is called for these types after schema validation |
| `render = [...]` | Plugin's `render(type, fragment)` is called when displaying these types |
| `extend_cli = [...]` | Plugin adds these names as top-level CLI subcommands |

Plugins have no filesystem access beyond what parc gives them through the `parc_host` namespace, no network, no environment variables, no spawning processes.

## Lifecycle hooks

| Hook | When | Can mutate? |
|------|------|-------------|
| `pre-create` | Before a new fragment is written | Yes (return modified fragment) |
| `post-create` | After a new fragment is written | No (read-only) |
| `pre-update` | Before an edit lands | Yes |
| `post-update` | After an edit lands | No |
| `pre-delete` | Before a soft-delete | Can abort by returning an error |
| `post-delete` | After a soft-delete | No |

A plugin returning an error from a `pre-*` hook aborts the operation and propagates the error to the user.

## Extending the CLI

A plugin that declares `extend_cli = ["weekly-review"]` exposes itself as `parc weekly-review`. parc dispatches the subcommand and any arguments to the plugin's `cli_dispatch(args: Vec<String>)` function.

```bash
parc weekly-review                  # dispatched to the plugin
parc weekly-review --since monday   # arguments are forwarded as-is
```

Plugin output is written to stdout via `parc_host::print` (or `print_json`); errors via `parc_host::error`. parc takes care of TTY detection and colour stripping.

## Compiling a plugin

Plugins are standard Rust crates compiled to `wasm32-unknown-unknown`. parc ships a `parc-plugin-sdk` crate that re-exports the host bindings and helper macros — see the SDK documentation for the full API surface.

```toml
# Cargo.toml
[lib]
crate-type = ["cdylib"]

[dependencies]
parc-plugin-sdk = "0.1"
```

```rust
use parc_plugin_sdk::*;

#[parc_plugin]
fn validate(ty: &str, fragment: &Fragment) -> ValidateResult {
    if ty == "todo" && fragment.title.len() > 80 {
        ValidateResult::error("todo titles must be 80 characters or less")
    } else {
        ValidateResult::ok()
    }
}
```

Build with `cargo build --target wasm32-unknown-unknown --release` and install the resulting `.wasm` plus a `plugin.toml` with `parc plugin install ./target/wasm32-unknown-unknown/release/`.

## Configuration

Per-plugin settings live under the `plugins` key in the vault's `config.yml`:

```yaml
plugins:
  my-plugin:
    api_url: https://example.invalid
    threshold: 5
```

parc passes the matching subtree to the plugin's `init(config: serde_json::Value)` callback when the plugin is loaded. Updates to `config.yml` only take effect the next time the plugin is loaded — `parc reindex` reloads plugins as a side effect.

## When plugins make sense

Plugins are the right tool for:

- Custom validation that can't be expressed in a YAML schema
- Custom output rendering — e.g. PlantUML / Mermaid → ASCII art for terminal display
- Wiring parc into another tool's lifecycle — e.g. push new todos to an external tracker
- Adding domain-specific subcommands that share parc's vault, schema, and indexing infrastructure

For one-off scripts, the JSON-RPC server is usually a better fit — no Rust required, no `.wasm` build pipeline, and the same surface area.
