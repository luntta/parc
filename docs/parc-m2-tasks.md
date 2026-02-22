# parc M2 — Implementation Task Breakdown

**Goal:** Multi-vault support — vault resolution priority chain (`--vault` flag, `PARC_VAULT` env var, local discovery, global fallback), `parc vault` / `parc vault list` commands, and `parc init --vault <path>`.

**Prerequisite:** M1 complete (wiki-links, bidirectional links, backlinks, doctor).

---

## Phase 0: Vault Resolution Priority Chain

### T0.1 — `resolve_vault()` in `vault.rs`

Add a new `resolve_vault` function to `parc-core/src/vault.rs` that implements the full vault resolution priority chain.

**Priority order:**
1. `explicit` — from `--vault` flag (highest priority)
2. `PARC_VAULT` — environment variable
3. Local discovery — walk up from CWD looking for `.parc/`
4. Global fallback — `~/.parc`

**Functions:**

```rust
/// Resolve the active vault using the priority chain:
/// explicit path > PARC_VAULT env > local discovery (CWD walk-up) > global ~/.parc
pub fn resolve_vault(explicit: Option<&Path>) -> Result<PathBuf, ParcError>
```

**Behavior:**
- If `explicit` is `Some`, check if it's a valid vault. If the path ends with `.parc`, use it directly; otherwise, append `.parc`. Return `VaultNotFound` if not valid.
- If `PARC_VAULT` env var is set and non-empty, treat it the same way as `explicit`.
- Otherwise, delegate to existing `discover_vault()` (which already walks up from CWD and falls back to global).

**Note:** `discover_vault()` and `discover_vault_from()` remain as internal helpers. `resolve_vault` becomes the primary entry point for all vault resolution in the CLI.

**Acceptance criteria:**
- `resolve_vault(Some("/tmp/my-vault/.parc"))` returns that path if valid.
- `resolve_vault(None)` with `PARC_VAULT=/tmp/my-vault/.parc` returns that path.
- `resolve_vault(None)` with no env var falls back to `discover_vault()` behavior.
- Explicit path takes precedence over `PARC_VAULT`.
- `PARC_VAULT` takes precedence over local discovery.
- Invalid explicit path returns `VaultNotFound`.

**Estimated effort:** 1–2 hours.

---

## Phase 1: Global `--vault` Flag

### T1.1 — Add `--vault` to CLI and thread vault path through all commands

Add a global `--vault <path>` argument to the `Cli` struct. Resolve the vault once in `main()` and pass the resolved `PathBuf` to every command.

**Changes to `parc-cli/src/main.rs`:**

```rust
#[derive(Parser)]
#[command(name = "parc", about = "Personal Archive — structured fragments of thought")]
struct Cli {
    /// Path to vault (overrides PARC_VAULT and vault discovery)
    #[arg(global = true, long)]
    vault: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}
```

**Resolution in `main()`:**
- For `init`: vault resolution is special — the vault may not exist yet. Pass `cli.vault` directly to the init command, which handles creation.
- For all other commands: call `resolve_vault(cli.vault.as_deref())` once, then pass the resolved `PathBuf` to the command's `run()` function.

**Changes to all command `run()` signatures:**

Every command currently calls `discover_vault()` internally. Update all 14 commands to accept `vault: &Path` as their first parameter instead:

| Command | Current signature (example) | New signature |
|---------|---------------------------|---------------|
| `init` | `run(global: bool)` | `run(global: bool, explicit_vault: Option<&Path>)` |
| `new` | `run(type_name, title, ...)` | `run(vault: &Path, type_name, title, ...)` |
| `list` | `run(type_name, status, ...)` | `run(vault: &Path, type_name, status, ...)` |
| `show` | `run(id, json)` | `run(vault: &Path, id, json)` |
| `edit` | `run(id)` | `run(vault: &Path, id)` |
| `set` | `run(id, field, value)` | `run(vault: &Path, id, field, value)` |
| `search` | `run(query, ...)` | `run(vault: &Path, query, ...)` |
| `delete` | `run(id)` | `run(vault: &Path, id)` |
| `link` | `run(id_a, id_b)` | `run(vault: &Path, id_a, id_b)` |
| `unlink` | `run(id_a, id_b)` | `run(vault: &Path, id_a, id_b)` |
| `backlinks` | `run(id, json)` | `run(vault: &Path, id, json)` |
| `doctor` | `run(json)` | `run(vault: &Path, json)` |
| `reindex` | `run()` | `run(vault: &Path)` |
| `types` | `run()` | `run(vault: &Path)` |

**Files to change:**
- `parc-cli/src/main.rs` — add `--vault` flag, resolve once, pass to all commands
- `parc-cli/src/commands/init.rs`
- `parc-cli/src/commands/new.rs`
- `parc-cli/src/commands/list.rs`
- `parc-cli/src/commands/show.rs`
- `parc-cli/src/commands/edit.rs`
- `parc-cli/src/commands/set.rs`
- `parc-cli/src/commands/search.rs`
- `parc-cli/src/commands/delete.rs`
- `parc-cli/src/commands/link.rs`
- `parc-cli/src/commands/unlink.rs`
- `parc-cli/src/commands/backlinks.rs`
- `parc-cli/src/commands/doctor.rs`
- `parc-cli/src/commands/reindex.rs`
- `parc-cli/src/commands/types.rs`

**Acceptance criteria:**
- `parc --vault /tmp/test-vault/.parc list` uses the specified vault.
- `parc list` (no flag) behaves identically to current behavior.
- All 14 commands accept and use the resolved vault path.
- No command calls `discover_vault()` internally anymore (except `init`, which has special handling).

**Estimated effort:** 3–4 hours.

---

## Phase 2: `parc vault` and `parc vault list` Commands

### T2.1 — `VaultInfo` struct and `vault_info()` in `parc-core`

Add a function to query metadata about a vault.

**Data structures (in `vault.rs`):**

```rust
#[derive(Debug)]
pub enum VaultScope {
    Local,
    Global,
}

#[derive(Debug)]
pub struct VaultInfo {
    pub path: PathBuf,
    pub scope: VaultScope,
    pub fragment_count: usize,
}
```

**Functions:**

```rust
/// Returns metadata about a vault: path, scope, and fragment count.
pub fn vault_info(vault_path: &Path) -> Result<VaultInfo, ParcError>
```

**Scope detection:**
- If the vault path equals `global_vault_path()`, scope is `Global`.
- Otherwise, scope is `Local`.

**Fragment count:**
- Count `.md` files in `fragments/` directory.

**Acceptance criteria:**
- Returns correct path and fragment count.
- Correctly identifies global vs. local scope.
- Returns `VaultNotFound` for invalid paths.

**Estimated effort:** 1 hour.

---

### T2.2 — `parc vault` command

Shows information about the active vault.

**Output:**

```
Active vault: /home/alice/project/.parc
Scope:        local
Fragments:    42
```

**CLI definition:**

```rust
/// Show active vault info, or manage vaults
Vault {
    #[command(subcommand)]
    subcommand: Option<VaultCommands>,
},
```

```rust
#[derive(Subcommand)]
enum VaultCommands {
    /// List all known vaults
    List,
}
```

**Behavior:**
- `parc vault` (no subcommand) — resolves the active vault and displays its info.
- `parc vault --json` — outputs `VaultInfo` as JSON.

**Files to create/change:**
- `parc-cli/src/commands/vault.rs` — new file
- `parc-cli/src/commands/mod.rs` — add `pub mod vault;`
- `parc-cli/src/main.rs` — add `Vault` variant to `Commands` enum

**Acceptance criteria:**
- `parc vault` shows path, scope, and fragment count.
- `--json` outputs structured JSON.
- Works with `--vault` flag override.

**Estimated effort:** 1–2 hours.

---

### T2.3 — `parc vault list` command

Lists all known vaults (global + any local vault discovered from CWD).

**Output:**

```
SCOPE   PATH                          FRAGMENTS
local   /home/alice/project/.parc     42
global  /home/alice/.parc             15
```

Mark the active vault with `*`:

```
SCOPE    PATH                          FRAGMENTS
local *  /home/alice/project/.parc     42
global   /home/alice/.parc             15
```

**Core function (in `vault.rs`):**

```rust
/// Discover all known vaults: global (if exists) + local (if found from CWD).
/// Does not maintain a persistent registry — just checks known locations.
pub fn discover_all_vaults() -> Result<Vec<VaultInfo>, ParcError>
```

**Discovery logic:**
1. Check global vault path (`~/.parc`) — include if `is_vault()`.
2. Walk up from CWD looking for `.parc/` — include if found and different from global.
3. Return both (or just global if no local vault exists, or just local if no global).

**Behavior:**
- `parc vault list` — shows all known vaults.
- `parc vault list --json` — outputs as JSON array.
- Active vault (the one that would be used by commands) is marked.

**Acceptance criteria:**
- Lists global vault when it exists.
- Lists local vault when found from CWD.
- Shows both when both exist.
- Active vault indicated.
- `--json` outputs valid JSON array.

**Estimated effort:** 1–2 hours.

---

## Phase 3: Init Enhancements

### T3.1 — Support `parc init --vault <path>`

The global `--vault` flag naturally supports creating vaults at arbitrary locations. Update `init` command handling so `--vault` specifies where to create the vault.

**Behavior:**
- `parc init` — creates local vault at `$CWD/.parc` (unchanged).
- `parc init --global` — creates global vault at `~/.parc` (unchanged).
- `parc init --vault /some/path` — creates vault at `/some/path/.parc` (or at `/some/path` if it already ends in `.parc`).
- `--vault` and `--global` are mutually exclusive — error if both provided.

**Changes to `init` command:**

```rust
pub fn run(global: bool, explicit_vault: Option<&Path>) -> anyhow::Result<()> {
    let vault_path = if let Some(path) = explicit_vault {
        if path.ends_with(".parc") {
            path.to_path_buf()
        } else {
            path.join(".parc")
        }
    } else if global {
        parc_core::vault::global_vault_path()?
    } else {
        std::env::current_dir()?.join(".parc")
    };

    parc_core::vault::init_vault(&vault_path)?;
    println!("Initialized vault at {}", vault_path.display());
    Ok(())
}
```

**Acceptance criteria:**
- `parc init --vault /tmp/test` creates `/tmp/test/.parc/` with full vault structure.
- `parc init --vault /tmp/test/.parc` creates `/tmp/test/.parc/` (no double `.parc`).
- `parc init --vault /tmp/test --global` produces an error.
- After init, `parc --vault /tmp/test/.parc list` works against the new vault.

**Estimated effort:** 1 hour.

---

## Phase 4: Tests

### T4.1 — Unit and integration tests

**Unit tests (`parc-core`):**

`resolve_vault` priority chain:
- Explicit path provided → uses it (even if `PARC_VAULT` is set).
- No explicit path, `PARC_VAULT` set → uses env var.
- No explicit path, no env var, local vault exists → finds local vault.
- No explicit path, no env var, no local vault → falls back to global.
- Invalid explicit path → `VaultNotFound`.
- Invalid `PARC_VAULT` path → `VaultNotFound`.

`vault_info`:
- Returns correct fragment count.
- Returns correct scope (local vs. global).

`discover_all_vaults`:
- Finds both global and local when both exist.
- Finds only global when no local exists.
- Finds only local when no global exists.

**Integration tests (`parc-cli`):**

```bash
# --vault flag overrides discovery
parc init --vault /tmp/test-vault
parc --vault /tmp/test-vault/.parc new note --title "Test"
parc --vault /tmp/test-vault/.parc list    # → shows the fragment

# PARC_VAULT env overrides discovery
PARC_VAULT=/tmp/test-vault/.parc parc list  # → shows the fragment

# parc vault shows info
parc --vault /tmp/test-vault/.parc vault   # → shows path, scope, fragment count

# parc vault list
parc vault list                             # → lists known vaults

# --vault + --global conflict on init
parc init --vault /tmp/foo --global         # → error

# init at arbitrary path
parc init --vault /tmp/new-vault
parc --vault /tmp/new-vault/.parc types     # → works
```

**Acceptance criteria:**
- All unit tests pass.
- All integration tests pass.
- Existing M0 and M1 tests still pass (no regressions).
- `cargo clippy` clean.

**Estimated effort:** 3–4 hours.

---

## Suggested Implementation Order

| Order | Task | Depends on | Est. Hours |
|-------|------|------------|------------|
| 1 | T0.1 — `resolve_vault()` | M1 | 1.5 |
| 2 | T1.1 — Global `--vault` flag + thread vault path | T0.1 | 3.5 |
| 3 | T2.1 — `VaultInfo` and `vault_info()` | T0.1 | 1 |
| 4 | T2.2 — `parc vault` command | T1.1, T2.1 | 1.5 |
| 5 | T2.3 — `parc vault list` command | T2.2 | 1.5 |
| 6 | T3.1 — Init with `--vault` | T1.1 | 1 |
| 7 | T4.1 — Tests | all above | 3.5 |
|   | **TOTAL** | | **~14 hours** |

---

## Key Design Decisions

1. **Single resolution function**: `resolve_vault()` in core, called once in `main()`, resolved path passed to all commands. No command needs to know the priority chain.

2. **Vault list scope**: `vault list` shows global vault (if exists) + local vault (if found from CWD). No persistent registry file — keeps it simple. A registry could be added in a later milestone.

3. **Init with `--vault`**: The global `--vault` flag applies to `init` too — `parc init --vault /path` creates vault at `/path/.parc`. This replaces any need for a separate `--path` flag on `init`.

4. **Backward compatibility**: `discover_vault()` and `discover_vault_from()` stay available in the public API for library consumers (`parc-core`). The CLI exclusively uses `resolve_vault()`.

5. **No cross-vault**: Cross-vault search and cross-vault links are explicitly out of scope per PRD.

6. **Init special-casing**: `init` is the only command that doesn't use `resolve_vault()` for its primary vault path, because the vault doesn't exist yet. It receives the raw `--vault` option and constructs the path itself.

---

## Files Changed Summary

| File | Change |
|------|--------|
| `parc-core/src/vault.rs` | Add `resolve_vault()`, `vault_info()`, `VaultInfo`, `VaultScope`, `discover_all_vaults()` |
| `parc-cli/src/main.rs` | Add global `--vault` flag, resolve once, pass to commands |
| `parc-cli/src/commands/init.rs` | Accept `explicit_vault: Option<&Path>` |
| `parc-cli/src/commands/new.rs` | Accept `vault: &Path`, remove internal `discover_vault()` |
| `parc-cli/src/commands/list.rs` | Accept `vault: &Path`, remove internal `discover_vault()` |
| `parc-cli/src/commands/show.rs` | Accept `vault: &Path`, remove internal `discover_vault()` |
| `parc-cli/src/commands/edit.rs` | Accept `vault: &Path`, remove internal `discover_vault()` |
| `parc-cli/src/commands/set.rs` | Accept `vault: &Path`, remove internal `discover_vault()` |
| `parc-cli/src/commands/search.rs` | Accept `vault: &Path`, remove internal `discover_vault()` |
| `parc-cli/src/commands/delete.rs` | Accept `vault: &Path`, remove internal `discover_vault()` |
| `parc-cli/src/commands/link.rs` | Accept `vault: &Path`, remove internal `discover_vault()` |
| `parc-cli/src/commands/unlink.rs` | Accept `vault: &Path`, remove internal `discover_vault()` |
| `parc-cli/src/commands/backlinks.rs` | Accept `vault: &Path`, remove internal `discover_vault()` |
| `parc-cli/src/commands/doctor.rs` | Accept `vault: &Path`, remove internal `discover_vault()` |
| `parc-cli/src/commands/reindex.rs` | Accept `vault: &Path`, remove internal `discover_vault()` |
| `parc-cli/src/commands/types.rs` | Accept `vault: &Path`, remove internal `discover_vault()` |
| `parc-cli/src/commands/vault.rs` | **New file** — `vault` and `vault list` commands |
| `parc-cli/src/commands/mod.rs` | Add `pub mod vault;` |
| `parc-cli/tests/integration.rs` | Add M2 integration tests |

---

## Definition of Done (M2)

- [ ] `resolve_vault()` implements full priority chain: `--vault` > `PARC_VAULT` > local discovery > global fallback
- [ ] `--vault` flag works globally on all commands
- [ ] All commands accept vault path as parameter (no internal `discover_vault()` calls)
- [ ] `parc vault` shows active vault path, scope, and fragment count
- [ ] `parc vault list` shows all known vaults with active indicator
- [ ] `parc init --vault <path>` creates vault at arbitrary location
- [ ] `--vault` and `--global` on `init` are mutually exclusive
- [ ] `PARC_VAULT` env var respected when `--vault` not provided
- [ ] All existing M0/M1 tests still pass (no regressions)
- [ ] All new unit tests pass
- [ ] All new integration tests pass
- [ ] `cargo clippy` clean
