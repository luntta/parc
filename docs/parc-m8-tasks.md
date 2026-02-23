# M8 — WASM Plugin System

## Features

1. **Plugin manifest format** — TOML manifest defining name, version, capabilities, and WASM binary path
2. **WASM runtime integration** — Embed `wasmtime` to load and execute `.wasm` plugins
3. **Plugin host API** — Define the host functions plugins can call (read/write fragments, query index, etc.)
4. **Capability-based sandboxing** — Plugins only access what their manifest declares
5. **Lifecycle event dispatch** — WASM plugins receive the same lifecycle events as Tier 1 hooks
6. **Custom validation** — Plugins can validate fragments for custom types
7. **Custom rendering** — Plugins can provide alternative rendering for fragment types
8. **Custom CLI subcommands** — Plugins can register subcommands under `parc <plugin-name> <cmd>`
9. **`parc plugin` management commands** — install, remove, list, info
10. **Plugin configuration** — Per-plugin config in vault `config.yml`
11. **Integration with existing hook system** — WASM plugins coexist with Tier 1 hook scripts
12. **Integration tests** — Full plugin lifecycle tests

**PRD refs:** §3.1 (architecture diagram), §12 (Plugin System), §9 (plugin config), M8 milestone definition.

---

## Feature 1: Plugin Manifest Format

### Files
- `parc-core/src/plugin.rs` — **New.** Manifest types, loading, validation

### Design

Each WASM plugin lives in `<vault>/plugins/` as a pair: a `.wasm` binary and a `.toml` manifest. The manifest declares what the plugin is and what it's allowed to do.

```
plugins/
├── word-count.wasm
├── word-count.toml
├── kanban.wasm
└── kanban.toml
```

Manifest format:

```toml
[plugin]
name = "word-count"
version = "0.1.0"
description = "Adds word count to fragment metadata"
wasm = "word-count.wasm"         # relative to plugins/ dir

[capabilities]
read_fragments = true            # can call host fn to read fragments
write_fragments = false          # cannot modify fragments via host API
extend_cli = false               # no custom subcommands
hooks = ["post-create", "post-update"]   # lifecycle events to subscribe to
render = []                      # fragment types to provide rendering for
validate = []                    # fragment types to provide validation for
```

### Tasks
- [ ] Create `parc-core/src/plugin.rs` with manifest types:
  - `PluginManifest { plugin: PluginMeta, capabilities: PluginCapabilities }`
  - `PluginMeta { name: String, version: String, description: Option<String>, wasm: String }`
  - `PluginCapabilities { read_fragments: bool, write_fragments: bool, extend_cli: bool, hooks: Vec<String>, render: Vec<String>, validate: Vec<String> }`
- [ ] `load_manifest(path: &Path) -> Result<PluginManifest, ParcError>` — parse TOML file
- [ ] `discover_plugins(vault: &Path) -> Result<Vec<PluginManifest>, ParcError>` — scan `plugins/` for `.toml` files
- [ ] Validate manifest: name must be non-empty, wasm file must exist, hook names must be valid `HookEvent` variants
- [ ] Add `PluginError` variant to `ParcError` in `error.rs`
- [ ] Add `pub mod plugin;` to `lib.rs`
- [ ] Add `toml` crate to `parc-core/Cargo.toml`
- [ ] Unit tests: parse valid manifest, reject missing wasm file, reject invalid hook name

---

## Feature 2: WASM Runtime Integration

### Files
- `parc-core/src/plugin/runtime.rs` — WASM module loading and instance management
- `parc-core/Cargo.toml` — Add `wasmtime` dependency

### Design

Use `wasmtime` to compile and instantiate WASM modules. Each plugin gets its own `Instance` with a `Store` containing plugin-specific state. Modules are precompiled on first load for faster subsequent starts.

The plugin guest must export specific functions that the host calls. We use a simple C-ABI-compatible interface with JSON serialization for complex data (same pattern as Tier 1 hooks):

**Required guest exports:**
- `parc_plugin_init() -> i32` — called once at load time, return 0 for success
- `parc_plugin_name() -> *const u8` — return plugin name (must match manifest)

**Optional guest exports (based on capabilities):**
- `parc_on_event(event_ptr, event_len, fragment_ptr, fragment_len) -> i32` — lifecycle hook
- `parc_validate(fragment_ptr, fragment_len) -> i32` — validation (0 = valid)
- `parc_render(fragment_ptr, fragment_len) -> i32` — custom rendering
- `parc_command(cmd_ptr, cmd_len, args_ptr, args_len) -> i32` — CLI subcommand

Guest-to-host communication uses shared linear memory with a simple allocator protocol: guest exports `parc_alloc(size) -> *mut u8` and `parc_free(ptr, size)`. Host writes input data into guest memory, guest writes output via a host-provided `parc_host_output(ptr, len)` function.

### Tasks
- [ ] Add `wasmtime` dependency to `parc-core/Cargo.toml`:
  - `wasmtime = { version = "28", default-features = false, features = ["cranelift", "runtime"] }`
  - Feature-gate behind `wasm-plugins` feature flag: `wasm-plugins = ["wasmtime"]`
- [ ] Create `plugin/runtime.rs` (convert `plugin.rs` to `plugin/mod.rs` + submodules):
  - `WasmRuntime` struct — holds `wasmtime::Engine` (shared across plugins)
  - `PluginInstance` struct — holds `wasmtime::Store<PluginState>`, `wasmtime::Instance`
  - `PluginState` — per-plugin state: manifest, output buffer, vault path
- [ ] `WasmRuntime::new() -> Result<Self>` — create engine with default config
- [ ] `WasmRuntime::load_plugin(manifest, wasm_bytes) -> Result<PluginInstance>`:
  - Compile WASM module
  - Create store with `PluginState`
  - Link host imports (Feature 3)
  - Instantiate module
  - Call `parc_plugin_init()`, fail if non-zero
  - Verify `parc_plugin_name()` matches manifest name
- [ ] `PluginInstance::call_event(event, fragment_json) -> Result<Option<String>>` — for lifecycle hooks
- [ ] `PluginInstance::call_validate(fragment_json) -> Result<ValidationResult>` — for validation
- [ ] `PluginInstance::call_render(fragment_json) -> Result<Option<String>>` — for rendering
- [ ] `PluginInstance::call_command(cmd, args_json) -> Result<String>` — for CLI subcommands
- [ ] Helper: write JSON bytes into guest memory via `parc_alloc`, read output via buffer
- [ ] Unit test: load a minimal no-op WASM plugin, verify init succeeds

---

## Feature 3: Plugin Host API

### Files
- `parc-core/src/plugin/host.rs` — Host functions exposed to WASM guests

### Design

Host functions are linked into the WASM instance via `wasmtime::Linker`. They provide controlled access to vault operations. Each function checks capabilities before executing.

Host functions are namespaced under `parc_host`:

| Host Function | Capability Required | Description |
|--------------|-------------------|-------------|
| `parc_host_output(ptr, len)` | (always) | Write output data back to host |
| `parc_host_log(level, ptr, len)` | (always) | Log a message (debug/info/warn/error) |
| `parc_host_fragment_get(id_ptr, id_len)` | `read_fragments` | Read a fragment by ID (prefix) |
| `parc_host_fragment_search(query_ptr, query_len)` | `read_fragments` | Search fragments using DSL |
| `parc_host_fragment_list(params_ptr, params_len)` | `read_fragments` | List fragments with filters |
| `parc_host_fragment_create(json_ptr, json_len)` | `write_fragments` | Create a new fragment |
| `parc_host_fragment_update(json_ptr, json_len)` | `write_fragments` | Update an existing fragment |

All host functions that return data write the result into the `PluginState` output buffer, returning the byte length. The guest then reads from the buffer. Functions that would violate capabilities return an error code (-1) without executing.

### Tasks
- [ ] Create `plugin/host.rs` with host function definitions
- [ ] Implement `parc_host_output(ptr, len)` — copies guest memory region to `PluginState.output_buffer`
- [ ] Implement `parc_host_log(level, ptr, len)` — logs to stderr with plugin name prefix
- [ ] Implement `parc_host_fragment_get(id_ptr, id_len)`:
  - Check `read_fragments` capability, return -1 if denied
  - Resolve vault, open index, load fragment, serialize to JSON
  - Write result to output buffer
- [ ] Implement `parc_host_fragment_search(query_ptr, query_len)`:
  - Check `read_fragments` capability
  - Parse DSL query, execute search, serialize results as JSON array
- [ ] Implement `parc_host_fragment_list(params_ptr, params_len)`:
  - Check `read_fragments`, deserialize params, query index
- [ ] Implement `parc_host_fragment_create(json_ptr, json_len)`:
  - Check `write_fragments` capability
  - Deserialize fragment data, create via core API
- [ ] Implement `parc_host_fragment_update(json_ptr, json_len)`:
  - Check `write_fragments` capability
  - Deserialize, merge fields, update via core API
- [ ] Register all host functions via `wasmtime::Linker` in `runtime.rs`
- [ ] Unit tests: verify capability denial returns -1, verify read_fragments works when allowed

---

## Feature 4: Capability-Based Sandboxing

### Files
- `parc-core/src/plugin/mod.rs` — Capability checking logic
- `parc-core/src/plugin/host.rs` — Enforcement in host functions

### Design

Capabilities are checked at the host function boundary. The WASM sandbox already prevents filesystem/network access — capabilities further restrict which parc operations a plugin can invoke. A plugin with `read_fragments = true` but `write_fragments = false` can inspect the vault but not modify it.

The `hooks` capability list restricts which lifecycle events the plugin receives. The `render` and `validate` lists restrict which fragment types the plugin can handle.

### Tasks
- [ ] `PluginCapabilities::allows_hook(event: &str) -> bool` — check hooks list
- [ ] `PluginCapabilities::allows_render(fragment_type: &str) -> bool` — check render list
- [ ] `PluginCapabilities::allows_validate(fragment_type: &str) -> bool` — check validate list
- [ ] Guard every host function call with capability check; return error code on denial
- [ ] `PluginManager::plugins_for_event(event) -> Vec<&PluginInstance>` — filter by hooks capability
- [ ] `PluginManager::plugins_for_render(type_name) -> Vec<&PluginInstance>` — filter by render capability
- [ ] `PluginManager::plugins_for_validate(type_name) -> Vec<&PluginInstance>` — filter by validate capability
- [ ] Unit tests: plugin with read-only caps cannot call write host functions

---

## Feature 5: Lifecycle Event Dispatch

### Files
- `parc-core/src/plugin/manager.rs` — **New.** `PluginManager` orchestrating loaded plugins
- `parc-core/src/hook.rs` — Extend to dispatch to WASM plugins alongside scripts

### Design

`PluginManager` loads all plugins at startup and provides methods to dispatch lifecycle events. It integrates with the existing `HookRunner` pattern — WASM plugins are invoked after Tier 1 hook scripts, maintaining the same semantics:

- **Pre-event plugins**: Can modify the fragment (return modified JSON). Chained sequentially. Non-zero return aborts the operation.
- **Post-event plugins**: Side-effects only. Errors logged as warnings, don't abort.

The existing `run_pre_hooks()` / `run_post_hooks()` functions in `hook.rs` are extended to accept an optional `PluginManager` reference. If present, WASM plugins are dispatched after script hooks.

### Tasks
- [ ] Create `plugin/manager.rs` with `PluginManager` struct:
  - `plugins: Vec<PluginInstance>`
  - `runtime: WasmRuntime`
- [ ] `PluginManager::load_all(vault: &Path) -> Result<Self>`:
  - Discover manifests, load each WASM module, initialize instances
  - Log plugin load success/failure to stderr
  - Skip plugins that fail to load (warn, don't abort)
- [ ] `PluginManager::dispatch_pre_event(event, fragment) -> Result<Fragment>`:
  - Filter plugins by hooks capability for this event
  - Serialize fragment to JSON, call each plugin's `parc_on_event` sequentially
  - If plugin returns modified JSON, deserialize and pass to next plugin
  - Non-zero return → abort with `ParcError::PluginError`
- [ ] `PluginManager::dispatch_post_event(event, fragment) -> Result<()>`:
  - Same filter, but ignore errors (log warning)
- [ ] Extend `run_pre_hooks()` signature: `run_pre_hooks(vault, event, fragment, runner, plugins: Option<&PluginManager>)`
  - After script hooks complete, dispatch to WASM plugins
- [ ] Extend `run_post_hooks()` similarly
- [ ] Update call sites in CLI commands (`new.rs`, `edit.rs`, `delete.rs`) to pass `PluginManager`
- [ ] Integration test: hook script and WASM plugin both fire on fragment creation

---

## Feature 6: Custom Validation

### Files
- `parc-core/src/plugin/manager.rs` — Validation dispatch method
- `parc-core/src/schema.rs` — Hook into schema validation pipeline

### Design

Plugins declaring `validate = ["my-type"]` get called during fragment validation for that type. Plugin validation runs **after** schema validation — it can enforce rules beyond what YAML schemas express (e.g., cross-field dependencies, external lookups).

Validation returns a JSON object: `{"valid": true}` or `{"valid": false, "errors": ["msg1", "msg2"]}`.

### Tasks
- [ ] `PluginManager::validate(fragment) -> Result<Vec<ValidationIssue>>`:
  - Filter plugins by validate capability for this fragment type
  - Call each plugin's `parc_validate` with fragment JSON
  - Collect and merge validation errors
  - Return empty vec if all pass
- [ ] Define `ValidationIssue { plugin: String, message: String }`
- [ ] Integrate into fragment write path: after schema validation, run plugin validation
  - On validation failure, abort write and return errors
- [ ] Integration test: plugin rejects a fragment with invalid custom field

---

## Feature 7: Custom Rendering

### Files
- `parc-core/src/plugin/manager.rs` — Render dispatch method
- `parc-cli/src/commands/show.rs` — Use plugin rendering when available

### Design

Plugins declaring `render = ["kanban"]` can provide custom terminal rendering for fragments of that type. The `parc show` command checks for a rendering plugin before falling back to default Markdown rendering.

The plugin receives the fragment JSON and returns a rendered string (plain text or Markdown).

### Tasks
- [ ] `PluginManager::render(fragment) -> Result<Option<String>>`:
  - Filter plugins by render capability for this fragment type
  - Call first matching plugin's `parc_render`
  - Return `None` if no plugin handles this type
- [ ] Update `show.rs` to call `PluginManager::render()` first:
  - If `Some(rendered)`, display it
  - If `None`, use existing default rendering
- [ ] Integration test: plugin provides custom rendering for a custom type

---

## Feature 8: Custom CLI Subcommands

### Files
- `parc-cli/src/main.rs` — Dynamic subcommand dispatch
- `parc-core/src/plugin/manager.rs` — Command listing and execution

### Design

Plugins declaring `extend_cli = true` can register custom subcommands. These appear as `parc <plugin-name> [args...]`. The plugin's `parc_command` export receives the subcommand name and arguments as JSON.

The CLI discovers available plugin commands at startup and adds them as dynamic subcommands via clap's external subcommand mechanism.

### Tasks
- [ ] `PluginManager::list_commands() -> Vec<PluginCommand>`:
  - `PluginCommand { plugin_name: String, commands: Vec<CommandSpec> }`
  - `CommandSpec { name: String, description: String, args: Vec<ArgSpec> }`
  - Call each plugin's `parc_commands()` export (optional, returns JSON array of `CommandSpec`)
- [ ] `PluginManager::execute_command(plugin_name, cmd, args) -> Result<String>`:
  - Find plugin by name, check `extend_cli` capability
  - Serialize args as JSON, call `parc_command`
  - Return output string
- [ ] Update `main.rs` to handle unrecognized subcommands:
  - Use clap's `allow_external_subcommands` on the top-level command
  - When an unknown subcommand is encountered, check `PluginManager::list_commands()`
  - If found, call `execute_command`, print output
  - If not found, show standard "unknown command" error
- [ ] Integration test: plugin registers a subcommand, CLI dispatches to it

---

## Feature 9: `parc plugin` Management Commands

### Files
- `parc-cli/src/commands/plugin.rs` — **New.** Plugin management handler
- `parc-cli/src/commands/mod.rs` — `pub mod plugin;`
- `parc-cli/src/main.rs` — `Plugin` command variant with subcommands

### CLI Interface

```bash
parc plugin list                        # list installed plugins
parc plugin info <name>                 # show plugin details and capabilities
parc plugin install <path-to-wasm> [--manifest <path-to-toml>]   # install a plugin
parc plugin remove <name>               # remove a plugin
```

### Tasks
- [ ] Add `Plugin` command to `main.rs` with subcommands:
  ```
  Plugin {
      #[command(subcommand)]
      action: PluginAction,
  }
  ```
  - `List { #[arg(long)] json: bool }`
  - `Info { name: String, #[arg(long)] json: bool }`
  - `Install { path: PathBuf, #[arg(long)] manifest: Option<PathBuf> }`
  - `Remove { name: String }`
- [ ] `plugin list`:
  - Discover all manifests in `plugins/`
  - Display table: NAME, VERSION, CAPABILITIES (summarized), DESCRIPTION
  - `--json`: array of manifest objects
- [ ] `plugin info <name>`:
  - Load specific manifest, display full details
  - Show capabilities as a readable list
  - Show subscribed hooks, validated types, rendered types
  - `--json`: full manifest as JSON
- [ ] `plugin install <path>`:
  - Copy `.wasm` file to `plugins/`
  - If `--manifest` provided, copy `.toml` to `plugins/`
  - If no manifest, generate a minimal one (name from filename, no capabilities)
  - Validate: load the WASM module, call `parc_plugin_init`, verify it works
  - On failure, clean up copied files
- [ ] `plugin remove <name>`:
  - Find and delete `.wasm` and `.toml` files from `plugins/`
  - Confirm before deleting (unless `--force`)
- [ ] Register in `mod.rs` and `main.rs`
- [ ] Integration tests: install a test plugin, list it, show info, remove it

---

## Feature 10: Plugin Configuration

### Files
- `parc-core/src/config.rs` — Parse `plugins` section
- `parc-core/src/plugin/manager.rs` — Pass config to plugins

### Design

Per-plugin configuration lives in `config.yml` under the `plugins` key:

```yaml
plugins:
  word-count:
    min_words: 10
    warn_threshold: 5000
  kanban:
    columns: ["todo", "in-progress", "done"]
```

Plugin config is passed as JSON to `parc_plugin_init()` (extending it to accept a config parameter). Plugins that don't need config receive `{}`.

### Tasks
- [ ] Update `VaultConfig` in `config.rs` to parse `plugins: HashMap<String, serde_yaml::Value>`
- [ ] `PluginManager::load_all()` passes per-plugin config to each instance
- [ ] Extend `parc_plugin_init` to accept config: `parc_plugin_init(config_ptr, config_len) -> i32`
  - Serialize the plugin's YAML config section as JSON, write to guest memory
  - Plugins without config receive empty object `{}`
- [ ] `parc plugin info <name>` shows current config from `config.yml`
- [ ] Unit test: plugin receives and uses config values

---

## Feature 11: Integration with Existing Hook System

### Files
- `parc-core/src/hook.rs` — Unify dispatch ordering
- `parc-core/src/plugin/manager.rs` — Implement `HookRunner`-compatible interface

### Design

Tier 1 (scripts) and Tier 2 (WASM) hooks coexist. Execution order for a given event:

1. Tier 1 generic hook scripts (e.g., `hooks/pre-create`)
2. Tier 1 type-specific hook scripts (e.g., `hooks/pre-create.todo`)
3. Tier 2 WASM plugins (ordered by manifest discovery, i.e., alphabetical by name)

This ordering ensures simple scripts run first (fast, predictable) and WASM plugins can build on their output. A pre-hook failure at any tier aborts the entire operation.

### Tasks
- [ ] Refactor `run_pre_hooks()` to accept `Option<&PluginManager>`:
  - After all script hooks, call `PluginManager::dispatch_pre_event()`
  - Fragment modification chains through both tiers
- [ ] Refactor `run_post_hooks()` similarly
- [ ] Update `CliHookRunner` usage in `new.rs`, `edit.rs`, `delete.rs` to optionally pass `PluginManager`
- [ ] Ensure `parc doctor` checks plugin health:
  - All manifests have valid WASM files
  - All WASM modules load and init successfully
  - Report as warnings (not errors) — plugins are optional
- [ ] Integration test: Tier 1 script and Tier 2 WASM plugin both fire on same event, verify ordering

---

## Feature 12: Integration Tests

### Files
- `parc-core/tests/plugin_integration.rs` — **New.** Full plugin test suite
- `parc-core/tests/fixtures/` — Test WASM plugin binaries and manifests

### Design

Build minimal test plugins in Rust compiled to `wasm32-wasip1`. The test fixtures are pre-compiled `.wasm` files checked into the repo. Each test creates a temp vault, installs the test plugin, and exercises it.

### Test Plugins Needed

1. **`echo-plugin`** — Minimal plugin that returns its input unchanged. Tests basic lifecycle.
2. **`modify-plugin`** — Pre-create hook that adds a tag. Tests fragment modification.
3. **`validate-plugin`** — Rejects fragments missing a required custom field. Tests validation.
4. **`counter-plugin`** — Tracks how many times it's called via host log. Tests host API.
5. **`cli-plugin`** — Registers a `greet` subcommand. Tests CLI extension.

### Tasks
- [ ] Create test plugin source in `tests/fixtures/plugins/` (Rust source → compiled to WASM)
- [ ] Build script or Makefile target to compile test plugins to `.wasm`
- [ ] Test: load plugin — discover manifest, load WASM, init succeeds
- [ ] Test: lifecycle dispatch — create fragment, verify WASM plugin's `on_event` is called
- [ ] Test: pre-hook modification — plugin modifies fragment, verify change persists
- [ ] Test: validation — plugin rejects invalid fragment, verify error propagates
- [ ] Test: capability enforcement — plugin without `write_fragments` cannot call write host functions
- [ ] Test: custom rendering — plugin provides rendering, verify `show` uses it
- [ ] Test: CLI subcommand — plugin registers command, verify dispatch and output
- [ ] Test: config — plugin receives config from `config.yml`
- [ ] Test: coexistence — Tier 1 script + Tier 2 WASM plugin fire on same event
- [ ] Test: plugin install/remove — CLI commands manage plugins correctly
- [ ] Test: doctor — reports plugin issues as warnings

---

## Implementation Order

1. **Feature 1** (manifest) — define the data model, no WASM yet
2. **Feature 2** (runtime) — get `wasmtime` loading and calling a minimal WASM module
3. **Feature 3** (host API) — expose vault operations to plugins
4. **Feature 4** (sandboxing) — capability enforcement on host functions
5. **Feature 5** (lifecycle dispatch) — wire WASM plugins into the hook pipeline
6. **Feature 10** (config) — pass configuration to plugins
7. **Feature 6** (validation) — plugin validation in the write path
8. **Feature 7** (rendering) — plugin rendering in `show`
9. **Feature 8** (CLI subcommands) — dynamic subcommand dispatch
10. **Feature 9** (`parc plugin` commands) — management CLI
11. **Feature 11** (integration) — unify Tier 1 + Tier 2, doctor checks
12. **Feature 12** (tests) — full integration suite

---

## Verification

```bash
cargo build -p parc-core --features wasm-plugins
cargo test -p parc-core --features wasm-plugins
cargo test -p parc-cli

# Plugin management
parc plugin list                                     # empty list initially
parc plugin install ./my-plugin.wasm --manifest ./my-plugin.toml
parc plugin list                                     # shows installed plugin
parc plugin info my-plugin                           # full details

# Lifecycle hooks (with a plugin subscribed to post-create)
parc new note --title "Test plugin hooks"
# stderr should show plugin log output if plugin logs

# Custom validation (with a validate plugin for custom type)
parc schema add ./custom-type.yml
parc new custom-type --title "Missing required field"
# should fail with plugin validation error

# Custom CLI subcommand
parc my-plugin greet --name "World"
# should output plugin-defined response

# Capability enforcement
# Plugin with read-only caps cannot modify fragments via host API
# (verified via integration tests, not manual testing)

# Coexistence with Tier 1 hooks
echo '#!/bin/sh\necho "tier1"' > .parc/hooks/post-create && chmod +x .parc/hooks/post-create
parc new note --title "Both tiers fire"
# both hook script and WASM plugin should execute

# Plugin removal
parc plugin remove my-plugin
parc plugin list                                     # empty again

# Doctor checks
parc doctor                                          # reports plugin health

# Feature flag: building without WASM support
cargo build -p parc-core                             # works without wasmtime
cargo build -p parc-core --features wasm-plugins     # includes WASM support
```
