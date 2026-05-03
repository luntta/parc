---
layout: layouts/doc.njk
title: Terminal UI
eyebrow: CLI · §11
---

A keyboard-driven terminal UI sits inside the same `parc` binary. It uses the same `parc-core` queries as the CLI — there's no separate state — so anything you can do in the TUI you can also reach from a script.

## Launching

```bash
parc          # opens the TUI when stdout is a TTY; otherwise prints `parc today`
parc tui      # always launches the TUI
parc --no-tui # forces the plain `parc today` digest, even in a TTY
```

The CLI auto-falls-back to the `today` digest when output is piped or redirected, so `parc | head` and `parc > digest.txt` both work without dropping you into a UI.

## Layout

A persistent menu strip across the top selects between six tabs. Below it sits a two-pane split: a list panel on the left, a detail panel on the right. Exactly one pane is _focused_ at a time — its border is highlighted and navigation keys (arrows, page, etc.) act on it. A footer strip at the bottom shows the current focus and a key cheat-sheet.

| Tab | Number | What it shows |
|-----|--------|---------------|
| **Today** | `1` | The same three sections as `parc today` (touched today / due / open & high priority) |
| **List** | `2` | Recent fragments, newest first |
| **Stale** | `3` | The same set as `parc stale` |
| **Due** | `4` | Open todos due this week, grouped by overdue / today / soon |
| **Review** | `5` | The same multi-section digest as `parc review` |
| **Search** | `6` | The most recent launcher fragment search results |

The detail pane renders the selected fragment's body as styled Markdown — headings, lists, blockquotes, code, inline code, bold/italic, links, and `[[id]]` wiki-links are highlighted. A scrollbar appears when the body overflows the pane.

`/` opens a two-pane universal launcher over the current view. Plain input searches fragments with the full DSL and also matches commands/views, so typing `review`, `due`, `archive`, or `#backend` surfaces the relevant action or fragment result. When a fragment is selected, typed field intents such as `status done`, `due friday`, `priority high`, or `assignee alice` appear as runnable actions; partial inputs like `status `, `priority h`, `due tom`, or `assignee al` suggest schema-aware values. Tag actions are explicit to avoid accidental replacement: `add tag tui`, `remove tag old`, or `set tags tui backend`. The right pane previews the highlighted fragment, command, or action; field-action previews show validation errors or normalized due dates before you run them. `Enter` on a fragment opens the result set in the Search tab; `Enter` on a command/action runs it. `Ctrl-P` opens the same launcher with `>` prefilled for command-only filtering; typing after `>` filters commands.

## Keybindings

### Navigation

| Key | Action |
|-----|--------|
| `1` / `2` / `3` / `4` / `5` / `6` | Jump to a tab |
| `Tab` | Cycle to the next tab |
| `Shift-Tab` | Toggle pane focus (list ↔ detail) |
| `/` | Open the universal launcher |
| `Ctrl-P` | Open the launcher command-only |
| `↓` / `↑` | Move within the focused pane |
| `PgDn` / `PgUp` | Page within the focused pane |
| `Home` / `End` | Top / bottom of the focused pane |
| `Ctrl-d` / `Ctrl-u` | Half-page scroll within the focused pane |
| `r` | Reload the current tab |

### Actions on the selected fragment

| Key | Action |
|-----|--------|
| `e` | Edit in `$EDITOR` (suspends the TUI, resumes on exit) |
| `t` | Toggle todo status (open ↔ done) — no-op for non-todos |
| `s` | Set status |
| `D` | Set due date |
| `P` | Set priority |
| `@` | Set assignee |
| `#` | Set tags |
| `p` | Promote to another type — opens an input prompt |
| `a` | Archive (toggle) |
| `d` | Delete — opens a `y/n` confirm dialog |
| `y` | Copy the full ID to the system clipboard |

### Modals and general

| Key | Action |
|-----|--------|
| `?` | Toggle the help overlay |
| `Esc` | Cancel a modal; close the launcher |
| `y` / `n` | Confirm or cancel inside a confirm dialog |
| `Enter` | Submit inside an input prompt |
| `q` | Quit |
| `Ctrl-C` | Quit from anywhere |

In universal launcher mode, all printable keystrokes append to the query. The query is parsed as the full [search DSL]({{ '/search-dsl/' | url }}), so `type:todo #backend due:this-week` filters fragments as you type, while command/view/action matches are mixed into the same result list. Use `Shift-Tab` to move focus between results and preview, `Enter` to open a selected fragment search result in the Search tab or run a selected command/action, and `Esc` to close the launcher.

If the launcher input starts with `>`, it switches to command-only mode. Command results include existing TUI actions such as edit, toggle status, archive, delete, promote, yank ID, quick field edits, capture, reload, help, and tab switching. Commands that need a selected fragment are hidden when nothing is selected.

## When to use it

The TUI shines for browsing and quick edits — flipping between tabs to see what you've been doing, using the launcher to search or run actions, and editing or toggling todos without leaving the keyboard. For one-shot queries, scripted output, or piping into other tools, the CLI is faster: every persistent TUI tab maps directly to a resurfacing/list command, and every TUI action maps to `parc edit`, `parc set`, `parc archive`, `parc delete`, or `parc promote`.
