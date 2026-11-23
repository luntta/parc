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

**Go to…** (Ctrl+K) is one box for fragments and commands, ranked together like the TUI's launcher: type part of a title to open a fragment with the caret in its text, or a command (new todo, switch vault, archive, history, settings…). The list is on the left; the right shows the selected fragment as it reads (type, status, title and text, read-only and as saved) or what a command does, and PgUp/PgDn scroll it. Start with `>` for commands only (Ctrl+Shift+P opens it that way).

Start with `/` to search inside fragments, like `/` in Vim (Ctrl+Shift+F opens it that way): every line of every fragment body that contains the text, listed under its fragment with `line:column` and the match marked. The text is matched exactly, ignoring case unless you type an uppercase letter. The preview marks every match and centres the selected one; Enter opens the fragment with that match selected. Lines count from the top of the text, as the editor shows it.

The sidebar lists every fragment in scope, archived ones included and marked, with its type's glyph, status and age; titles get two lines. The box above it filters as you type: plain words match titles, types, statuses, tags and vault names; anything in the search language (`type:todo`, `#tag`, `due:overdue`) asks the vault, with completion as in Search. **Open** hides done, closed and archived work. The button beside it groups the list by type (the default), by tag (a fragment with several tags is listed under each of them, untagged ones under *No tags*) or not at all, and sorts it by last edit (the default), creation date or title, newest or oldest first (A to Z or Z to A for titles); in creation order, each row's age is how long ago it was created. Click a group's header to fold it. The list remembers its grouping, order and folded groups on this computer, and Go to… offers the same choices. ↑/↓ move through the list and its group headers (↓ from the filter enters it), ← folds a group and → opens it, and Enter opens a fragment with the caret in its text. Drag the sidebar's edge to resize it (double-click to reset), or hide it with Ctrl+\\.

| Key | Does |
|-----|------|
| Ctrl+K | Go to a fragment or command |
| Ctrl+Shift+F | Search inside fragments (Go to… with `/`) |
| Ctrl+N or + | New capture |
| Ctrl+F or / | Search |
| Ctrl+S or Ctrl+Enter | Save |
| Ctrl+\\ | Hide or show the sidebar |
| G, A, P | Graph, Agent, Pulse |
| T | Toggle the open fragment's status (schema `toggle:`) |

Single keys work outside text fields; the Ctrl ones work anywhere (Cmd on macOS).

- **Search** — parc’s search language against the selected scope. Results update as you type; Tab completes filter names and their values (types, statuses, priorities, tags, date words) from the vault's schemas, and the chips add common filters.
- **Graph** — every fragment in scope and the links between them, laid out once and drawn to pan (drag) and zoom (scroll). Hover or focus a fragment to pick out its neighbours; select one to open it. Fragments without links are left out until **Show unlinked**. Each saved fragment also has a **Graph** section under its text with its own linked fragments.
- **Agent** — inspect the saved fragment context; no model is connected or sent data.
- **Pulse** — open work, counts by type, and fragments by the week they were last edited. Each count opens the search that lists what it counts.

[Aihio](https://luntta.fi/aihio/) provides UI components and theme tokens (light and dark follow the system); CodeMirror 6 provides source-preserving Markdown editing. Inter and JetBrains Mono ship from `node_modules` (`fonts.css`) and lead Aihio's font tokens, because Chromium on Linux otherwise resolves Aihio's `-apple-system` stack to the desktop default, often Liberation Sans. `npm start` bundles the renderer locally with esbuild. No CDN is used and the page has no network access; the app's only network use is a speech model download you start in Settings.

## Quick capture

A small window over whatever you're doing: pick a vault and type, write, Ctrl+S, and it closes. Esc asks before dropping text; text in progress is kept if the window closes some other way. Open it from the sidebar, from Go to…, with a global shortcut set in **Settings** (for example `CommandOrControl+Shift+Space`), or by launching the app with `--capture`:

```sh
./node_modules/.bin/electron . --capture
```

A second launch hands `--capture` to the running app; with none running it opens only the capture window. Some desktops, including many Wayland compositors, don't let apps register global shortcuts: bind that command in the desktop's own shortcut settings instead (Settings shows the exact command).

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

The smoke test opens a temporary Electron window with a synthetic microphone and a stub recognizer. It records, converts the recording to WAV in the renderer, saves, reopens and edits Markdown, searches the new revision, captures a todo and toggles it, reloads after an outside save, restores a draft across a window reload, checks cross-vault browsing, save routing and both graphs, then opens a fragment from Go to… (checking its preview), greps for a heading and opens the match, filters the sidebar, groups it by type and by tag, folds a group and sorts by title and by creation, restores a version from History, archives and trashes with Undo, searches as it types and saves from the quick capture window, then adds two dictation languages in Settings (checking the model list and its recommendation) and dictates in each. It does not test real speech accuracy or microphone hardware.

Scope: a desktop workspace prototype, not a packaged installer, LLM session or historical-recall engine. The next harness increment should consume shared, cited results through parc-server rather than keeping a separate desktop-only archive.
