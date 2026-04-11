---
layout: layouts/doc.njk
title: Desktop GUI
eyebrow: Reference · §07
---

`parc-gui` is a Tauri-based desktop application that ships with parc. It wraps `parc-core` directly (no IPC layer) and gives you a graphical interface for the same vaults the CLI works with.

## Install

```bash
# Build the frontend bundle first
cd parc-gui/ui && npm install && npm run build && cd ../..

# Install the binary
cargo install --path parc-gui

# Run it
parc-gui
```

For development with hot-reload:

```bash
cd parc-gui/ui && npx tauri dev
```

The frontend is vanilla TypeScript with web components and zero runtime npm dependencies. The backend is Rust with `tauri` v2.

### System dependencies

| OS | Package |
|----|---------|
| Arch Linux | `sudo pacman -S webkit2gtk-4.1` |
| Debian / Ubuntu | `sudo apt install libwebkit2gtk-4.1-dev` |
| macOS | Built in |
| Windows | Built in (WebView2) |

## Features

### Fragment management

- List view with type, status, priority, due date, and tag chips
- Schema-driven form editor — every field uses the right input control
- Live Markdown preview alongside the editor
- Per-fragment delete, archive, and trash workflow

### Search

- Full DSL search with the same parser as the CLI
- Autocomplete suggestions for filter keys, tags, and types
- Filter chips that translate clicks into DSL terms
- Saved-query bar (per-vault, stored in `<vault>/config.yml`)

### Graph view

Interactive Canvas 2D force-directed backlink graph. Drag nodes, zoom and pan, click to navigate to a fragment, hover for the title. The graph is computed from the index, not stored — changes show up immediately.

### Tag browser

Cloud and list views. Cloud sizes tags proportionally to their usage count; list view sorts by count or name and supports type filtering.

### History viewer

Timeline of every snapshot for the active fragment, side-by-side diff against any version, one-click restore.

### Attachments

Drag-and-drop file uploads, thumbnail previews for images, click-to-open for the rest.

### Vault switcher

Multi-vault support — switch between known vaults from a dropdown, run reindex and doctor on the active vault, see vault metadata at a glance.

### Command palette

`Ctrl+K` opens a fuzzy command palette covering search, navigation, fragment creation, and vault switching.

### Keyboard-driven

Full shortcut map — press `Ctrl+?` to view it. Vim-style `j` / `k` navigation in lists, `o` / `e` to open or edit, `?` for help.

### Dark mode

System-following by default; manual light/dark toggle in the header. The theme matches the CLI's `color: auto` behaviour.

## Same vaults, same files

The GUI uses the same `.parc` vault format as the CLI — there is no separate database. Open the same vault from `parc list` and `parc-gui` and you see the same fragments. Changes from one are visible in the other after a reindex (or immediately, if both are watching the same SQLite WAL file).
