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

A persistent menu strip across the top selects between three tabs. Below it sits a two-pane split: a list panel on the left, a detail panel on the right. Exactly one pane is _focused_ at a time — its border is highlighted and navigation keys (arrows, page, etc.) act on it. A footer strip at the bottom shows the current focus and a key cheat-sheet.

| Tab | Number | What it shows |
|-----|--------|---------------|
| **Today** | `1` | The same three sections as `parc today` (touched today / due / open & high priority) |
| **List** | `2` | Recent fragments, newest first |
| **Stale** | `3` | The same set as `parc stale` |

The detail pane renders the selected fragment's body as styled Markdown — headings, lists, blockquotes, code, inline code, bold/italic, links, and `[[id]]` wiki-links are highlighted. A scrollbar appears when the body overflows the pane.

`/` or `4` opens a two-pane search popup over the current view. The query field stays active while the left pane lists live DSL search results and the right pane previews the selected fragment with matched terms highlighted.

## Keybindings

### Navigation

| Key | Action |
|-----|--------|
| `1` / `2` / `3` | Jump to a tab |
| `Tab` | Cycle to the next tab |
| `Shift-Tab` | Toggle pane focus (list ↔ detail) |
| `/` / `4` | Open the search popup |
| `↓` / `↑` | Move within the focused pane |
| `PgDn` / `PgUp` | Page within the focused pane |
| `Home` / `End` | Top / bottom of the focused pane |
| `Ctrl-d` / `Ctrl-u` | Half-page scroll within the focused pane |
| `r` | Reload the current tab (Today / List / Stale) |

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
| `Esc` | Cancel a modal; close the search popup |
| `y` / `n` | Confirm or cancel inside a confirm dialog |
| `Enter` | Submit inside an input prompt |
| `q` | Quit |
| `Ctrl-C` | Quit from anywhere |

In the search popup, all printable keystrokes append to the query. The query is parsed as the full [search DSL]({{ '/search-dsl/' | url }}), so `type:todo #backend due:this-week` filters the list as you type. Use `Shift-Tab` to move focus between results and preview, `Enter` to edit the selected result, and `Esc` to close the popup.

## When to use it

The TUI shines for browsing and quick edits — flipping between tabs to see what you've been doing, opening search to refine, and editing or toggling todos without leaving the keyboard. For one-shot queries, scripted output, or piping into other tools, the CLI is faster: every persistent TUI tab maps directly to a `parc today`, `parc list`, or `parc stale` invocation, and every TUI action maps to `parc edit`, `parc set`, `parc archive`, `parc delete`, or `parc promote`.
