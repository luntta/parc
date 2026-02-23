# M9 — Tauri Desktop GUI

## Features

1. **`parc-gui` crate scaffold** — New workspace member with Tauri, React (TypeScript), and Vite
2. **Tauri command layer** — Rust backend exposing `parc-core` operations as Tauri IPC commands
3. **App shell & navigation** — Sidebar layout with navigation, global search, and keyboard shortcuts
4. **Fragment list view** — Browsable, filterable, sortable fragment table with type/status badges
5. **Fragment detail view** — Full fragment display with rendered Markdown, metadata, backlinks, and attachments
6. **Fragment editor** — In-app fragment editing with YAML frontmatter + Markdown body, live preview
7. **Fragment creation** — Type-aware creation form with schema-driven fields and template loading
8. **Search with DSL** — Full DSL search bar with autocomplete, filter chips, and result highlighting
9. **Tag browser** — Tag cloud/list with counts, click-to-filter, tag management
10. **Backlink graph** — Interactive visual graph of fragment links using a force-directed layout
11. **Attachment management** — Drag-and-drop attachment upload, inline preview (images, PDFs), detach
12. **History viewer** — Version timeline, side-by-side diff, one-click restore
13. **Vault switcher** — Multi-vault support: switch between global/local vaults, vault info panel
14. **Settings & preferences** — In-app settings UI for vault config, theme, display preferences
15. **Theming & dark mode** — Light/dark theme with system preference detection
16. **Keyboard-driven workflow** — Command palette (Ctrl+K), vim-style navigation hints, full keyboard accessibility
17. **Integration tests** — Tauri integration tests for command layer + Playwright E2E tests for frontend

**PRD refs:** §3.1 (architecture — Tauri imports parc-core directly), §3.2 (integration layers), §13 (GUI Readiness), M9 milestone definition.

---

## Feature 1: `parc-gui` Crate Scaffold

### Files
- `Cargo.toml` (workspace root) — Add `parc-gui` to `members`
- `parc-gui/Cargo.toml` — Tauri app Rust backend manifest
- `parc-gui/tauri.conf.json` — Tauri configuration
- `parc-gui/src/main.rs` — Tauri entry point
- `parc-gui/src/lib.rs` — Command registration and app setup
- `parc-gui/ui/` — Frontend project root (Vite + React + TypeScript)
- `parc-gui/ui/package.json` — Frontend dependencies
- `parc-gui/ui/tsconfig.json` — TypeScript configuration
- `parc-gui/ui/vite.config.ts` — Vite configuration with Tauri plugin
- `parc-gui/ui/index.html` — HTML entry point
- `parc-gui/ui/src/main.tsx` — React entry point
- `parc-gui/ui/src/App.tsx` — Root component

### Design

Tauri v2 app with a React + TypeScript frontend bundled via Vite. The Rust backend is a thin wrapper around `parc-core` — all business logic stays in the library. The frontend communicates with the backend exclusively through Tauri's IPC invoke mechanism (type-safe via `@tauri-apps/api`).

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
│   │   └── history.rs
│   ├── state.rs            # Managed Tauri state (vault path, config cache)
│   └── error.rs            # GUI-specific error wrapper implementing Serialize
├── ui/
│   ├── package.json
│   ├── tsconfig.json
│   ├── vite.config.ts
│   ├── index.html
│   └── src/
│       ├── main.tsx
│       ├── App.tsx
│       ├── api/            # Typed Tauri invoke wrappers
│       ├── components/     # Reusable UI components
│       ├── views/          # Page-level views
│       ├── hooks/          # React custom hooks
│       ├── store/          # State management (Zustand)
│       ├── types/          # TypeScript type definitions
│       └── styles/         # CSS / Tailwind config
```

### Tasks
- [ ] Add `"parc-gui"` to workspace members in root `Cargo.toml`
- [ ] Create `parc-gui/Cargo.toml`:
  - `parc-core = { path = "../parc-core" }`
  - `tauri = { version = "2", features = ["protocol-asset"] }`
  - `tauri-build` (build dependency)
  - `serde`, `serde_json` (with derive)
  - `anyhow`, `thiserror`
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
  - `npm create vite@latest . -- --template react-ts`
  - Install: `@tauri-apps/api@2`, `@tauri-apps/plugin-dialog`, `@tauri-apps/plugin-fs`
  - Install: `tailwindcss`, `@tailwindcss/typography`, `postcss`, `autoprefixer`
  - Install: `zustand` (state management), `react-router-dom` (routing)
  - Install: `@uiw/react-codemirror` (editor), `react-markdown`, `remark-gfm` (Markdown rendering)
  - Install: `@xyflow/react` (graph visualization)
  - Install: `cmdk` (command palette)
  - Configure Tailwind with custom parc color palette
- [ ] Create minimal `App.tsx` with "parc" heading and Tauri invoke test
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
- [ ] Unit tests for each command: mock vault with temp dir, call command, verify DTO output

---

## Feature 3: App Shell & Navigation

### Files
- `parc-gui/ui/src/App.tsx` — Root layout with sidebar and content area
- `parc-gui/ui/src/components/Sidebar.tsx` — Navigation sidebar
- `parc-gui/ui/src/components/TopBar.tsx` — Top bar with search and actions
- `parc-gui/ui/src/components/CommandPalette.tsx` — Ctrl+K command palette
- `parc-gui/ui/src/hooks/useKeyboard.ts` — Global keyboard shortcut handler
- `parc-gui/ui/src/store/navigation.ts` — Navigation state (Zustand)
- `parc-gui/ui/src/styles/global.css` — Base styles, Tailwind imports

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

### Tasks
- [ ] Create `App.tsx` with React Router:
  - Routes: `/` (list), `/fragment/:id` (detail), `/search` (search results), `/tags` (tag browser), `/graph` (backlink graph), `/settings` (settings), `/trash` (trash view)
- [ ] Create `Sidebar.tsx`:
  - Navigation sections: fragment types (All, Note, Todo, Decision, Risk, Idea), tools (Tags, Graph), management (Trash)
  - Active route highlighting
  - Fragment count badges per type (fetched from `vault_info`)
  - Vault name/path at bottom with switch button
  - Collapsible to icon-only mode
- [ ] Create `TopBar.tsx`:
  - Global search input (triggers DSL search on Enter, shows suggestions on type)
  - "+ New" button with dropdown for fragment type selection
  - Settings gear icon
  - Vault status indicator
- [ ] Create `CommandPalette.tsx` using `cmdk`:
  - Triggered by `Ctrl+K` / `Cmd+K`
  - Actions: New fragment (by type), search, switch vault, reindex, navigate to any view
  - Recent fragments as suggestions
  - Fuzzy matching on fragment titles
- [ ] Create `useKeyboard.ts` hook:
  - Global shortcuts: `Ctrl+K` (command palette), `Ctrl+N` (new fragment), `Ctrl+F` (focus search), `Escape` (close panels/modals)
  - Navigation: `j/k` for list movement when not in an input, `Enter` to open selected
- [ ] Create `navigation.ts` Zustand store:
  - `selectedFragmentId: string | null`
  - `sidebarCollapsed: boolean`
  - `detailPanelOpen: boolean`
  - `currentTypeFilter: string | null`
- [ ] Create `global.css` with Tailwind base:
  - Custom properties for theme colors
  - Typography defaults for rendered Markdown
  - Scrollbar styling
  - Transition defaults

---

## Feature 4: Fragment List View

### Files
- `parc-gui/ui/src/views/FragmentList.tsx` — Main list view
- `parc-gui/ui/src/components/FragmentCard.tsx` — Card component for grid view
- `parc-gui/ui/src/components/FragmentRow.tsx` — Row component for table view
- `parc-gui/ui/src/components/TypeBadge.tsx` — Colored type badge
- `parc-gui/ui/src/components/StatusBadge.tsx` — Status pill with color
- `parc-gui/ui/src/components/TagChip.tsx` — Clickable tag chip
- `parc-gui/ui/src/components/FilterBar.tsx` — Active filter chips with clear buttons
- `parc-gui/ui/src/store/fragments.ts` — Fragment list state (Zustand)
- `parc-gui/ui/src/hooks/useFragments.ts` — Fragment data fetching hook

### Design

The list view is the primary view. It supports two display modes (table and cards) and pulls data from the Tauri backend via the `fragment_list` command. Filters are applied via query parameters so they can be deep-linked from the sidebar.

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
- [ ] Create `FragmentList.tsx`:
  - Fetch fragments via `listFragments()` with current filters
  - Toggle between table view and card grid view (persisted in local storage)
  - Column sort headers: title, type, status, updated, created, priority, due
  - Empty state: "No fragments found" with create button
  - Loading skeleton while fetching
  - Click row/card to select fragment (opens detail panel or navigates)
  - Right-click context menu: Edit, Archive, Delete, Copy ID
- [ ] Create `FragmentCard.tsx`:
  - Type badge (colored), title, truncated body preview (first 2 lines), tags, date
  - Status indicator
  - Due date with overdue highlighting (red if past due)
- [ ] Create `FragmentRow.tsx`:
  - Table row: short ID, type badge, status badge, title, tags (truncated), updated date
  - Priority indicator for todos (colored dot)
  - Due date with overdue highlighting
- [ ] Create `TypeBadge.tsx` — small colored pill with type name
- [ ] Create `StatusBadge.tsx` — status pill with semantic color
- [ ] Create `TagChip.tsx` — clickable chip that adds tag filter, removable when active
- [ ] Create `FilterBar.tsx`:
  - Shows active filters as removable chips
  - Quick-add dropdown for common filters (type, status, tag, priority)
  - "Clear all" button when filters active
- [ ] Create `fragments.ts` Zustand store:
  - `fragments: FragmentSummary[]`
  - `loading: boolean`
  - `filters: { type?, status?, tags: string[], priority?, sort, limit }`
  - `viewMode: 'table' | 'cards'`
  - Actions: `fetchFragments()`, `setFilter()`, `clearFilters()`, `toggleViewMode()`
- [ ] Create `useFragments.ts` hook:
  - Wraps store with auto-refetch on filter change
  - Debounced refetch on rapid filter changes
  - Optimistic updates on delete/archive

---

## Feature 5: Fragment Detail View

### Files
- `parc-gui/ui/src/views/FragmentDetail.tsx` — Full fragment detail view
- `parc-gui/ui/src/components/MetadataPanel.tsx` — Structured metadata display
- `parc-gui/ui/src/components/MarkdownRenderer.tsx` — Rendered Markdown with wiki-link support
- `parc-gui/ui/src/components/BacklinksSection.tsx` — Backlinks list
- `parc-gui/ui/src/components/AttachmentsSection.tsx` — Attachment list with previews
- `parc-gui/ui/src/components/FragmentActions.tsx` — Action toolbar (edit, delete, archive, etc.)

### Design

The detail view appears in the right panel when a fragment is selected, or as a full-page view when navigated to directly. It shows everything about a fragment: metadata, rendered body, backlinks, and attachments.

Wiki-links (`[[id]]` and `[[id|text]]`) in the rendered Markdown are converted to clickable links that navigate to the linked fragment. Attachment references (`![[attach:filename]]`) render inline previews where possible (images) or download links.

### Tasks
- [ ] Create `FragmentDetail.tsx`:
  - Fetch full fragment via `getFragment(id)` on mount/id change
  - Layout: metadata panel at top, rendered body, backlinks section, attachments section
  - Loading skeleton
  - 404 state for invalid IDs
  - Sticky action toolbar at top
- [ ] Create `MetadataPanel.tsx`:
  - Display: type badge, status badge, full ID (copyable), title
  - Fields grid: created_at, updated_at, created_by, due, priority, assignee (conditional on type)
  - Tags as clickable chips
  - Links as clickable fragment references (show title on hover via tooltip)
  - Extra fields displayed dynamically based on schema
- [ ] Create `MarkdownRenderer.tsx`:
  - Uses `react-markdown` + `remark-gfm` for rendering
  - Custom renderers:
    - Wiki-links (`[[id]]`, `[[id|text]]`) → clickable internal links with fragment title tooltip
    - Attachment refs (`![[attach:file]]`) → inline image preview or file link
    - Code blocks with syntax highlighting
    - Inline `#hashtags` → clickable tag links
  - Typography styling via `@tailwindcss/typography`
- [ ] Create `BacklinksSection.tsx`:
  - Fetch via `getBacklinks(id)`
  - List of linking fragments: type badge, title, snippet of linking context
  - Click to navigate to linking fragment
  - "No backlinks" empty state
- [ ] Create `AttachmentsSection.tsx`:
  - Fetch via `listAttachments(id)`
  - Grid of attachment cards: filename, size, type icon
  - Image attachments: thumbnail preview
  - Click to open in system viewer (via Tauri shell open)
  - Detach button with confirmation
- [ ] Create `FragmentActions.tsx`:
  - Buttons: Edit (opens editor view), Delete (confirmation dialog), Archive
  - Copy ID button, Copy markdown link button
  - History button (opens history viewer)
  - More menu: Export as JSON, Export as Markdown

---

## Feature 6: Fragment Editor

### Files
- `parc-gui/ui/src/views/FragmentEditor.tsx` — Full editor view
- `parc-gui/ui/src/components/FrontmatterForm.tsx` — Schema-driven metadata form
- `parc-gui/ui/src/components/MarkdownEditor.tsx` — CodeMirror-based body editor
- `parc-gui/ui/src/components/EditorPreview.tsx` — Live Markdown preview pane
- `parc-gui/ui/src/hooks/useAutoSave.ts` — Auto-save debounce logic

### Design

The editor is a split-pane view: structured form fields on top (driven by the fragment's schema), Markdown body editor below with optional side-by-side live preview. The frontmatter form renders appropriate inputs based on field types (text, enum dropdown, date picker, tag multiselect).

Changes are tracked locally and saved explicitly via Ctrl+S or a Save button. An auto-save draft is stored in local storage to prevent data loss on accidental close.

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
│  │ ## Context            │ │ │ Context              │ │
│  │                       │ │ │                      │ │
│  │ We need fast full-   │ │ │ We need fast full-   │ │
│  │ text search across   │ │ │ text search across   │ │
│  │ all fragments...     │ │ │ all fragments...     │ │
│  └──────────────────────┘ │ └──────────────────────┘ │
└──────────────────────────────────────────────────────┘
```

### Tasks
- [ ] Create `FragmentEditor.tsx`:
  - Load fragment data on mount (or empty template for new fragments)
  - Top bar: back button, fragment title, Save (Ctrl+S) and Cancel buttons
  - Unsaved changes indicator (dot or asterisk in title)
  - Warn on navigation away with unsaved changes (beforeunload equivalent)
  - On save: call `fragment_update`, show success toast, navigate back to detail
  - Validation errors displayed inline (red borders on invalid fields)
- [ ] Create `FrontmatterForm.tsx`:
  - Fetch schema via `schema_get(type)` to determine available fields
  - Render fields dynamically based on `FieldType`:
    - `String` → text input
    - `Enum` → select dropdown with allowed values
    - `Date` → date picker input (native `input[type=date]`)
    - `ListOfStrings` → multi-input (for deciders, etc.)
  - Tags field: multi-select with typeahead from existing tags (`tags_list`)
  - Links field: multi-input with ID prefix search autocomplete
  - Title field: always visible, required, auto-focus on new fragments
- [ ] Create `MarkdownEditor.tsx`:
  - CodeMirror 6 editor with Markdown syntax highlighting
  - Extensions: line numbers, bracket matching, search (Ctrl+F), undo/redo
  - Custom completions: `#` triggers tag suggestions, `[[` triggers fragment ID/title suggestions
  - Toolbar: bold, italic, heading, link, code block, bullet list, numbered list
  - Drag-and-drop file onto editor triggers attachment flow
- [ ] Create `EditorPreview.tsx`:
  - Reuses `MarkdownRenderer` component from Feature 5
  - Synced scroll position with editor (best-effort)
  - Toggle show/hide preview (default: show)
  - Debounced rendering (100ms after last keystroke)
- [ ] Create `useAutoSave.ts`:
  - Stores draft to `localStorage` keyed by fragment ID (or `"new-<type>"` for creation)
  - Saves every 5 seconds if dirty
  - Clears draft on explicit save or discard
  - Prompts to restore draft if found on editor open

---

## Feature 7: Fragment Creation

### Files
- `parc-gui/ui/src/views/FragmentCreate.tsx` — Creation flow
- `parc-gui/ui/src/components/TypeSelector.tsx` — Fragment type selection

### Design

Fragment creation reuses the editor components (Feature 6) but starts from a template. The flow is:

1. User clicks "+ New" → type selector appears (modal or dropdown)
2. User picks a type → editor opens with template loaded and schema fields pre-configured
3. User fills in fields and body → clicks Save
4. Backend creates fragment → navigates to new fragment's detail view

### Tasks
- [ ] Create `TypeSelector.tsx`:
  - Grid of type cards: icon, name, description, field summary
  - Keyboard navigable (arrow keys + Enter)
  - Shows all types from `schema_list` (built-in + custom)
- [ ] Create `FragmentCreate.tsx`:
  - Type selection step (if type not pre-selected from sidebar)
  - Delegates to `FragmentEditor` in create mode
  - Loads template body from schema/template
  - Pre-fills: type, default status, default priority, default_tags from config
  - CLI-flag-equivalent fields pre-set if navigated from sidebar (e.g., clicking "+ Todo" pre-selects todo type)
  - On save: calls `fragment_create`, shows success toast with ID, navigates to detail view

---

## Feature 8: Search with DSL

### Files
- `parc-gui/ui/src/views/SearchResults.tsx` — Search results page
- `parc-gui/ui/src/components/SearchBar.tsx` — Global search input with autocomplete
- `parc-gui/ui/src/components/SearchSuggestions.tsx` — Autocomplete dropdown
- `parc-gui/ui/src/components/FilterChips.tsx` — Visual DSL filter chips
- `parc-gui/ui/src/hooks/useSearch.ts` — Search state and debouncing
- `parc-gui/ui/src/utils/dsl.ts` — Client-side DSL token parsing for UI assistance

### Design

The search bar is always visible in the top bar. It accepts the full parc search DSL. As the user types, the UI provides:

1. **Syntax assistance** — recognized filter prefixes (`type:`, `status:`, etc.) are highlighted with distinct colors
2. **Value suggestions** — after typing `type:`, show available types; after `tag:`, show existing tags; after `status:`, show valid statuses for the current type filter
3. **Recent searches** — shown when the search bar is focused and empty
4. **Live results** — results update as the user types (debounced 300ms)

### Tasks
- [ ] Create `SearchBar.tsx`:
  - Input with DSL syntax highlighting (colored spans for filter tokens)
  - Focus with `Ctrl+F` or clicking
  - Submit on Enter (navigates to search results view)
  - Clear button (×) when non-empty
  - Dropdown overlay for suggestions while typing
- [ ] Create `SearchSuggestions.tsx`:
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
- [ ] Create `FilterChips.tsx`:
  - Parse current query into visual chips (e.g., `type:todo` → blue chip "Type: todo")
  - Each chip has × button to remove that filter term
  - Click chip to edit that filter value
- [ ] Create `SearchResults.tsx`:
  - Calls `search(query)` and displays results
  - Reuses `FragmentRow` / `FragmentCard` from list view
  - Highlights matching text in title and body snippet
  - Shows result count and query execution time
  - Empty state: "No results for <query>" with suggestions
- [ ] Create `useSearch.ts`:
  - Debounced search (300ms after last keystroke)
  - Search history stored in `localStorage` (last 20 queries)
  - Abort previous in-flight search when new query arrives
- [ ] Create `dsl.ts` utility:
  - `tokenize(query: string) -> Token[]` — splits query into filter tokens and text terms
  - `Token = { type: 'filter' | 'text' | 'hashtag' | 'phrase', value: string, filterName?: string }`
  - Used for syntax highlighting and suggestion context (client-side only, actual parsing is in `parc-core`)

---

## Feature 9: Tag Browser

### Files
- `parc-gui/ui/src/views/TagBrowser.tsx` — Tag browser view
- `parc-gui/ui/src/components/TagCloud.tsx` — Visual tag cloud
- `parc-gui/ui/src/components/TagList.tsx` — Sortable tag table

### Design

The tag browser shows all tags (merged frontmatter + inline) with their usage counts. Two view modes: a visual cloud (font size proportional to count) and a sortable table. Clicking a tag navigates to a filtered fragment list.

### Tasks
- [ ] Create `TagBrowser.tsx`:
  - Fetch tags via `listTags()`
  - Toggle between cloud and list views
  - Search/filter within tags
  - Sort options: alphabetical, by count (asc/desc)
- [ ] Create `TagCloud.tsx`:
  - Tags sized proportionally to usage count (min 0.8rem, max 2.5rem)
  - Colored by most common fragment type using that tag
  - Hover shows exact count
  - Click navigates to `/` with `tag:` filter applied
- [ ] Create `TagList.tsx`:
  - Table: tag name, count, fragment type breakdown
  - Sortable columns
  - Click tag → navigate to filtered fragment list
  - Inline sparkline or mini bar showing type distribution

---

## Feature 10: Backlink Graph

### Files
- `parc-gui/ui/src/views/GraphView.tsx` — Full-page graph view
- `parc-gui/ui/src/components/FragmentGraph.tsx` — Graph visualization component
- `parc-gui/ui/src/hooks/useGraphData.ts` — Graph data fetching and transformation

### Design

Interactive force-directed graph showing fragments as nodes and links as edges. Uses `@xyflow/react` (React Flow) for rendering. Nodes are colored by fragment type and sized by link count. The graph can be filtered by type, tag, or a root fragment (show N-hop neighborhood).

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
- [ ] Create `useGraphData.ts`:
  - Fetch all fragments with links (via `fragment_list` with `has:links` or full list)
  - Transform into nodes + edges for React Flow:
    - Node: `{ id, label: title, type: fragment_type, data: { fragment } }`
    - Edge: `{ source, target }` for each link
  - Support filtering: by type, by tag, by neighborhood (N hops from a root fragment)
  - Memoize transformation
- [ ] Create `FragmentGraph.tsx`:
  - React Flow canvas with force-directed layout (using `dagre` or `elkjs` for layout)
  - Node rendering: colored circle/rectangle by type, label with title (truncated)
  - Edge rendering: directional arrows
  - Interactions: click node to select fragment (shows mini detail panel), double-click to navigate to detail view
  - Zoom/pan controls
  - Minimap for large graphs
- [ ] Create `GraphView.tsx`:
  - Full-page graph with control panel overlay
  - Controls: type filter checkboxes, tag filter, depth slider (1-5 hops), layout algorithm toggle
  - "Focus on fragment" input — centers graph on a specific fragment's neighborhood
  - Fragment count display
  - Export as PNG button (canvas capture)

---

## Feature 11: Attachment Management

### Files
- `parc-gui/ui/src/components/AttachmentUploader.tsx` — Drag-and-drop upload zone
- `parc-gui/ui/src/components/AttachmentPreview.tsx` — Inline file preview (images, PDFs)
- `parc-gui/ui/src/components/AttachmentGrid.tsx` — Grid of attachment thumbnails

### Design

Attachments can be added via drag-and-drop onto the editor or detail view, or via a file picker dialog. Images show inline previews. Other files show type-appropriate icons. Tauri's asset protocol serves attachment files directly to the webview.

### Tasks
- [ ] Create `AttachmentUploader.tsx`:
  - Drop zone with visual feedback (border highlight on drag-over)
  - Accept any file type
  - On drop: call `attach_file` with the file path (Tauri gives us the path)
  - Progress indicator for large files (or spinner)
  - Multiple file drop support
  - Also accessible via "Browse files" button using Tauri's dialog plugin
- [ ] Create `AttachmentPreview.tsx`:
  - Image files (png, jpg, gif, svg, webp): render `<img>` via Tauri asset protocol
  - PDF: embedded viewer or link to open externally
  - Text/code files: syntax-highlighted preview (first 50 lines)
  - Other: file icon + metadata (name, size, type)
  - Click to open in system default app
- [ ] Create `AttachmentGrid.tsx`:
  - Grid layout of attachment cards with thumbnails
  - Hover reveals: filename, size, detach button
  - Lightbox mode for images (click to expand full-size)
  - Drag-out support (drag attachment to desktop/other app, if Tauri supports)

---

## Feature 12: History Viewer

### Files
- `parc-gui/ui/src/views/HistoryView.tsx` — Fragment history page
- `parc-gui/ui/src/components/VersionTimeline.tsx` — Vertical timeline of versions
- `parc-gui/ui/src/components/DiffViewer.tsx` — Side-by-side diff display

### Design

The history viewer shows all saved versions of a fragment as a vertical timeline. Selecting a version shows a side-by-side diff against the current version (or any other version). One-click restore creates a new version from the historical snapshot.

### Tasks
- [ ] Create `VersionTimeline.tsx`:
  - Vertical timeline with version entries: timestamp, relative time ("2 hours ago"), size
  - Current version at top (marked as "Current")
  - Click to select a version for viewing/diffing
  - Active version highlighted
- [ ] Create `DiffViewer.tsx`:
  - Side-by-side view: old version (left) vs current (right)
  - Line-by-line diff with additions (green), deletions (red), context lines (gray)
  - Collapsible unchanged sections
  - Unified diff toggle (single-column mode)
  - Stats header: "+N lines, -M lines"
- [ ] Create `HistoryView.tsx`:
  - Layout: timeline on left, diff/preview on right
  - Fetch history via `history_list(id)`
  - On version select: fetch via `history_get(id, timestamp)`, compute diff via `history_diff(id, timestamp)`
  - Toolbar: "Restore this version" button with confirmation dialog
  - "View raw" toggle to see full Markdown source of historical version
  - Compare mode: select two historical versions to diff against each other

---

## Feature 13: Vault Switcher

### Files
- `parc-gui/ui/src/components/VaultSwitcher.tsx` — Vault selection dropdown/modal
- `parc-gui/ui/src/components/VaultInfoPanel.tsx` — Vault details panel
- `parc-gui/ui/src/store/vault.ts` — Vault state (Zustand)

### Design

The vault switcher lives at the bottom of the sidebar. It shows the current vault name and allows switching between known vaults. Switching vaults triggers a full state refresh (fragment list, tags, schema).

### Tasks
- [ ] Create `vault.ts` Zustand store:
  - `currentVault: VaultInfo`
  - `availableVaults: VaultInfo[]`
  - `loading: boolean`
  - Actions: `fetchVaultInfo()`, `switchVault(path)`, `refreshAll()`
  - On `switchVault`: call backend, then refresh all dependent stores (fragments, tags, schemas)
- [ ] Create `VaultSwitcher.tsx`:
  - Compact display: vault name (directory name) + scope icon (global/local)
  - Click opens dropdown of available vaults
  - "Open vault..." option → Tauri directory picker
  - "Init new vault..." option → directory picker + `vault_init`
  - Active vault has checkmark
- [ ] Create `VaultInfoPanel.tsx`:
  - Shown in settings or as expanded vault switcher
  - Vault path, scope (global/local)
  - Fragment count by type (mini bar chart)
  - Total fragments, total tags
  - Index status: last reindex time, DB size
  - "Reindex" button, "Run Doctor" button
  - Doctor findings displayed inline if any

---

## Feature 14: Settings & Preferences

### Files
- `parc-gui/ui/src/views/SettingsView.tsx` — Settings page
- `parc-gui/ui/src/store/preferences.ts` — GUI preferences (Zustand + localStorage)

### Design

Settings page with sections for vault configuration (read from `config.yml`) and GUI-specific preferences (stored in localStorage). Vault config changes are written back to `config.yml` via a backend command.

### Tasks
- [ ] Create `preferences.ts` Zustand store (persisted to `localStorage`):
  - `theme: 'light' | 'dark' | 'system'`
  - `listViewMode: 'table' | 'cards'`
  - `editorPreviewVisible: boolean`
  - `sidebarCollapsed: boolean`
  - `searchHistory: string[]`
  - `recentFragments: string[]` (last 20 viewed fragment IDs)
  - `fontSize: number` (12-20)
  - `graphDefaultDepth: number`
- [ ] Create `SettingsView.tsx` with sections:
  - **Appearance**: Theme toggle (light/dark/system), font size slider, sidebar default state
  - **Editor**: Preview pane default (on/off), auto-save interval, tab size
  - **Vault config** (from `config.yml`): user name, default tags, date format, ID display length, color mode
  - **About**: version, vault path, parc-core version, link to docs

---

## Feature 15: Theming & Dark Mode

### Files
- `parc-gui/ui/src/styles/themes.css` — Theme CSS custom properties
- `parc-gui/ui/src/hooks/useTheme.ts` — Theme detection and switching
- `parc-gui/ui/tailwind.config.ts` — Tailwind dark mode configuration

### Design

Tailwind's `class` strategy for dark mode, driven by a `dark` class on `<html>`. Three modes: light, dark, system (follows OS preference via `prefers-color-scheme`). Theme colors use CSS custom properties so they can be swapped without Tailwind rebuild.

### Tasks
- [ ] Create `themes.css`:
  - Light theme variables: backgrounds, text, borders, accents
  - Dark theme variables under `.dark` selector
  - Fragment type colors (consistent across themes, adjusted for contrast)
  - Syntax highlighting colors for Markdown code blocks
  - Diff viewer colors (red/green with appropriate contrast in both themes)
- [ ] Configure `tailwind.config.ts`:
  - `darkMode: 'class'`
  - Extend theme with custom colors referencing CSS variables
  - Typography plugin dark mode variants
- [ ] Create `useTheme.ts`:
  - Reads preference from `preferences` store
  - Listens to `prefers-color-scheme` media query for system mode
  - Applies/removes `dark` class on `<html>` element
  - Updates Tauri window theme (via `appWindow.setTheme()`) for native title bar consistency
- [ ] Ensure all components use theme-aware classes:
  - `bg-surface` / `bg-surface-dark` instead of hardcoded colors
  - `text-primary` / `text-secondary` semantic classes
  - Border, shadow, and accent colors all theme-aware

---

## Feature 16: Keyboard-Driven Workflow

### Files
- `parc-gui/ui/src/hooks/useKeyboard.ts` — Extended from Feature 3
- `parc-gui/ui/src/components/ShortcutHint.tsx` — Keyboard shortcut tooltip
- `parc-gui/ui/src/components/ShortcutHelp.tsx` — Full shortcut reference (Ctrl+?)

### Design

The app should be fully usable without a mouse. All interactive elements are keyboard-accessible, and common operations have dedicated shortcuts. A help overlay (`Ctrl+?`) shows all available shortcuts.

### Tasks
- [ ] Extend `useKeyboard.ts` with full shortcut map:
  - **Global**: `Ctrl+K` command palette, `Ctrl+N` new fragment, `Ctrl+F` search, `Ctrl+,` settings, `Ctrl+?` shortcut help
  - **List view**: `j`/`k` move selection, `Enter` open, `e` edit, `d` delete (with confirm), `a` archive, `/` focus search
  - **Detail view**: `e` edit, `Backspace` back to list, `[`/`]` prev/next fragment
  - **Editor**: `Ctrl+S` save, `Escape` cancel/back, `Ctrl+Shift+P` toggle preview
  - Shortcuts disabled when focus is in text input/textarea (except Ctrl combos)
- [ ] Create `ShortcutHint.tsx`:
  - Small tooltip showing keyboard shortcut next to buttons/actions
  - Shows platform-appropriate modifier (`Cmd` on macOS, `Ctrl` on Linux/Windows)
  - Only visible after a short hover delay or when `Alt` is held
- [ ] Create `ShortcutHelp.tsx`:
  - Modal overlay triggered by `Ctrl+?`
  - Grouped by context: Global, List, Detail, Editor
  - Two-column layout: shortcut key + description
  - Searchable/filterable

---

## Feature 17: Integration Tests

### Files
- `parc-gui/src/tests/` — Rust integration tests for Tauri commands
- `parc-gui/ui/tests/` — Playwright E2E tests for the frontend
- `parc-gui/ui/playwright.config.ts` — Playwright configuration

### Design

Two test layers:

1. **Rust command tests** — Unit/integration tests for the Tauri command layer. Create a temp vault, call command functions directly (without Tauri runtime), verify DTO output.

2. **E2E tests** — Playwright tests that launch the full Tauri app and interact with it through the UI. These are slower but verify the full stack.

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
  - Test: error handling — invalid ID returns proper GuiError
- [ ] Set up Playwright:
  - Install `@playwright/test`
  - Configure `playwright.config.ts` with Tauri-specific setup (launch app binary)
  - Helper: `launchApp()` — starts Tauri app, returns page handle
- [ ] Playwright E2E tests:
  - Test: App launches and shows fragment list
  - Test: Create a new note via UI → appears in list
  - Test: Click fragment → detail view shows correct content
  - Test: Edit fragment → changes persist
  - Test: Search with DSL → results displayed
  - Test: Tag browser shows tags with counts
  - Test: Dark mode toggle works
  - Test: Command palette opens and navigates
  - Test: Keyboard shortcuts work (j/k navigation, Ctrl+N new, etc.)
  - Test: Vault switcher lists available vaults

---

## Implementation Order

1. **Feature 1** (scaffold) — get the Tauri app building and launching
2. **Feature 2** (command layer) — wire parc-core to the frontend via Tauri IPC
3. **Feature 15** (theming) — establish the design system before building views
4. **Feature 3** (app shell) — layout, navigation, command palette
5. **Feature 4** (fragment list) — primary view, most used
6. **Feature 5** (fragment detail) — fragment viewing with Markdown rendering
7. **Feature 6** (editor) — in-app editing with CodeMirror
8. **Feature 7** (creation) — new fragment flow
9. **Feature 8** (search) — DSL search with autocomplete
10. **Feature 9** (tags) — tag browser
11. **Feature 13** (vault switcher) — multi-vault support
12. **Feature 11** (attachments) — drag-and-drop, previews
13. **Feature 12** (history) — version timeline and diffs
14. **Feature 10** (graph) — backlink visualization
15. **Feature 14** (settings) — preferences and config
16. **Feature 16** (keyboard) — full keyboard accessibility
17. **Feature 17** (tests) — integration and E2E test suites

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

# Frontend tests
cd parc-gui/ui && npm test && cd ../..

# E2E tests
cd parc-gui/ui && npx playwright test && cd ../..

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
# 7. Click Graph in sidebar → verify graph renders with nodes and edges
# 8. Toggle dark mode → verify all views render correctly
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
- [ ] Fragment detail with rendered Markdown, backlinks, and attachments
- [ ] In-app editor with schema-driven form, CodeMirror body, and live preview
- [ ] Fragment creation with type selection and template loading
- [ ] Full DSL search with autocomplete suggestions
- [ ] Tag browser with cloud and list views
- [ ] Interactive backlink graph with filtering and navigation
- [ ] Attachment drag-and-drop upload and inline preview
- [ ] History viewer with timeline, diff, and restore
- [ ] Multi-vault switching
- [ ] Light/dark theme with system preference detection
- [ ] Full keyboard navigation and command palette
- [ ] All Rust command tests pass
- [ ] Playwright E2E tests pass
- [ ] `cargo clippy` clean
- [ ] `cargo test --workspace` passes
