# M4 — Templates, Aliases & Hooks

## Features

1. **`parc schema add <path>`** — Register user-defined fragment types
2. **Relative date parsing for `--due`** — Resolve `today`, `tomorrow`, `in-3-days` etc.
3. **Tier 1 hook scripts** — Lifecycle hooks in `<vault>/hooks/`
4. **Tab completion** — `parc completions <shell>` via `clap_complete`

**Already done (skip):** Aliases (config.yml, SchemaRegistry::resolve), Templates (load_template, built-in fallback, editor merge).

---

## Feature 1: `parc schema add <path>`

### Files
- `parc-core/src/schema.rs` — `add_schema()`, `validate_schema_file()`
- `parc-cli/src/main.rs` — `Schema { Add { path } }` command variant
- `parc-cli/src/commands/schema.rs` — Command handler
- `parc-cli/src/commands/mod.rs` — `pub mod schema;`

### Tasks
- [ ] Add `validate_schema_file(path) -> Result<Schema>` to parse & validate a schema YAML
- [ ] Add `add_schema(vault, source_path) -> Result<String>` that copies to `schemas/`
- [ ] Check for name collision with existing schemas
- [ ] Optionally create empty template at `templates/<name>.md`
- [ ] Add `Schema { Add { path } }` CLI command and handler
- [ ] Integration test: add custom schema, verify with `parc types`

---

## Feature 2: Relative date parsing for `--due`

### Files
- `parc-core/src/date.rs` — `resolve_due_date(input) -> Result<String>`
- `parc-core/src/lib.rs` — `pub mod date;`
- `parc-core/src/search.rs` — Import shared date resolution from `date.rs`
- `parc-cli/src/commands/new.rs` — Call `resolve_due_date()` on `--due`
- `parc-cli/src/commands/set.rs` — Call `resolve_due_date()` when field is "due"

### Tasks
- [ ] Create `date.rs` with `resolve_due_date()` and `resolve_relative_date_to_range()`
- [ ] Move `RelativeDate`, `parse_relative_date()`, `resolve_relative_date()` to `date.rs`
- [ ] Add `in-N-days` shorthand
- [ ] Have `search.rs` import from `date.rs`
- [ ] Call `resolve_due_date()` in `new.rs` and `set.rs`
- [ ] Unit tests for all shorthands
- [ ] Integration tests: `parc new todo "X" --due today`

---

## Feature 3: Tier 1 Hook Scripts

### Files
- `parc-core/src/hook.rs` — `HookEvent`, `HookScript`, `discover_hooks()`, `HookRunner` trait
- `parc-core/src/lib.rs` — `pub mod hook;`
- `parc-cli/src/hooks.rs` — `CliHookRunner` (process spawning)
- `parc-cli/src/commands/new.rs` — Pre/post-create hooks
- `parc-cli/src/commands/edit.rs` — Pre/post-update hooks
- `parc-cli/src/commands/set.rs` — Pre/post-update hooks
- `parc-cli/src/commands/delete.rs` — Pre/post-delete hooks

### Tasks
- [ ] Define `HookEvent` enum, `HookScript` struct, `HookContext`
- [ ] Implement `discover_hooks(vault, event, type)` to scan hooks dir
- [ ] Define `HookRunner` trait with `run_pre_hook` / `run_post_hook`
- [ ] Implement `CliHookRunner` in CLI (spawn process, pipe JSON to stdin)
- [ ] Pre-hooks: non-zero exit aborts; stdout parsed as modified fragment JSON
- [ ] Post-hooks: non-zero exit warns; stdout ignored
- [ ] Hook naming: `pre-create`, `post-create.todo` (type-specific)
- [ ] Integrate into new, edit, set, delete commands
- [ ] Integration test: create hook script, verify execution

---

## Feature 4: Tab Completion

### Files
- `parc-cli/Cargo.toml` — `clap_complete` dependency
- `parc-cli/src/main.rs` — `Completions { shell }` command
- `parc-cli/src/commands/completions.rs` — Generate completions to stdout
- `parc-cli/src/commands/mod.rs` — `pub mod completions;`

### Tasks
- [ ] Add `clap_complete` to Cargo.toml
- [ ] Add `Completions { shell: String }` command variant
- [ ] Generate completions for bash, zsh, fish, elvish
- [ ] Integration test: `parc completions bash` produces output

---

## Verification

```bash
cargo build
cargo test -p parc-core
cargo test -p parc-cli

# Schema add
parc schema add /path/to/custom.yml
parc types  # should show new type

# Due date
parc new todo "Test" --due today
parc new todo "Test" --due tomorrow
parc new todo "Test" --due in-3-days

# Hooks
echo '#!/bin/sh\necho "hook fired" >&2' > .parc/hooks/post-create
chmod +x .parc/hooks/post-create
parc new note "Hook test"  # should see "hook fired"

# Completions
parc completions bash
parc completions zsh
```
