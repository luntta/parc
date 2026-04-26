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

A persistent menu strip across the top selects between four tabs. The list panel on the left shows fragments matching the active tab; the detail panel on the right shows the highlighted fragment.

| Tab | Number | What it shows |
|-----|--------|---------------|
| **Today** | `1` | The same three sections as `parc today` (touched today / due / open & high priority) |
| **List** | `2` | Recent fragments, newest first |
| **Stale** | `3` | The same set as `parc stale` |
| **Search** | `4` or `/` | Live DSL search — type to filter, results refresh per keystroke |

## Keybindings

| Key | Action |
|-----|--------|
| `1` / `2` / `3` / `4` | Jump to a tab |
| `Tab` | Cycle to the next tab |
| `/` | Jump to Search and start typing |
| `j` / `↓` | Move selection down |
| `k` / `↑` | Move selection up |
| `r` | Reload the current tab (Today / List / Stale) |
| `Backspace` | Delete a character in Search |
| `Esc` | Clear the Search input |
| `q` | Quit |
| `Ctrl-C` | Quit |

In Search, all printable keystrokes append to the query. The query is parsed as the full [search DSL]({{ '/search-dsl/' | url }}), so `type:todo #backend due:this-week` filters the list as you type.

## When to use it

The TUI shines for browsing — flipping between tabs to see what you've been doing, jumping into Search to refine, and seeing the rendered detail view without reaching for a separate command. For one-shot queries, scripted output, or piping into other tools, the CLI is faster: every TUI tab maps directly to a `parc today`, `parc list`, `parc stale`, or `parc search` invocation.
