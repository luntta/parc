# parc Capture

A local-first Electron workspace for Linux and macOS. Choose a vault from the switcher at the top of the sidebar, select a fragment, and edit its Markdown in place. The active line exposes its syntax; other lines style headings, emphasis, links, lists and task markers without changing the source. Task boxes toggle with a click, `#tags` are marked, and `[[id]]` links show the title of the fragment they point at (click to open it; a prefix that matches more than one fragment stays unresolved). Typing `[[` opens a link picker over the vault's fragments, recent first; keep typing to filter by title, tag, type or ID, and press Enter or Tab to insert the shortest unique `[[id]]`. **Save** (Ctrl+S or Ctrl+Enter; Cmd on macOS) commits a revision. New captures can be typed or dictated; if you omit a title the first body line supplies one. The window's title names the open fragment, with a dot while it has unsaved changes.

### Types and fields

A new capture starts as the vault's default type (a note unless Vault settings say otherwise); its badge (**New note ▾**) picks any type the vault has switched on. The row under the badge holds the type's fields: enum fields such as status, priority, likelihood or impact open a menu of the schema's values, date fields such as due open a calendar, and tags are added with the tag button and removed with their ×. On a saved fragment each field change is saved at once, as it is in `parc tui`; the text keeps its own Save. On a new capture they wait for its first save. When the schema declares a `toggle:` (todos flip open/done, decisions proposed/accepted, risks identified/resolved), the status mark toggles it, and so does **T** outside the editor.

### Vault settings

**Vault settings…** (in the vault switcher, in Go to…, or from Settings) shows the vault's types. It is kept in the vault itself (`config.yml` and `schemas/`), so everyone who opens the vault, from the app or the CLI, gets the same types.

- **Switch a type off** and new captures can't use it: the type menu, Go to… and quick capture stop offering it, and the CLI and server refuse it. Its fragments stay as they are: they open, change and turn up in searches. The type new captures start as can't be switched off; choose another under **New captures → Start as** first.
- **Edit** a type to change its fields: add one (Text, Date, Choice or List), make it required, give it a default, or remove it (its values stay in the files). A choice field's values can be renamed, added and removed. Renaming a value moves the fragments that hold it. Removing a value that fragments hold asks where they go; Save refuses to leave a fragment invalid. On the status field, each value says what it means (Open, Active, Done or Closed), which drives status colours, the **Open** filter and the open-work counts in Pulse, and **T toggles** picks the pair the status mark and **T** flip between. Built-in types have **Restore built-in**; a type no fragment uses can be deleted.
- **New type** starts with a status field (open ⇄ done) that you can change or remove. Its name is fixed once created, because fragments and hooks refer to it; its shortcut (`parc b "…"` in a terminal) can change.

### History, archive and trash

The **⋯** menu of an open fragment has **History**, **Copy link** (its shortest `[[id]]`), **Archive** and **Move to trash**; the "Edited …" time in the status bar opens History too. History lists earlier versions newest first and compares the one you select with the current version, field by field and line by line; **Restore this version** puts it back (the version it replaces stays in History). Archived fragments stay in the list, marked, and leave default searches. Archiving and trashing offer **Undo** in the status bar; trashed fragments can also come back with `parc trash --restore`.

### Drafts and changes from elsewhere

Unsaved text is kept locally as you type, so closing the window or a crash doesn't lose it: the next launch reopens the draft. Leaving a fragment with unsaved changes asks whether to save them. Each opened vault is watched: a save from the CLI, the TUI, another app or a `git pull` refreshes the list (and search), and reloads the open fragment if you haven't edited it. If you have, Save asks before overwriting the newer version (it stays in the fragment's history either way). The check compares the file's exact contents, so it also catches a save made in the same second. The app reopens the vault you used last.

## Start

From the repository root, with Rust and Node 22.12+:

```sh
cargo build -p parc-cli -p parc-server
cd parc-desktop
npm ci
npm start
```

The default vault is `prototype/.parc` under Electron’s user data directory. Hover over a vault in the switcher to see its path; each vault's mark has its own colour. If parc's global vault (`~/.parc`) exists, the switcher offers it by name. **Open vault folder…** accepts an existing `.parc` or its parent; only open trusted vaults, because writes run vault hooks. Opened vaults are remembered locally, not discovered by scanning your machine. **All opened vaults** combines fragment lists without merging the vaults. New captures in that view require an explicit destination; existing fragments always save back to their original vault. Writes use the standard `fragment.create` / `fragment.update` RPCs and preserve type-specific metadata; every write names the revision it was based on (`expected_revision`), so a newer save from elsewhere is never silently overwritten. The app prefers `../target/debug/{parc,parc-server}`, falling back to PATH.

### Moving around

The app opens on **Pulse**, the first item in the sidebar: where the vault stands today. Switching vaults lands on the new vault's Pulse, or keeps you in a type's view, the Graph or a blank capture.

**Go to…** (Ctrl+K) is one box for fragments and commands, ranked together like the TUI's launcher: type part of a title to open a fragment with the caret in its text, or a command (new todo, switch vault, archive, history, settings…). The list is on the left; the right shows the selected fragment as it reads (type, status, title and text, read-only and as saved) or what a command does, and PgUp/PgDn scroll it. Start with `>` for commands only (Ctrl+Shift+P opens it that way).

Start with `/` to search inside fragments, like `/` in Vim (Ctrl+Shift+F opens it that way): every line of every fragment body that contains the text, listed under its fragment with `line:column` and the match marked. The text is matched exactly, ignoring case unless you type an uppercase letter. The preview marks every match and centres the selected one; Enter opens the fragment with that match selected. Lines count from the top of the text, as the editor shows it.

The compose button beside the vault switcher starts a new capture. Below it, the sidebar lists every fragment in scope, archived ones included and dimmed, one line each: a mark, the title and its age (`now`, `5m`, `3h`, `2d`, then a date). The mark says what the group header doesn't. Grouped by type it is the status: a ring for open work, a half-filled ring for active, a check for done and a dashed ring for closed, from each status's category in the schema. Otherwise it is the type's glyph. Grouped by type, each group's header opens that type's own view (see below); its fold mark is at the end of the header. In **All opened vaults** a dot in the vault's colour sits before the age. Hover a row for its full title, type, status and tags. The opened fragment is scrolled into view when you open it from elsewhere. The box above the list filters as you type and shows how many fragments match: plain words match titles, types, statuses, tags and vault names; anything in the search language (`type:todo`, `#tag`, `due:overdue`) asks the vault, with completion as in Search. **Open** hides done, closed and archived work. The button beside it groups the list by type (the default), by tag (a fragment with several tags is listed under each of them, untagged ones under *No tags*) or not at all, and sorts it by last edit (the default), creation date or title, newest or oldest first (A to Z or Z to A for titles); in creation order, each row's age is how long ago it was created. Click a tag group's header, or any group's fold mark, to fold it; a folded group keeps its header and count. The list remembers its grouping, order and folded groups on this computer, and Go to… offers the same choices. ↑/↓ move through the list and its group headers (↓ from the filter enters it), ← folds a group and → opens it, and Enter opens a fragment with the caret in its text. Drag the sidebar's edge to resize it (double-click to reset), or hide it with Ctrl+\\.

| Key | Does |
|-----|------|
| Ctrl+K | Go to a fragment or command |
| Ctrl+Shift+F | Search inside fragments (Go to… with `/`) |
| Ctrl+N or + | New capture |
| Ctrl+F or / | Search |
| Ctrl+S or Ctrl+Enter | Save |
| Ctrl+\\ | Hide or show the sidebar |
| P, G, A | Pulse (home), Graph, Agent |
| Shift+← → (↑ ↓) | Move the focused card to another column (matrix: cell) |
| T | Toggle the open fragment's status (schema `toggle:`) |

Single keys work outside text fields; the Ctrl ones work anywhere (Cmd on macOS).

- **Search** — parc’s search language against the selected scope. Results update as you type; Tab completes filter names and their values (types, statuses, priorities, tags, date words) from the vault's schemas, and the chips add common filters.
- **Graph** — every fragment in scope and the links between them, laid out once and drawn to pan (drag) and zoom (scroll). Hover or focus a fragment to pick out its neighbours; select one to open it. Fragments without links are left out until **Show unlinked**. Each saved fragment also has a **Graph** section under its text with its own linked fragments.
- **Agent** — inspect the saved fragment context; no model is connected or sent data.
- **Pulse** — open work, counts by type, and fragments by the week they were last edited. It redraws when the vault changes. Open todos, open risks and each type's count open that type's view; the other counts open the search that lists what they count.

### A type's own view

Each type has a view of its own: open it from its group header in the sidebar (grouped by type), from a count in Pulse, or by its name in Go to… ("Todos", "Risks").

- **Board** — a type with a status (todos, decisions, ideas, risks and any type you add) gets a column per status value, in the schema's order and marked with its meaning (open, active, done, closed). Drag a card to another column to change its status, or focus it and press Shift+← or Shift+→; arrows move between cards and Enter opens one. The card moves at once and the change is saved like any field edit, only the status, so nothing else in the file is touched; **Undo** in the status bar moves it back. Cards show the title, high priority, the due date (red when overdue), severity for risks, a couple of tags and the age, in the sidebar's order. A done or closed column shows its latest eight until you ask for the rest, and the **+** on a column starts a new fragment with that status.
- **Matrix** — a type with likelihood and impact fields (risks) opens on a risk matrix: likelihood up the side, impact along the bottom, both from the schema's values, so the most serious risks sit top right. Each cell is tinted by severity (Minor, Moderate, Major, Severe: the product of both positions on their scales). Drag a risk to another cell, or press Shift with an arrow, to reassess it; risks without both fields wait under the matrix in **Not assessed**. Beside it, **Most serious first** ranks the assessed risks with their status and mitigation. Done and closed risks stay out unless **Settled** is on. **Matrix** and **Board** switch between the two lenses; the app remembers the choice on this computer.
- **Cards** — a type without a status (notes) lists its fragments as cards.

Archived fragments stay out of these views, as they do in default searches.

[Aihio](https://luntta.fi/aihio/) provides UI components and theme tokens (light and dark follow the system unless **Settings → Appearance** picks Light or Dark, which the app applies to every window through Electron’s `nativeTheme`; a pick shows at once and Cancel puts the saved theme back); CodeMirror 6 provides source-preserving Markdown editing. Inter and JetBrains Mono ship from `node_modules` (`fonts.css`) and lead Aihio's font tokens, because Chromium on Linux otherwise resolves Aihio's `-apple-system` stack to the desktop default, often Liberation Sans. `npm start` bundles the renderer locally with esbuild. No CDN is used and the page has no network access; the app's only network use is a speech model download you start in Settings.

## Quick capture

A small window over whatever you're doing: pick a vault and type, write, Ctrl+S, and it closes. Esc asks before dropping text; text in progress is kept if the window closes some other way. Open it from the sidebar, from Go to…, with a global shortcut set in **Settings** (for example `CommandOrControl+Shift+Space`), or by launching the app with `--capture`:

```sh
./node_modules/.bin/electron . --capture
```

A second launch hands `--capture` to the running app; with none running it opens only the capture window. Either way it is quick enough that the app doesn't need to keep running in the background: on Hyprland the window appeared about 140 ms after the command with the app running, about 230 ms from a cold start, and about 300 ms with the app's files dropped from the disk cache, as after login. The text box has focus by the time the window shows.

On Wayland, bind that command in the desktop's own shortcut settings: **Settings → Quick capture** shows it with a **Copy** button. The app's own shortcut goes through the desktop portal, which may refuse it without saying so (Hyprland's refuses apps it can't match to an installed `.desktop` file, as with `npm start`), so Settings only says the desktop was asked. On X11, macOS and Windows, Electron registers the app's own shortcut directly and says when it can't.

### Hyprland

Hyprland 0.56 reads Lua. Add this to your config (on Omarchy, `~/.config/hypr/bindings.lua`), with the command from Settings:

```lua
-- Quick capture on Super+N: focus the capture window if it's open, otherwise open one.
-- On Wayland the app can't raise its own window, so the binding does.
local parc_capture = "<the command from Settings → Quick capture>"
hl.bind("SUPER + N", function()
  for _, window in ipairs(hl.get_windows()) do
    if window.class == "parc-capture" and window.title == "Quick capture — parc" then
      return hl.dispatch(hl.dsp.focus({ window = "address:" .. window.address }))
    end
  end
  hl.exec_cmd(parc_capture)
end, { description = "parc quick capture" })

-- Float it over whatever you're doing, centred, on every workspace. Without this it tiles.
hl.window_rule({
  match = { class = "^parc-capture$", title = "^Quick capture — parc$" },
  float = true, center = true, size = { 620, 440 }, pin = true,
})
```

Both match the capture window by its title, `Quick capture — parc`; the main window shares the `parc-capture` class. Check the config with `hyprctl configerrors`.

## Local dictation (optional)

Install **whisper.cpp** so its `whisper-cli` program is on your PATH (e.g. `brew install whisper-cpp` on macOS, or your Linux distribution's package or an upstream build), or choose where it is in **Settings**. ffmpeg is not needed: the app converts recordings to the 16 kHz WAV whisper.cpp reads.

Then open **Settings → Dictation**:

- **Languages you dictate in.** Add one or more. With one, every recording is in it; with several, a picker next to **Dictate** chooses the language of the next recording (the first is the default, and **Detect automatically** is always offered). With none, whisper detects the language.
- **Speech model.** Five whisper.cpp models are listed with their size, and one is marked **Recommended** for your languages: an English-only model if you only dictate English (faster, and better at English), otherwise Large v3 Turbo, which is the most accurate for other languages but slower on computers without a GPU. **Download** fetches it from Hugging Face with progress and **Cancel**; a cancelled or interrupted download resumes where it stopped. The file is kept only if its SHA-256 matches the hash pinned in the app, and is stored under `models/` in Electron’s user data directory. The first model you download is the one dictation uses; **Use** switches between downloaded models, and the trash button deletes one. You can also choose a model file you already have.

Downloading a model is the only time the app uses the network, and only when you press **Download**. If an English-only model is chosen while you dictate in other languages, Settings and the Dictate button say so (it would write English regardless). `PARC_WHISPER_MODEL` and `PARC_WHISPER_BIN` still work for terminal launches; Settings wins when both are set. Until dictation is ready the button reads **Set up dictation** and opens Settings.

**Dictate** checks the configuration before opening the mic. Allow microphone access when prompted (macOS: System Settings → Privacy & Security → Microphone). Click **Stop & transcribe** or let the two-minute limit stop it; **Cancel recording** or Escape discards the recording. whisper-cli transcribes the audio on your machine, in the chosen language. The draft remains editable. There is no remote speech API and no automatic note save. The temporary WAV and transcript are removed after processing, including error exits; a force-kill or system crash may leave a `parc-speech-*` temp directory behind. Typing needs no microphone, speech model or credentials.

Settings live in `settings.json` beside `vaults.json` in Electron’s user data directory.

## Verify

After building the Rust binaries:

```sh
npm run lint:ui      # check both pages' markup against Aihio's schema
npm test             # temporary vault round trips; transcription cleanup & errors
npm run test:smoke   # requires a graphical session
```

The smoke test opens a temporary Electron window with a synthetic microphone and a stub recognizer. It checks that the app starts on Pulse, records, converts the recording to WAV in the renderer, saves, reopens and edits Markdown, searches the new revision, captures a todo and toggles it, reloads after an outside save, restores a draft across a window reload, checks cross-vault browsing, save routing and both graphs, then opens a fragment from Go to… (checking its preview), greps for a heading and opens the match, filters the sidebar, groups it by type and by tag, folds a group and sorts by title and by creation, moves a todo across its board by drag and drop and by Shift+→ and undoes it, places a risk on the matrix and steps it down a row, switches to the risk board and opens it from Pulse, restores a version from History, archives and trashes with Undo, searches as it types and saves from the quick capture window, copies the quick capture command in Settings, previews a theme and cancels it, then adds two dictation languages (saving Dark with them) (checking the model list and its recommendation) and dictates in each. It does not test real speech accuracy or microphone hardware.

Scope: a desktop workspace prototype, not a packaged installer, LLM session or historical-recall engine. The next harness increment should consume shared, cited results through parc-server rather than keeping a separate desktop-only archive.
