# M9 — Tauri Desktop GUI

## Features

1. **`parc-gui` crate scaffold** — New workspace member with Tauri, vanilla TypeScript, and Vite
2. **Tauri command layer** — Rust backend exposing `parc-core` operations as Tauri IPC commands, plus `render_markdown` command via comrak
3. **App shell & navigation** — Sidebar layout with hash-based router, global search, and keyboard shortcuts
4. **Fragment list view** — Browsable, filterable, sortable fragment table with type/status badges
5. **Fragment detail view** — Full fragment display with server-rendered Markdown, metadata, backlinks, and attachments
6. **Fragment editor** — In-app fragment editing with schema-driven form + contenteditable Markdown body
7. **Fragment creation** — Type-aware creation form with schema-driven fields and template loading
8. **Search with DSL** — Full DSL search bar with autocomplete, filter chips, and result highlighting
9. **Tag browser** — Tag cloud/list with counts, click-to-filter, tag management
10. **Backlink graph** — Interactive visual graph of fragment links using Canvas 2D + hand-rolled force-directed layout
11. **Attachment management** — Drag-and-drop attachment upload, inline preview (images, PDFs), detach
12. **History viewer** — Version timeline, side-by-side diff, one-click restore
13. **Vault switcher** — Multi-vault support: switch between global/local vaults, vault info panel
14. **Settings & preferences** — In-app settings UI for vault config, theme, display preferences
15. **Theming & dark mode** — Light/dark theme with system preference detection, plain CSS custom properties
16. **Keyboard-driven workflow** — Command palette (Ctrl+K), vim-style navigation hints, full keyboard accessibility
17. **Integration tests** — Tauri integration tests for command layer + manual smoke tests for frontend

**PRD refs:** §3.1 (architecture — Tauri imports parc-core directly), §3.2 (integration layers), §13 (GUI Readiness), M9 milestone definition.

**Key constraint: zero npm runtime dependencies.** The frontend uses vanilla TypeScript web components, plain CSS with custom properties, and communicates with the Rust backend exclusively through Tauri IPC. Only dev dependencies are allowed: `@tauri-apps/api`, `vite`, `typescript`.

---

## Feature 1: `parc-gui` Crate Scaffold

### Files
- `Cargo.toml` (workspace root) — Add `parc-gui` to `members`
- `parc-gui/Cargo.toml` — Tauri app Rust backend manifest
- `parc-gui/tauri.conf.json` — Tauri configuration
- `parc-gui/src/main.rs` — Tauri entry point
- `parc-gui/src/lib.rs` — Command registration and app setup
- `parc-gui/ui/` — Frontend project root (Vite + vanilla TypeScript)
- `parc-gui/ui/package.json` — Frontend dev dependencies only
- `parc-gui/ui/tsconfig.json` — TypeScript configuration
- `parc-gui/ui/vite.config.ts` — Vite configuration with Tauri plugin
- `parc-gui/ui/index.html` — HTML entry point
- `parc-gui/ui/src/main.ts` — App entry point (register custom elements, mount router)
- `parc-gui/ui/src/app-shell.ts` — `<app-shell>` root web component

### Design

Tauri v2 app with a vanilla TypeScript frontend bundled via Vite. The Rust backend is a thin wrapper around `parc-core` — all business logic stays in the library. The frontend communicates with the backend exclusively through Tauri's IPC invoke mechanism (typed via `@tauri-apps/api`).

**Zero npm runtime dependencies.** Dev dependencies only:
- `@tauri-apps/api@2` — Tauri IPC types (tree-shaken away at build, only type wrappers)
- `vite` — bundler
- `typescript` — type checking

All UI is built with **Web Components** (custom elements with Shadow DOM). State management uses a simple event bus pattern. Routing is hash-based (~50 lines).

Directory structure:

```
parc-gui/
├── Cargo.toml
├── tauri.conf.json
├── build.rs
├── icons/                  # App icons (all sizes)
├── src/
│   ├── main.rs             # Tauri entry point (#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")])
│   ├── lib.rs              # setup(), register commands
│   ├── commands/           # Tauri command modules (one per domain)
│   │   ├── mod.rs
│   │   ├── fragment.rs
│   │   ├── search.rs
│   │   ├── vault.rs
│   │   ├── schema.rs
│   │   ├── tag.rs
│   │   ├── link.rs
│   │   ├── attachment.rs
│   │   ├── history.rs
│   │   └── markdown.rs     # render_markdown command (comrak)
│   ├── state.rs            # Managed Tauri state (vault path, config cache)
│   └── error.rs            # GUI-specific error wrapper implementing Serialize
├── ui/
│   ├── package.json
│   ├── tsconfig.json
│   ├── vite.config.ts
│   ├── index.html
│   └── src/
│       ├── main.ts          # Register all custom elements, boot app
│       ├── api/             # Typed Tauri invoke wrappers
│       ├── components/      # Web components (custom elements)
│       ├── views/           # Page-level view components
│       ├── lib/             # Utility modules (router, event-bus, state, keyboard)
│       ├── types/           # TypeScript type definitions
│       └── styles/          # CSS files (imported by components)
```

### Tasks
- [ ] Add `"parc-gui"` to workspace members in root `Cargo.toml`
- [ ] Create `parc-gui/Cargo.toml`:
  - `parc-core = { path = "../parc-core" }`
  - `tauri = { version = "2", features = ["protocol-asset"] }`
  - `tauri-build` (build dependency)
  - `serde`, `serde_json` (with derive)
  - `anyhow`, `thiserror`
  - `comrak` (Markdown → HTML rendering)
- [ ] Create `parc-gui/build.rs` with `tauri_build::build()`
- [ ] Create `parc-gui/tauri.conf.json`:
  - App identifier: `com.parc.gui`
  - Window: title "parc", default size 1200×800, min size 800×500
  - Security: `dangerousRemoteDomainIpcAccess` disabled
  - Build: `beforeDevCommand` → `npm run dev`, `beforeBuildCommand` → `npm run build`
  - `devUrl` → `http://localhost:5173`
  - `frontendDist` → `../ui/dist`
- [ ] Create `parc-gui/src/main.rs` — Tauri bootstrap
- [ ] Create `parc-gui/src/lib.rs` — `run()` function with `tauri::Builder`, register all commands
- [ ] Create `parc-gui/src/state.rs`:
  - `AppState { vault_path: RwLock<PathBuf>, config: RwLock<Config> }`
  - Initialized from vault discovery at app startup
- [ ] Create `parc-gui/src/error.rs`:
  - `GuiError` enum wrapping `ParcError` + GUI-specific errors
  - Implement `serde::Serialize` for `GuiError` (required by Tauri commands)
  - Implement `From<ParcError>` for `GuiError`
- [ ] Initialize frontend project in `parc-gui/ui/`:
  - Create `package.json` with **dev dependencies only**: `@tauri-apps/api@2`, `vite`, `typescript`
  - No React, no Tailwind, no runtime npm packages
  - Create `vite.config.ts` (vanilla mode, no framework plugins)
  - Create `tsconfig.json` targeting ES2022, DOM lib
- [ ] Create `index.html` with single `<app-shell>` element
- [ ] Create `main.ts` that imports and registers all custom elements
- [ ] Create minimal `<app-shell>` web component with "parc" heading and Tauri invoke test
- [ ] Verify `cargo build -p parc-gui` compiles the Rust backend
- [ ] Verify `cargo tauri dev` launches the app with hot-reload
- [ ] Verify `cargo test --workspace` still passes

---

## Feature 2: Tauri Command Layer

### Files
- `parc-gui/src/commands/mod.rs` — Command module registry
- `parc-gui/src/commands/fragment.rs` — Fragment CRUD commands
- `parc-gui/src/commands/search.rs` — Search command
- `parc-gui/src/commands/vault.rs` — Vault info, reindex, doctor
- `parc-gui/src/commands/schema.rs` — Schema list/get
- `parc-gui/src/commands/tag.rs` — Tag listing
- `parc-gui/src/commands/link.rs` — Link/unlink/backlinks
- `parc-gui/src/commands/attachment.rs` — Attachment management
- `parc-gui/src/commands/history.rs` — History list/get/restore
- `parc-gui/src/commands/markdown.rs` — Markdown rendering via comrak
- `parc-gui/ui/src/api/index.ts` — TypeScript API wrapper barrel export
- `parc-gui/ui/src/api/fragments.ts` — Fragment API calls
- `parc-gui/ui/src/api/search.ts` — Search API calls
- `parc-gui/ui/src/api/vault.ts` — Vault API calls
- `parc-gui/ui/src/api/types.ts` — Shared TypeScript types mirroring Rust structs

### Design

Each Tauri command is a thin `#[tauri::command]` async function that reads `AppState`, calls `parc-core`, and returns a serializable result. The frontend gets typed wrappers around `invoke()` calls. All commands follow the pattern:

```rust
#[tauri::command]
async fn fragment_get(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<FragmentDto, GuiError> {
    let vault = state.vault_path.read().await;
    let fragment = parc_core::fragment::read_fragment(&vault, &id)?;
    Ok(FragmentDto::from(fragment))
}
```

**DTO layer:** Tauri commands return `*Dto` structs (`FragmentDto`, `SearchResultDto`, etc.) that implement `Serialize` and are purpose-built for the frontend. These map cleanly from `parc-core` types but may flatten or reshape data for UI convenience (e.g., `extra_fields` merged into a flat object).

**Markdown rendering command:** A dedicated `render_markdown` command uses comrak to convert Markdown body text to HTML on the Rust side. This replaces any frontend Markdown library. Wiki-links and attachment references are resolved during rendering.

```rust
#[tauri::command]
async fn render_markdown(
    state: tauri::State<'_, AppState>,
    markdown: String,
) -> Result<String, GuiError> {
    let vault = state.vault_path.read().await;
    let html = parc_core::markdown::render_to_html(&markdown, &vault)?;
    Ok(html)
}
```

### Tasks
- [ ] Create `commands/mod.rs` exporting all command functions
- [ ] Define DTO types in a shared location (`parc-gui/src/dto.rs`):
  - `FragmentDto` — full fragment with all fields flattened, body included
  - `FragmentSummaryDto` — compact version for lists (no body)
  - `SearchResultDto` — search result with snippet
  - `SchemaDto` — schema with fields
  - `TagCountDto` — tag name + count
  - `VaultInfoDto` — vault path, fragment count, type counts
  - `VersionEntryDto` — history version timestamp + size
  - `AttachmentInfoDto` — filename + size
  - `DoctorReportDto` — findings list
  - `BacklinkDto` — linking fragment summary
- [ ] Implement `commands/fragment.rs`:
  - `fragment_list(state, type_filter?, status?, tag?, limit?, sort?) -> Vec<FragmentSummaryDto>`
  - `fragment_get(state, id) -> FragmentDto`
  - `fragment_create(state, type_name, title, tags?, body?, extra_fields?) -> FragmentDto`
  - `fragment_update(state, id, title?, tags?, body?, extra_fields?) -> FragmentDto`
  - `fragment_delete(state, id) -> bool`
  - `fragment_archive(state, id) -> bool`
  - `fragment_restore_from_trash(state, id) -> bool`
- [ ] Implement `commands/search.rs`:
  - `search(state, query, limit?, sort?) -> Vec<SearchResultDto>`
- [ ] Implement `commands/vault.rs`:
  - `vault_info(state) -> VaultInfoDto`
  - `vault_reindex(state) -> usize`
  - `vault_doctor(state) -> DoctorReportDto`
  - `vault_switch(state, path) -> VaultInfoDto` — update `AppState` vault path
  - `vault_list(state) -> Vec<VaultInfoDto>`
  - `vault_init(state, path, global: bool) -> VaultInfoDto`
- [ ] Implement `commands/schema.rs`:
  - `schema_list(state) -> Vec<SchemaDto>`
  - `schema_get(state, type_name) -> SchemaDto`
- [ ] Implement `commands/tag.rs`:
  - `tags_list(state) -> Vec<TagCountDto>`
- [ ] Implement `commands/link.rs`:
  - `link_fragments(state, id_a, id_b) -> bool`
  - `unlink_fragments(state, id_a, id_b) -> bool`
  - `backlinks(state, id) -> Vec<BacklinkDto>`
- [ ] Implement `commands/attachment.rs`:
  - `attach_file(state, fragment_id, file_path) -> AttachmentInfoDto`
  - `detach_file(state, fragment_id, filename) -> bool`
  - `list_attachments(state, fragment_id) -> Vec<AttachmentInfoDto>`
  - `get_attachment_path(state, fragment_id, filename) -> String` — returns absolute path for display
- [ ] Implement `commands/history.rs`:
  - `history_list(state, id) -> Vec<VersionEntryDto>`
  - `history_get(state, id, timestamp) -> FragmentDto`
  - `history_restore(state, id, timestamp) -> FragmentDto`
  - `history_diff(state, id, timestamp?) -> DiffDto` — returns structured diff (added/removed lines)
- [ ] Implement `commands/markdown.rs`:
  - `render_markdown(state, markdown) -> String` — Markdown → HTML via comrak
  - Resolves wiki-links (`[[id]]`, `[[id|text]]`) to `<a>` tags with fragment titles
  - Resolves attachment refs (`![[attach:file]]`) to asset protocol URLs or `<img>` tags
  - Resolves inline `#hashtags` to clickable links
  - Uses comrak with GFM extensions (tables, strikethrough, task lists, autolinks)
- [ ] Register all commands in `lib.rs` via `tauri::Builder::default().invoke_handler(tauri::generate_handler![...])`
- [ ] Create TypeScript type definitions in `ui/src/api/types.ts` mirroring all DTOs
- [ ] Create typed invoke wrappers in `ui/src/api/`:
  - `fragments.ts` — `getFragment(id)`, `listFragments(opts)`, `createFragment(data)`, etc.
  - `search.ts` — `search(query, opts)`
  - `vault.ts` — `getVaultInfo()`, `reindex()`, `switchVault(path)`, etc.
  - `tags.ts` — `listTags()`
  - `links.ts` — `linkFragments(a, b)`, `getBacklinks(id)`, etc.
  - `attachments.ts` — `attachFile(id, path)`, `listAttachments(id)`, etc.
  - `history.ts` — `listHistory(id)`, `getVersion(id, ts)`, `restoreVersion(id, ts)`, `diffVersion(id, ts)`
  - `markdown.ts` — `renderMarkdown(text)`
- [ ] Unit tests for each command: mock vault with temp dir, call command, verify DTO output

---

## Feature 3: App Shell & Navigation

### Files
- `parc-gui/ui/src/app-shell.ts` — `<app-shell>` root web component with layout
- `parc-gui/ui/src/components/side-bar.ts` — `<side-bar>` navigation component
- `parc-gui/ui/src/components/top-bar.ts` — `<top-bar>` with search and actions
- `parc-gui/ui/src/components/command-palette.ts` — `<command-palette>` custom element
- `parc-gui/ui/src/lib/router.ts` — Hash-based router (~50 lines)
- `parc-gui/ui/src/lib/keyboard.ts` — Global keyboard shortcut handler
- `parc-gui/ui/src/lib/event-bus.ts` — Simple pub/sub event bus for state coordination
- `parc-gui/ui/src/lib/state.ts` — App state objects (navigation, preferences)
- `parc-gui/ui/src/styles/global.css` — Base styles, CSS custom properties

### Design

Three-column layout that adapts responsively:

```
┌─────────────────────────────────────────────────────────┐
│  TopBar: [🔍 Search...              ] [+ New ▼] [⚙]   │
├──────────┬──────────────────────┬───────────────────────┤
│ Sidebar  │   List / Grid        │   Detail Panel        │
│          │                      │                       │
│ ▸ All    │   fragment cards     │   metadata            │
│ ▸ Notes  │   or table rows      │   rendered body       │
│ ▸ Todos  │                      │   backlinks           │
│ ▸ Deci…  │                      │   attachments         │
│ ▸ Risks  │                      │   history             │
│ ▸ Ideas  │                      │                       │
│ ──────── │                      │                       │
│ ▸ Tags   │                      │                       │
│ ▸ Graph  │                      │                       │
│ ──────── │                      │                       │
│ ▸ Trash  │                      │                       │
│ ──────── │                      │                       │
│  Vault:  │                      │                       │
│  ~/.parc │                      │                       │
└──────────┴──────────────────────┴───────────────────────┘
```

The sidebar collapses to icons on narrow windows. The detail panel shows/hides based on whether a fragment is selected. On very narrow windows, the layout becomes single-column with back navigation.

**Router:** A minimal hash-based router (`lib/router.ts`) that maps `#/path` to view components. No npm dependency — just `hashchange` listener + a route table. ~50 lines of code.

**State management:** Simple state objects + an event bus (`lib/event-bus.ts`). Components subscribe to state changes via `bus.on('fragments:updated', callback)`. No Zustand, no Redux — just plain JS objects and CustomEvents.

### Tasks
- [ ] Create `lib/router.ts`:
  - Hash-based routing: listen to `hashchange`, match routes, swap view components in a `<main>` slot
  - Routes: `#/` (list), `#/fragment/:id` (detail), `#/search` (search results), `#/tags` (tag browser), `#/graph` (backlink graph), `#/settings` (settings), `#/trash` (trash view)
  - `navigate(path)` helper that sets `location.hash`
  - Route params extraction (`:id` → `params.id`)
- [ ] Create `lib/event-bus.ts`:
  - `on(event, callback)`, `off(event, callback)`, `emit(event, data)`
  - Typed events: `fragments:updated`, `fragment:selected`, `vault:switched`, `theme:changed`, `search:submit`, `navigate`
  - Singleton instance exported as `bus`
- [ ] Create `lib/state.ts`:
  - `navState: { selectedFragmentId, sidebarCollapsed, detailPanelOpen, currentTypeFilter }`
  - `prefState: { theme, listViewMode, editorPreviewVisible, fontSize }` (persisted to `localStorage`)
  - Mutations emit events via bus
- [ ] Create `<app-shell>` component:
  - Shadow DOM with three-column CSS grid layout
  - Slots: `<side-bar>`, `<main>` (router outlet), detail panel (conditional)
  - Listens to router changes, swaps view components in main area
- [ ] Create `<side-bar>` component:
  - Navigation sections: fragment types (All, Note, Todo, Decision, Risk, Idea), tools (Tags, Graph), management (Trash)
  - Active route highlighting via `hashchange` listener
  - Fragment count badges per type (fetched from `vault_info`)
  - Vault name/path at bottom with switch button
  - Collapsible to icon-only mode
- [ ] Create `<top-bar>` component:
  - Global search input (triggers DSL search on Enter, navigates to `#/search?q=...`)
  - "+ New" button with dropdown for fragment type selection
  - Settings gear icon
  - Vault status indicator
- [ ] Create `<command-palette>` custom element:
  - Triggered by `Ctrl+K` / `Cmd+K`
  - Modal overlay with search input + filtered action list
  - Actions: New fragment (by type), search, switch vault, reindex, navigate to any view
  - Recent fragments as suggestions
  - Fuzzy matching on fragment titles (simple `includes()` + score by position)
  - Keyboard navigation: up/down arrows, Enter to select, Escape to close
- [ ] Create `lib/keyboard.ts`:
  - Global shortcuts: `Ctrl+K` (command palette), `Ctrl+N` (new fragment), `Ctrl+F` (focus search), `Escape` (close panels/modals)
  - Navigation: `j/k` for list movement when not in an input, `Enter` to open selected
  - Registers on `document.addEventListener('keydown', ...)`
- [ ] Create `styles/global.css`:
  - CSS custom properties for theme colors (see Feature 15)
  - Typography defaults for rendered Markdown (`.rendered-md` class)
  - Scrollbar styling
  - Transition defaults
  - CSS reset / base styles

---

## Feature 4: Fragment List View

### Files
- `parc-gui/ui/src/views/fragment-list.ts` — `<fragment-list>` view component
- `parc-gui/ui/src/components/fragment-card.ts` — `<fragment-card>` for grid view
- `parc-gui/ui/src/components/fragment-row.ts` — `<fragment-row>` for table view
- `parc-gui/ui/src/components/type-badge.ts` — `<type-badge>` colored pill
- `parc-gui/ui/src/components/status-badge.ts` — `<status-badge>` colored pill
- `parc-gui/ui/src/components/tag-chip.ts` — `<tag-chip>` clickable tag
- `parc-gui/ui/src/components/filter-bar.ts` — `<filter-bar>` active filter chips

### Design

The list view is the primary view. It supports two display modes (table and cards) and pulls data from the Tauri backend via the `fragment_list` command. Filters are applied via hash query parameters so they can be deep-linked from the sidebar.

Type badges use distinct colors:
- Note: blue
- Todo: amber
- Decision: purple
- Risk: red
- Idea: green

Status badges use semantic colors:
- Open/identified/raw/proposed: neutral gray
- In-progress/exploring/mitigating: blue
- Done/accepted/resolved/promoted: green
- Cancelled/deprecated/discarded/parked: muted/strikethrough

### Tasks
- [ ] Create `<fragment-list>` view:
  - Fetch fragments via `listFragments()` with current filters
  - Toggle between table view and card grid view (persisted in `localStorage`)
  - Column sort headers: title, type, status, updated, created, priority, due
  - Empty state: "No fragments found" with create button
  - Loading skeleton while fetching
  - Click row/card to select fragment (navigates to `#/fragment/:id`)
  - Right-click context menu (plain `<div>` positioned on `contextmenu` event): Edit, Archive, Delete, Copy ID
- [ ] Create `<fragment-card>`:
  - Type badge (colored), title, truncated body preview (first 2 lines), tags, date
  - Status indicator
  - Due date with overdue highlighting (red if past due)
- [ ] Create `<fragment-row>`:
  - Table row: short ID, type badge, status badge, title, tags (truncated), updated date
  - Priority indicator for todos (colored dot)
  - Due date with overdue highlighting
- [ ] Create `<type-badge>` — small colored pill with type name, color set via attribute
- [ ] Create `<status-badge>` — status pill with semantic color
- [ ] Create `<tag-chip>` — clickable chip that adds tag filter, removable when active
- [ ] Create `<filter-bar>`:
  - Shows active filters as removable chips
  - Quick-add dropdown for common filters (type, status, tag, priority)
  - "Clear all" button when filters active
- [ ] Create fragment list state in `lib/state.ts`:
  - `fragmentState: { fragments, loading, filters: { type?, status?, tags[], priority?, sort, limit }, viewMode }`
  - `fetchFragments()` — calls API, emits `fragments:updated`
  - `setFilter()`, `clearFilters()`, `toggleViewMode()`
  - Auto-refetch on filter change (debounced for rapid changes)

---

## Feature 5: Fragment Detail View

### Files
- `parc-gui/ui/src/views/fragment-detail.ts` — `<fragment-detail>` view component
- `parc-gui/ui/src/components/metadata-panel.ts` — `<metadata-panel>` structured metadata
- `parc-gui/ui/src/components/rendered-body.ts` — `<rendered-body>` displays HTML from `render_markdown` command
- `parc-gui/ui/src/components/backlinks-section.ts` — `<backlinks-section>` list
- `parc-gui/ui/src/components/attachments-section.ts` — `<attachments-section>` with previews
- `parc-gui/ui/src/components/fragment-actions.ts` — `<fragment-actions>` toolbar

### Design

The detail view appears in the right panel when a fragment is selected, or as a full-page view when navigated to directly. It shows everything about a fragment: metadata, rendered body, backlinks, and attachments.

**Markdown rendering:** The body is sent to the Rust backend via the `render_markdown` Tauri command, which uses comrak to produce HTML. The returned HTML is set as `innerHTML` of the `<rendered-body>` component. Wiki-links and attachment references are resolved server-side during rendering.

Wiki-links (`[[id]]` and `[[id|text]]`) in the rendered HTML are `<a>` tags with `data-fragment-id` attributes. The `<rendered-body>` component adds click handlers that call `navigate()`. Attachment references (`![[attach:filename]]`) are rendered as `<img>` tags (for images) or download links using Tauri's asset protocol.

### Tasks
- [ ] Create `<fragment-detail>` view:
  - Fetch full fragment via `getFragment(id)` on mount/id change
  - Layout: metadata panel at top, rendered body, backlinks section, attachments section
  - Loading skeleton
  - 404 state for invalid IDs
  - Sticky action toolbar at top
- [ ] Create `<metadata-panel>`:
  - Display: type badge, status badge, full ID (copyable on click), title
  - Fields grid: created_at, updated_at, created_by, due, priority, assignee (conditional on type)
  - Tags as clickable `<tag-chip>` elements
  - Links as clickable fragment references (show title on hover via `title` attribute)
  - Extra fields displayed dynamically based on schema
- [ ] Create `<rendered-body>`:
  - Calls `renderMarkdown(body)` Tauri command, sets result as `innerHTML`
  - Intercepts clicks on `a[data-fragment-id]` → calls `navigate('#/fragment/' + id)`
  - Intercepts clicks on `a[data-tag]` → navigates to tag-filtered list
  - Styled via `.rendered-md` class in global CSS (typography, code blocks, tables, lists)
  - Re-renders when fragment body changes
- [ ] Create `<backlinks-section>`:
  - Fetch via `getBacklinks(id)`
  - List of linking fragments: type badge, title, snippet of linking context
  - Click to navigate to linking fragment
  - "No backlinks" empty state
- [ ] Create `<attachments-section>`:
  - Fetch via `listAttachments(id)`
  - Grid of attachment cards: filename, size, type icon
  - Image attachments: thumbnail preview via Tauri asset protocol
  - Click to open in system viewer (via Tauri shell open)
  - Detach button with confirmation
- [ ] Create `<fragment-actions>`:
  - Buttons: Edit (navigates to editor view), Delete (confirmation dialog), Archive
  - Copy ID button, Copy markdown link button
  - History button (opens history viewer)
  - More menu: Export as JSON, Export as Markdown

---

## Feature 6: Fragment Editor

### Files
- `parc-gui/ui/src/views/fragment-editor.ts` — `<fragment-editor>` view component
- `parc-gui/ui/src/components/frontmatter-form.ts` — `<frontmatter-form>` schema-driven metadata form
- `parc-gui/ui/src/components/markdown-input.ts` — `<markdown-input>` contenteditable body editor
- `parc-gui/ui/src/components/editor-preview.ts` — `<editor-preview>` live Markdown preview pane

### Design

The editor is a split-pane view: structured form fields on top (driven by the fragment's schema), contenteditable body editor below with optional side-by-side live preview. The frontmatter form renders appropriate inputs based on field types (text, enum dropdown, date picker, tag multiselect).

**Body editor:** A `contenteditable` div with a formatting toolbar. The user writes plain Markdown text. The toolbar inserts Markdown syntax (e.g., `**bold**`, `## heading`). The editor works with plain text, not rich text — it's a lightweight Markdown-aware textarea replacement. For users who prefer a plain `<textarea>`, a toggle switches between contenteditable and textarea mode.

Changes are tracked locally and saved explicitly via Ctrl+S or a Save button. An auto-save draft is stored in `localStorage` to prevent data loss on accidental close.

```
┌──────────────────────────────────────────────────────┐
│  [← Back]  Edit: "Fragment Title"    [Save] [Cancel] │
├──────────────────────────────────────────────────────┤
│  Title:  [_____________________________]             │
│  Status: [open ▼]   Priority: [medium ▼]            │
│  Due:    [2026-03-01 📅]   Assignee: [_________]     │
│  Tags:   [backend] [search] [+ add]                 │
│  Links:  [[01JQ7V4Y]] [+ add link]                  │
├──────────────────────────────────────────────────────┤
│  Body (Markdown)          │  Preview                 │
│  ┌──────────────────────┐ │ ┌──────────────────────┐ │
│  │ [B][I][H][Link][Code]│ │ │                      │ │
│  │ ## Context            │ │ │ Context              │ │
│  │                       │ │ │                      │ │
│  │ We need fast full-   │ │ │ We need fast full-   │ │
│  │ text search across   │ │ │ text search across   │ │
│  │ all fragments...     │ │ │ all fragments...     │ │
│  └──────────────────────┘ │ └──────────────────────┘ │
└──────────────────────────────────────────────────────┘
```

### Tasks
- [ ] Create `<fragment-editor>` view:
  - Load fragment data on mount (or empty template for new fragments)
  - Top bar: back button, fragment title, Save (Ctrl+S) and Cancel buttons
  - Unsaved changes indicator (dot or asterisk in title)
  - Warn on navigation away with unsaved changes (`beforeunload` + router guard)
  - On save: call `fragment_update`, show success toast, navigate back to detail
  - Validation errors displayed inline (red borders on invalid fields)
- [ ] Create `<frontmatter-form>`:
  - Fetch schema via `schema_get(type)` to determine available fields
  - Render fields dynamically based on `FieldType`:
    - `String` → text input
    - `Enum` → `<select>` dropdown with allowed values
    - `Date` → `<input type="date">`
    - `ListOfStrings` → multi-input (for deciders, etc.)
  - Tags field: comma-separated input with typeahead from existing tags (`tags_list`) via `<datalist>`
  - Links field: multi-input with ID prefix search autocomplete
  - Title field: always visible, required, auto-focus on new fragments
- [ ] Create `<markdown-input>`:
  - `contenteditable` div for Markdown body text (plain text mode, not rich text)
  - Formatting toolbar: bold (`**`), italic (`_`), heading (`##`), link (`[]()`), code (`` ` ``), bullet list, numbered list
  - Toolbar buttons insert Markdown syntax around selection or at cursor
  - Tab key inserts 2 spaces (not focus change)
  - Toggle between `contenteditable` and plain `<textarea>` mode
  - Emits `input` events with current text content
  - Monospace font, preserves whitespace
- [ ] Create `<editor-preview>`:
  - Calls `renderMarkdown(text)` Tauri command for live preview
  - Debounced rendering (200ms after last keystroke)
  - Uses same `.rendered-md` styles as detail view
  - Toggle show/hide preview (default: show)
- [ ] Auto-save draft logic (in `<fragment-editor>`):
  - Stores draft to `localStorage` keyed by fragment ID (or `"new-<type>"` for creation)
  - Saves every 5 seconds if dirty
  - Clears draft on explicit save or discard
  - Prompts to restore draft if found on editor open

---

## Feature 7: Fragment Creation

### Files
- `parc-gui/ui/src/views/fragment-create.ts` — `<fragment-create>` creation flow
- `parc-gui/ui/src/components/type-selector.ts` — `<type-selector>` type picker

### Design

Fragment creation reuses the editor components (Feature 6) but starts from a template. The flow is:

1. User clicks "+ New" → type selector appears (modal or dropdown)
2. User picks a type → editor opens with template loaded and schema fields pre-configured
3. User fills in fields and body → clicks Save
4. Backend creates fragment → navigates to new fragment's detail view

### Tasks
- [ ] Create `<type-selector>`:
  - Grid of type cards: icon, name, description, field summary
  - Keyboard navigable (arrow keys + Enter)
  - Shows all types from `schema_list` (built-in + custom)
- [ ] Create `<fragment-create>` view:
  - Type selection step (if type not pre-selected from sidebar)
  - Delegates to `<fragment-editor>` in create mode (reuses same component)
  - Loads template body from schema/template
  - Pre-fills: type, default status, default priority, default_tags from config
  - Pre-selects type if navigated from sidebar (e.g., clicking "+ Todo" pre-selects todo type)
  - On save: calls `fragment_create`, shows success toast with ID, navigates to detail view

---

## Feature 8: Search with DSL

### Files
- `parc-gui/ui/src/views/search-results.ts` — `<search-results>` view
- `parc-gui/ui/src/components/search-bar.ts` — `<search-bar>` with autocomplete
- `parc-gui/ui/src/components/search-suggestions.ts` — `<search-suggestions>` dropdown
- `parc-gui/ui/src/components/filter-chips.ts` — `<filter-chips>` visual DSL chips
- `parc-gui/ui/src/lib/dsl.ts` — Client-side DSL token parsing for UI assistance

### Design

The search bar is always visible in the top bar. It accepts the full parc search DSL. As the user types, the UI provides:

1. **Syntax assistance** — recognized filter prefixes (`type:`, `status:`, etc.) are highlighted with distinct colors via overlaid `<span>` elements
2. **Value suggestions** — after typing `type:`, show available types; after `tag:`, show existing tags; after `status:`, show valid statuses for the current type filter
3. **Recent searches** — shown when the search bar is focused and empty
4. **Live results** — results update as the user types (debounced 300ms)

### Tasks
- [ ] Create `<search-bar>`:
  - Input with DSL syntax highlighting (colored `<span>` overlay positioned behind transparent input)
  - Focus with `Ctrl+F` or clicking
  - Submit on Enter (navigates to `#/search?q=...`)
  - Clear button (×) when non-empty
  - Dropdown overlay for suggestions while typing
- [ ] Create `<search-suggestions>`:
  - Context-aware suggestions based on cursor position in query:
    - Empty query → recent searches + "Try: type:todo status:open"
    - After `type:` → list of types from schema
    - After `status:` → list of statuses (contextual to type if `type:` filter present)
    - After `tag:` or `#` → existing tags from `tags_list`
    - After `priority:` → low, medium, high, critical
    - After `due:` → today, this-week, overdue, tomorrow
    - After `by:` → known users
    - After `has:` → attachments, links, due
  - Keyboard navigation: up/down arrows, Enter to select, Escape to close
- [ ] Create `<filter-chips>`:
  - Parse current query into visual chips (e.g., `type:todo` → blue chip "Type: todo")
  - Each chip has × button to remove that filter term
  - Click chip to edit that filter value
- [ ] Create `<search-results>` view:
  - Calls `search(query)` and displays results
  - Reuses `<fragment-row>` / `<fragment-card>` from list view
  - Highlights matching text in title and body snippet
  - Shows result count and query execution time
  - Empty state: "No results for <query>" with suggestions
- [ ] Create `lib/dsl.ts` utility:
  - `tokenize(query: string) -> Token[]` — splits query into filter tokens and text terms
  - `Token = { type: 'filter' | 'text' | 'hashtag' | 'phrase', value: string, filterName?: string }`
  - Used for syntax highlighting and suggestion context (client-side only, actual parsing is in `parc-core`)
- [ ] Search state and debouncing (in `lib/state.ts` or inline in `<search-bar>`):
  - Debounced search (300ms after last keystroke)
  - Search history stored in `localStorage` (last 20 queries)
  - Abort previous in-flight search when new query arrives (via AbortController or flag)

---

## Feature 9: Tag Browser

### Files
- `parc-gui/ui/src/views/tag-browser.ts` — `<tag-browser>` view
- `parc-gui/ui/src/components/tag-cloud.ts` — `<tag-cloud>` visual cloud
- `parc-gui/ui/src/components/tag-list-view.ts` — `<tag-list-view>` sortable table

### Design

The tag browser shows all tags (merged frontmatter + inline) with their usage counts. Two view modes: a visual cloud (font size proportional to count) and a sortable table. Clicking a tag navigates to a filtered fragment list.

### Tasks
- [ ] Create `<tag-browser>` view:
  - Fetch tags via `listTags()`
  - Toggle between cloud and list views
  - Search/filter input to narrow tags
  - Sort options: alphabetical, by count (asc/desc)
- [ ] Create `<tag-cloud>`:
  - Tags sized proportionally to usage count (min 0.8rem, max 2.5rem, via `calc()`)
  - Colored by most common fragment type using that tag
  - Hover shows exact count (via `title` attribute)
  - Click navigates to `#/?tag=<name>`
- [ ] Create `<tag-list-view>`:
  - HTML `<table>`: tag name, count, fragment type breakdown
  - Sortable columns (click header to toggle sort)
  - Click tag → navigate to filtered fragment list

---

## Feature 10: Backlink Graph

### Files
- `parc-gui/ui/src/views/graph-view.ts` — `<graph-view>` full-page graph view
- `parc-gui/ui/src/components/fragment-graph.ts` — `<fragment-graph>` Canvas 2D component
- `parc-gui/ui/src/lib/force-layout.ts` — Hand-rolled force-directed layout algorithm

### Design

Interactive force-directed graph showing fragments as nodes and links as edges. Rendered on a **Canvas 2D** context with a hand-rolled force-directed layout algorithm. No external graph library. Nodes are colored by fragment type and sized by link count. The graph can be filtered by type, tag, or a root fragment (show N-hop neighborhood).

**Force-directed layout (`lib/force-layout.ts`):** Implements a simple velocity Verlet integration with:
- Repulsive force between all node pairs (Coulomb's law, `k / d²`)
- Attractive force along edges (Hooke's law, spring constant)
- Center gravity to keep the graph from drifting
- Velocity damping to reach equilibrium
- Runs on `requestAnimationFrame`, stops when energy drops below threshold

```
     ┌──────┐         ┌──────┐
     │ Note │────────▶│ Todo │
     │  A   │         │  B   │
     └──┬───┘         └──────┘
        │
        ▼
     ┌──────┐         ┌──────────┐
     │ Dec  │◀───────│ Decision  │
     │  C   │         │    D     │
     └──────┘         └──────────┘
```

### Tasks
- [ ] Create `lib/force-layout.ts`:
  - `ForceLayout` class taking `nodes: { id, x, y, vx, vy, mass }[]` and `edges: { source, target }[]`
  - `tick()` — one simulation step: compute forces, update velocities + positions
  - `run(onTick)` — runs via `requestAnimationFrame` until energy < threshold, calls `onTick` each frame
  - `stop()` — halts simulation
  - Configurable parameters: repulsion strength, spring length, spring stiffness, damping, gravity
  - Node pinning: `pin(nodeId)` freezes a node in place (for dragging)
- [ ] Create `<fragment-graph>` component:
  - `<canvas>` element, sized to fill container
  - Render loop: clear canvas, draw edges (lines with optional arrows), draw nodes (colored circles by type, label with title)
  - Hit testing: on `mousemove`, check distance to nodes for hover state; on `click`, select node
  - Drag support: `mousedown` on node → pin node, `mousemove` → update position, `mouseup` → unpin
  - Zoom/pan: `wheel` event for zoom, `mousedown` on canvas background + `mousemove` for pan
  - Transform matrix for zoom/pan (`ctx.setTransform()`)
  - Selected node emits event → shows mini detail panel or navigates
  - Double-click node → navigate to `#/fragment/:id`
- [ ] Create `<graph-view>` view:
  - Full-page graph with control panel overlay (positioned absolute)
  - Controls: type filter checkboxes, tag filter input, depth slider (1-5 hops), reset layout button
  - "Focus on fragment" input — centers graph on a specific fragment's neighborhood
  - Fragment count display
  - Fetch all fragments with links, transform into nodes + edges
  - Support filtering: by type, by tag, by neighborhood (N hops from a root fragment)

---

## Feature 11: Attachment Management

### Files
- `parc-gui/ui/src/components/attachment-uploader.ts` — `<attachment-uploader>` drop zone
- `parc-gui/ui/src/components/attachment-preview.ts` — `<attachment-preview>` inline preview
- `parc-gui/ui/src/components/attachment-grid.ts` — `<attachment-grid>` thumbnail grid

### Design

Attachments can be added via drag-and-drop onto the editor or detail view, or via a file picker dialog. Images show inline previews. Other files show type-appropriate icons. Tauri's asset protocol serves attachment files directly to the webview.

### Tasks
- [ ] Create `<attachment-uploader>`:
  - Drop zone with visual feedback (border highlight on `dragover`)
  - Accept any file type
  - On drop: call `attach_file` with the file path (Tauri gives us the path via drag event)
  - Spinner while uploading
  - Multiple file drop support
  - Also accessible via "Browse files" button using Tauri's dialog API (`@tauri-apps/api/dialog`)
- [ ] Create `<attachment-preview>`:
  - Image files (png, jpg, gif, svg, webp): render `<img>` via Tauri asset protocol URL
  - PDF: link to open externally
  - Text/code files: `<pre>` block with first 50 lines
  - Other: file icon + metadata (name, size, type)
  - Click to open in system default app
- [ ] Create `<attachment-grid>`:
  - CSS grid layout of attachment cards with thumbnails
  - Hover reveals: filename, size, detach button
  - Lightbox mode for images (click to expand full-size in modal overlay)

---

## Feature 12: History Viewer

### Files
- `parc-gui/ui/src/views/history-view.ts` — `<history-view>` fragment history view
- `parc-gui/ui/src/components/version-timeline.ts` — `<version-timeline>` vertical timeline
- `parc-gui/ui/src/components/diff-viewer.ts` — `<diff-viewer>` side-by-side diff display

### Design

The history viewer shows all saved versions of a fragment as a vertical timeline. Selecting a version shows a side-by-side diff against the current version (or any other version). One-click restore creates a new version from the historical snapshot.

### Tasks
- [ ] Create `<version-timeline>`:
  - Vertical timeline with version entries: timestamp, relative time ("2 hours ago"), size
  - Current version at top (marked as "Current")
  - Click to select a version for viewing/diffing
  - Active version highlighted
- [ ] Create `<diff-viewer>`:
  - Side-by-side view: old version (left) vs current (right)
  - Line-by-line diff with additions (green background), deletions (red background), context lines (gray)
  - Collapsible unchanged sections (show "... N unchanged lines ...")
  - Unified diff toggle (single-column mode)
  - Stats header: "+N lines, -M lines"
  - Pure CSS styling, data from `history_diff` Tauri command
- [ ] Create `<history-view>` view:
  - Layout: timeline on left, diff/preview on right
  - Fetch history via `history_list(id)`
  - On version select: fetch via `history_get(id, timestamp)`, compute diff via `history_diff(id, timestamp)`
  - Toolbar: "Restore this version" button with confirmation dialog
  - "View raw" toggle to see full Markdown source of historical version
  - Compare mode: select two historical versions to diff against each other

---

## Feature 13: Vault Switcher

### Files
- `parc-gui/ui/src/components/vault-switcher.ts` — `<vault-switcher>` dropdown
- `parc-gui/ui/src/components/vault-info-panel.ts` — `<vault-info-panel>` details

### Design

The vault switcher lives at the bottom of the sidebar. It shows the current vault name and allows switching between known vaults. Switching vaults triggers a full state refresh (fragment list, tags, schema).

### Tasks
- [ ] Create vault state in `lib/state.ts`:
  - `vaultState: { currentVault, availableVaults, loading }`
  - `fetchVaultInfo()`, `switchVault(path)`, `refreshAll()`
  - On `switchVault`: call backend, then emit `vault:switched` to refresh all dependent components
- [ ] Create `<vault-switcher>`:
  - Compact display: vault name (directory name) + scope icon (global/local)
  - Click opens dropdown of available vaults
  - "Open vault..." option → Tauri directory picker
  - "Init new vault..." option → directory picker + `vault_init`
  - Active vault has checkmark
- [ ] Create `<vault-info-panel>`:
  - Shown in settings or as expanded vault switcher
  - Vault path, scope (global/local)
  - Fragment count by type (mini bar chart using CSS `width` percentages)
  - Total fragments, total tags
  - Index status: last reindex time, DB size
  - "Reindex" button, "Run Doctor" button
  - Doctor findings displayed inline if any

---

## Feature 14: Settings & Preferences

### Files
- `parc-gui/ui/src/views/settings-view.ts` — `<settings-view>` page

### Design

Settings page with sections for vault configuration (read from `config.yml`) and GUI-specific preferences (stored in `localStorage`). Vault config changes are written back to `config.yml` via a backend command.

### Tasks
- [ ] Create preferences state in `lib/state.ts` (persisted to `localStorage`):
  - `prefState.theme: 'light' | 'dark' | 'system'`
  - `prefState.listViewMode: 'table' | 'cards'`
  - `prefState.editorPreviewVisible: boolean`
  - `prefState.sidebarCollapsed: boolean`
  - `prefState.fontSize: number` (12-20)
  - `prefState.graphDefaultDepth: number`
  - Search history and recent fragments stored separately in `localStorage`
- [ ] Create `<settings-view>` with sections:
  - **Appearance**: Theme toggle (light/dark/system), font size slider, sidebar default state
  - **Editor**: Preview pane default (on/off), textarea vs contenteditable toggle
  - **Vault config** (from `config.yml`): user name, default tags, date format, ID display length, color mode
  - **About**: version, vault path, parc-core version

---

## Feature 15: Theming & Dark Mode

### Files
- `parc-gui/ui/src/styles/themes.css` — Theme CSS custom properties
- `parc-gui/ui/src/lib/theme.ts` — Theme detection and switching

### Design

Plain CSS custom properties for theming. Three modes: light, dark, system (follows OS preference via `prefers-color-scheme`). A `data-theme="dark"` attribute on `<html>` toggles dark mode. All components reference CSS custom properties for colors, so theme switching is instant with no re-rendering.

### Tasks
- [ ] Create `themes.css`:
  - `:root` (light) and `[data-theme="dark"]` selectors
  - Variables: `--bg-primary`, `--bg-secondary`, `--bg-surface`, `--text-primary`, `--text-secondary`, `--text-muted`, `--border`, `--accent`, `--accent-hover`
  - Fragment type colors: `--type-note` (blue), `--type-todo` (amber), `--type-decision` (purple), `--type-risk` (red), `--type-idea` (green) — consistent across themes, adjusted for contrast
  - Status colors: `--status-open`, `--status-active`, `--status-done`, `--status-cancelled`
  - Syntax highlighting colors for code blocks in rendered Markdown
  - Diff viewer colors: `--diff-added`, `--diff-removed`, `--diff-context`
- [ ] Create `lib/theme.ts`:
  - `initTheme()` — reads preference from `localStorage`, applies `data-theme` attribute
  - `setTheme(mode: 'light' | 'dark' | 'system')` — updates `<html>` attribute + saves preference
  - Listens to `prefers-color-scheme` media query for system mode (via `matchMedia`)
  - Emits `theme:changed` event via bus
- [ ] Ensure all component styles use custom properties:
  - `var(--bg-primary)` instead of hardcoded colors
  - `var(--text-primary)`, `var(--text-secondary)` semantic references
  - Border, shadow, and accent colors all via custom properties

---

## Feature 16: Keyboard-Driven Workflow

### Files
- `parc-gui/ui/src/lib/keyboard.ts` — Extended from Feature 3
- `parc-gui/ui/src/components/shortcut-help.ts` — `<shortcut-help>` full reference modal

### Design

The app should be fully usable without a mouse. All interactive elements are keyboard-accessible, and common operations have dedicated shortcuts. A help overlay (`Ctrl+?`) shows all available shortcuts.

### Tasks
- [ ] Extend `lib/keyboard.ts` with full shortcut map:
  - **Global**: `Ctrl+K` command palette, `Ctrl+N` new fragment, `Ctrl+F` search, `Ctrl+,` settings, `Ctrl+?` shortcut help
  - **List view**: `j`/`k` move selection, `Enter` open, `e` edit, `d` delete (with confirm), `a` archive, `/` focus search
  - **Detail view**: `e` edit, `Backspace` back to list, `[`/`]` prev/next fragment
  - **Editor**: `Ctrl+S` save, `Escape` cancel/back, `Ctrl+Shift+P` toggle preview
  - Shortcuts disabled when focus is in text input/textarea/contenteditable (except Ctrl combos)
  - Route-aware: only activate shortcuts relevant to current view
- [ ] Create `<shortcut-help>` component:
  - Modal overlay triggered by `Ctrl+?`
  - Grouped by context: Global, List, Detail, Editor
  - Two-column layout: shortcut key + description
  - Shows platform-appropriate modifier (`Cmd` on macOS, `Ctrl` on Linux/Windows)
  - Close on Escape or click outside

---

## Feature 17: Integration Tests

### Files
- `parc-gui/src/tests/` — Rust integration tests for Tauri commands

### Design

**Rust command tests only.** Create a temp vault, call command functions directly (without Tauri runtime), verify DTO output. No Playwright E2E tests — frontend testing is via manual smoke tests (see Verification section).

### Tasks
- [ ] Set up Rust test infrastructure:
  - Helper: `create_test_vault() -> TempDir` — inits vault with sample fragments
  - Helper: `test_state(vault_path) -> AppState` — creates state for command testing
- [ ] Rust command tests:
  - Test: `fragment_list` returns correct DTOs
  - Test: `fragment_create` → `fragment_get` roundtrip
  - Test: `fragment_update` modifies fields correctly
  - Test: `fragment_delete` removes from listing
  - Test: `search` with DSL query returns matching fragments
  - Test: `vault_info` returns correct counts
  - Test: `vault_reindex` rebuilds successfully
  - Test: `tags_list` returns merged tags with correct counts
  - Test: `backlinks` returns linking fragments
  - Test: `history_list` shows versions after edits
  - Test: `vault_switch` changes active vault
  - Test: `schema_list` returns all types including custom
  - Test: `render_markdown` produces correct HTML with resolved wiki-links
  - Test: error handling — invalid ID returns proper GuiError

---

## Implementation Order

1. **Feature 1** (scaffold) — get the Tauri app building and launching
2. **Feature 2** (command layer) — wire parc-core to the frontend via Tauri IPC
3. **Feature 15** (theming) — establish CSS custom properties before building views
4. **Feature 3** (app shell) — layout, navigation, router, command palette
5. **Feature 4** (fragment list) — primary view, most used
6. **Feature 5** (fragment detail) — fragment viewing with server-rendered Markdown
7. **Feature 6** (editor) — in-app editing with contenteditable body
8. **Feature 7** (creation) — new fragment flow
9. **Feature 8** (search) — DSL search with autocomplete
10. **Feature 9** (tags) — tag browser
11. **Feature 13** (vault switcher) — multi-vault support
12. **Feature 11** (attachments) — drag-and-drop, previews
13. **Feature 12** (history) — version timeline and diffs
14. **Feature 10** (graph) — Canvas 2D backlink visualization
15. **Feature 14** (settings) — preferences and config
16. **Feature 16** (keyboard) — full keyboard accessibility
17. **Feature 17** (tests) — Rust integration tests

---

## Verification

```bash
# Build
cargo build -p parc-gui
cd parc-gui/ui && npm install && npm run build && cd ../..

# Development
cargo tauri dev                               # launches app with hot-reload

# Rust tests
cargo test -p parc-gui

# Full workspace
cargo test --workspace
cargo clippy --workspace

# Smoke test: manual
# 1. Launch app → verify fragment list loads
# 2. Create a new todo → verify it appears in list
# 3. Click the todo → verify detail view renders body and metadata
# 4. Edit the todo → verify changes persist
# 5. Search "type:todo status:open" → verify results
# 6. Click Tags in sidebar → verify tag cloud/list loads
# 7. Click Graph in sidebar → verify Canvas 2D graph renders with nodes and edges
# 8. Toggle dark mode → verify all views render correctly (CSS custom properties)
# 9. Press Ctrl+K → verify command palette opens
# 10. Drag an image onto a fragment → verify attachment uploads
# 11. Click History → verify version timeline and diff viewer
# 12. Switch vault → verify list refreshes with new vault's fragments

# Production build
cargo tauri build                             # creates distributable binary
```

---

## Definition of Done (M9)

- [ ] Tauri app builds and launches on Linux (primary), macOS, and Windows
- [ ] All parc-core operations accessible through Tauri commands
- [ ] Fragment list with filtering, sorting, and two view modes (table/cards)
- [ ] Fragment detail with server-rendered Markdown (comrak), backlinks, and attachments
- [ ] In-app editor with schema-driven form, contenteditable body, and live preview
- [ ] Fragment creation with type selection and template loading
- [ ] Full DSL search with autocomplete suggestions
- [ ] Tag browser with cloud and list views
- [ ] Interactive Canvas 2D backlink graph with force-directed layout
- [ ] Attachment drag-and-drop upload and inline preview
- [ ] History viewer with timeline, diff, and restore
- [ ] Multi-vault switching
- [ ] Light/dark theme via CSS custom properties with system preference detection
- [ ] Full keyboard navigation and command palette
- [ ] All Rust command tests pass
- [ ] Zero npm runtime dependencies (only `@tauri-apps/api`, `vite`, `typescript` as dev deps)
- [ ] `cargo clippy` clean
- [ ] `cargo test --workspace` passes
