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

### Related

Beside the text (under it in a narrow window), **Related** lists the fragments that bear on what you are writing, however old: up to five, from the open fragment's vault, or from every vault in view for a new capture. It asks again after each short pause in typing, as soon as a dictation lands, and when the vault changes on disk. Each source says why it is there (*Shares* words, *Tagged* with a tag you used, *Linked* either way, *Similar* in meaning) and quotes the line of its text with the most shared words, marked. A source that is still related keeps its place as you write; new ones join below, so the list doesn't jump. The panel never takes the caret. Click a source to read it in place without leaving your draft; from there **Link** puts its `[[id]]` at the caret and **Open** goes to it. It finds notes that share words, tags or links and, once **Settings → Recall** has downloaded the meaning model (480 MB, once per computer), notes that say the same thing in other words or another language. Each vault's server reads its notes for meaning in the background, at low priority; Settings shows how far it has got. It is the server's `recall.context`, as `parc recall` and the MCP `recall` tool use it.

### Drafts and changes from elsewhere

Unsaved text is kept locally as you type, in every tab, so closing the window or a crash doesn't lose it: the next launch reopens your tabs as you left them, drafts included. Closing a tab with unsaved changes asks whether to save them. Each opened vault is watched: a save from the CLI, the TUI, another app or a `git pull` refreshes the list (and search), and reloads each open fragment you haven't edited. If you have, Save asks before overwriting the newer version (it stays in the fragment's history either way). The check compares the file's exact contents, so it also catches a save made in the same second. The app reopens the vault you used last.

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

The app opens on **Pulse**, the first item in the sidebar: where the vault stands today. Switching vaults keeps your tabs: a fragment stays in its own vault, and views redraw for the new scope. If the current tab is a fragment from a vault the new scope leaves out, the new vault's Pulse takes its place.

**Go to…** (Ctrl+K) is one box for fragments and commands, ranked together like the TUI's launcher: type part of a title to open a fragment with the caret in its text, or a command (new todo, switch vault, archive, history, settings…). The list is on the left; the right shows the selected fragment as it reads (type, status, title and text, read-only and as saved) or what a command does, and PgUp/PgDn scroll it. Start with `>` for commands only (Ctrl+Shift+P opens it that way).

Start with `/` to search inside fragments, like `/` in Vim (Ctrl+Shift+F opens it that way): every line of every fragment body that contains the text, listed under its fragment with `line:column` and the match marked. The text is matched exactly, ignoring case unless you type an uppercase letter. The preview marks every match and centres the selected one; Enter opens the fragment with that match selected. Lines count from the top of the text, as the editor shows it. Ctrl+Enter opens the selected fragment beside, in the next group.

The compose button beside the vault switcher starts a new capture. Below it, the sidebar lists every fragment in scope, archived ones included and dimmed, one line each: a mark, the title and its age (`now`, `5m`, `3h`, `2d`, then a date). The mark says what the group header doesn't. Grouped by type it is the status: a ring for open work, a half-filled ring for active, a check for done and a dashed ring for closed, from each status's category in the schema. Otherwise it is the type's glyph. Grouped by type, each group's header opens that type's own view (see below); its fold mark is at the end of the header. In **All opened vaults** a dot in the vault's colour sits before the age. Hover a row for its full title, type, status and tags. The opened fragment is scrolled into view when you open it from elsewhere. The box above the list filters as you type and shows how many fragments match: plain words match titles, types, statuses, tags and vault names; anything in the search language (`type:todo`, `#tag`, `due:overdue`) asks the vault, with completion as in Search. **Open** hides done, closed and archived work. The button beside it groups the list by type (the default), by tag (a fragment with several tags is listed under each of them, untagged ones under *No tags*) or not at all, and sorts it by last edit (the default), creation date or title, newest or oldest first (A to Z or Z to A for titles); in creation order, each row's age is how long ago it was created. Click a tag group's header, or any group's fold mark, to fold it; a folded group keeps its header and count. The list remembers its grouping, order and folded groups on this computer, and Go to… offers the same choices. ↑/↓ move through the list and its group headers (↓ from the filter enters it), ← folds a group and → opens it, and Enter opens a fragment with the caret in its text (Ctrl+Enter: beside). Drag the sidebar's edge to resize it (double-click to reset), or hide it with Ctrl+\\ or the sidebar button beside the vault switcher (it shows while you point at the sidebar); while the sidebar is hidden, the same button starts the tabs and brings it back.

| Key | Does |
|-----|------|
| Ctrl+K | Go to a fragment or command |
| Ctrl+Shift+F | Search inside fragments (Go to… with `/`) |
| Ctrl+N or + | New capture |
| Ctrl+F or / | Search |
| Ctrl+S or Ctrl+Enter | Save |
| Ctrl+\\ | Hide or show the sidebar |
| Ctrl+W | Close the tab |
| Ctrl+Tab, Ctrl+PgDn / PgUp | Next or previous tab in the group |
| Alt+1–9 | A tab in the group (9: the last; not on macOS) |
| Ctrl+1–4 | Go to a group (one past the last opens a new one) |
| Ctrl+Alt+← → | Move the tab to the group on that side |
| Ctrl+Shift+T | Reopen the last closed tab |
| Ctrl+click | Open beside, in the next group |
| P, G, A | Pulse (home), Graph, Agent |
| Shift+← → (↑ ↓) | Move the focused card to another column (matrix: cell) |
| T | Toggle the open fragment's status (schema `toggle:`) |

Single keys work outside text fields; the Ctrl ones work anywhere (Cmd on macOS, except Ctrl+Tab and Ctrl+PgDn/PgUp).

- **Search** — parc’s search language against the selected scope. Results update as you type; Tab completes filter names and their values (types, statuses, priorities, tags, date words) from the vault's schemas, and the chips add common filters.
- **Graph** — every fragment in scope and the links between them, laid out once and drawn to pan (drag) and zoom (scroll). Hover or focus a fragment to pick out its neighbours; select one to open it. Fragments without links are left out until **Show unlinked**. Each saved fragment also has a **Graph** section under its text with its own linked fragments.
- **Agent** — ask about your notes with your ChatGPT plan. The agent searches and reads fragments, can't change them, and cites the ones it used. See [Agent](#agent-optional).
- **Pulse** — open work, counts by type, and fragments by the week they were last edited. It redraws when the vault changes. Open todos, open risks and each type's count open that type's view; the other counts open the search that lists what they count.

### Tabs and split view

Fragments and views open in tabs, as in a code editor. The work area is a panel lying beside the sidebar, and its tabs are its top edge; there is no bar above them. It holds up to four groups side by side; each has its own strip of tabs and shows one of them. The current group, whose shown tab is filled (another group's shown tab is fainter), is the one Save, Dictate, the **⋯** menu, **T** and Go to… act on, and Dictate, **⋯** and Save sit at the end of its strip; click or focus anything in a group to make it current. With more than one vault open, a fragment names its vault beside its ID, and the status bar says when it has unsaved changes.

- **Preview tabs.** A click in the list, the Graph, a board or a `[[link]]` opens the fragment as the group's preview, its name in italics: the next click replaces it, so browsing doesn't pile up tabs. Typing in it, a double-click (on its row or its tab), or moving it keeps it. Views (Pulse, Search, Graph, a type's view) open the same way, and a search you type into keeps its tab. A new capture starts as a preview too, so a blank one never lingers.
- **One tab per fragment.** Opening a fragment that is already open shows its tab, in whichever group it is.
- **Beside.** Ctrl+click (⌘ on macOS) a fragment in the list, the Graph, a board or a link, or a nav item, or press Ctrl+Enter in the list or in Go to…, to open it in the group on the right, opening one if there is none. A middle click opens it in a tab of its own. The split button at the end of a strip opens an empty group beside it; what you open next goes there.
- **Moving tabs.** Drag a tab along its strip, onto another group's strip or page, or onto the left or right third of a page to open a new group on that side; a fragment dragged from the list opens the same way. Ctrl+Alt+← and → move the current tab to the next group, opening one at the edge. A group left without tabs closes. Drag the line between groups to resize them (double-click it to share the width equally).
- **Closing.** Ctrl+W, the tab's ×, or a middle click. A tab with unsaved text shows a dot in place of its × and asks before it closes. Right-click a tab (or press the menu key on it) for Close others, Close to the right, Close saved, Close all, Keep open and moving it to another group, and on a fragment Copy link and History. Ctrl+Shift+T reopens the last closed tab.
- **History** opens in a tab of its own, about one fragment.
- The tabs, their groups and widths, and each tab's unsaved text are kept on this computer, so the next launch puts them back.

A view in a group that isn't current stays live: edit a todo on the left and its board on the right moves the card.

### A type's own view

Each type has a view of its own: open it from its group header in the sidebar (grouped by type), from a count in Pulse, or by its name in Go to… ("Todos", "Risks").

- **Board** — a type with a status (todos, decisions, ideas, risks and any type you add) gets a column per status value, in the schema's order and marked with its meaning (open, active, done, closed). Drag a card to another column to change its status, or focus it and press Shift+← or Shift+→; arrows move between cards and Enter opens one. The card moves at once and the change is saved like any field edit, only the status, so nothing else in the file is touched; **Undo** in the status bar moves it back. Cards show the title, high priority, the due date (red when overdue), severity for risks, a couple of tags and the age, in the sidebar's order. A done or closed column shows its latest eight until you ask for the rest, and the **+** on a column starts a new fragment with that status.
- **Matrix** — a type with likelihood and impact fields (risks) opens on a risk matrix: likelihood up the side, impact along the bottom, both from the schema's values, so the most serious risks sit top right. Each cell is tinted by severity (Minor, Moderate, Major, Severe: the product of both positions on their scales). Drag a risk to another cell, or press Shift with an arrow, to reassess it; risks without both fields wait under the matrix in **Not assessed**. Beside it, **Most serious first** ranks the assessed risks with their status and mitigation. Done and closed risks stay out unless **Settled** is on. **Matrix** and **Board** switch between the two lenses; the app remembers the choice on this computer.
- **Cards** — a type without a status (notes) lists its fragments as cards.

Archived fragments stay out of these views, as they do in default searches.

[Aihio](https://luntta.fi/aihio/) provides UI components and theme tokens (light and dark follow the system unless **Settings → Appearance** picks Light or Dark, which the app applies to every window through Electron’s `nativeTheme`; a pick shows at once and Cancel puts the saved theme back). On [Omarchy](https://omarchy.org/), **System** follows your active Omarchy theme live across both the main workspace and Quick Capture — palette colors (with WCAG contrast guards), light/dark mode, Hyprland corner rounding, and Window Opacity (**Opaque**, **Transparent**, and **Glassy** frosted translucency with crisp text). CodeMirror 6 provides source-preserving Markdown editing. Inter and JetBrains Mono ship from `node_modules` (`fonts.css`) and lead Aihio's font tokens, because Chromium on Linux otherwise resolves Aihio's `-apple-system` stack to the desktop default, often Liberation Sans. `npm start` bundles the renderer locally with esbuild. No CDN is used and the page has no network access; the app's only network use is a speech model download you start in Settings.

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

## Agent (optional)

The **Agent** view (**A**) answers questions about your notes with your ChatGPT plan, through OpenAI's *Sign in with ChatGPT*, or with your GitHub Copilot plan, through GitHub's Copilot SDK. ChatGPT works with Go, Plus, Pro and Business plans (in a Business workspace an admin has to allow other apps to use the plan); Copilot with Free, Pro, Pro+, Business and Enterprise. Neither needs an API key. A new chat starts on the provider chosen in the menu above it (the last one you signed in to); a chat stays on the provider it started on.

- **Signing in.** **Continue with ChatGPT** opens the sign-in in your browser, which comes back to parc on `127.0.0.1:1455` (another port when that one is taken). The first sign-in registers parc with your ChatGPT account, and ChatGPT asks whether parc may use your plan; parc refuses a sign-in without that permission. The sign-in is sealed with your system keyring (`chatgpt.bin` in Electron's user data directory) and renewed as it expires; with no keyring it lasts until parc quits. On Linux desktops Chromium doesn't recognise (Hyprland, sway, i3…), parc asks for the Secret Service (gnome-keyring, KeePassXC) itself; `--password-store=…` on the command line overrides that. **Sign out** forgets the sign-in; to disconnect parc from your account, use ChatGPT's settings.
- **Asking.** The agent searches and reads the fragments in view (the vault, or every opened vault) with the read-only tools of `parc mcp --read-only`, served by one `parc-server` per vault. It can't create, change or delete anything. Each search and read is listed above the answer; select one to see what it returned. Citations read as the fragment's title: click one to open it (Ctrl+click: beside). The fragment you used last can go with the question: its chip in the composer (**With …**) shares its saved version, and a click leaves it out (**Add …** puts it back). Enter asks and Shift+Enter starts a new line; ↑ in an empty box brings back the last question. **Stop** ends an answer and takes the question back. The model menu lists the models your plan offers.
- **Chats.** Each Agent tab holds one chat, saved once a question gets an answer and named for its first question. **New chat** (also in Go to…) opens another tab, or shows the group's empty one; an empty chat lists your recent chats under **Recent chats**. Picking one shows its tab if it is open, or brings it back in place, as it was, to go on from (Ctrl+click: beside, so two chats sit side by side). Go to… lists recent chats too, and tabs with chats come back when parc starts. A chat keeps searching the vaults it started in, whatever is in view; if one of them is no longer open in parc, it says so and waits. Delete a chat from the list (the trash button) or its tab's menu; **Undo** brings it back until parc quits. Chats are kept on this computer only, in `chats/` in Electron's user data directory, not in the vault.
- **GitHub Copilot.** **Continue with GitHub Copilot** shows a code and opens GitHub's code page in your browser; type the code there and approve parc. That is GitHub's device sign-in with parc's own GitHub app (it asks only to read your profile); the token is sealed with your keyring like the ChatGPT sign-in (`github.bin`). **Sign out** forgets it; to revoke it, use GitHub's **Settings → Applications**. The answers come from Copilot's own agent, run by the Copilot SDK in a child process with its data under `copilot/` in Electron's user data directory. parc starts it with nothing of its own (no shell, files, instructions or memory) and gives it only the same read-only vault tools, so here too the agent can only search and read. Copilot's models are those your plan offers (Claude, GPT and others); premium requests count as they do in Copilot's other apps.
- **What leaves the computer**, and only when you ask: your question, the shared fragment, and what the agent's searches and reads return. They go to OpenAI's API or to GitHub Copilot and count against that plan's usage. Nothing is stored on OpenAI's side (`store: false`): parc sends the whole chat with each question. When the plan's usage runs out the agent says so; ChatGPT's **Settings → Usage** shows the limits, including the share you allow parc.

### parc's GitHub app

Copilot sign-in uses a GitHub OAuth app registered for parc; its client ID is `GITHUB.clientId` in `copilot.cjs` (public, like every OAuth client ID). With the ID empty, **Continue with GitHub Copilot** is greyed out. A fork that wants its own app registers one like this:

1. Open [New OAuth App](https://github.com/settings/applications/new) (or the same page under an organization's settings, to have the organization own it).
2. **Application name** `parc`; **Homepage URL** the project's page; **Authorization callback URL** any valid URL, e.g. `http://127.0.0.1/` (device sign-in doesn't use it).
3. Tick **Enable Device Flow**, then **Register application**.
4. Copy the **Client ID** (it starts with `Ov23`) into `GITHUB.clientId`. Don't generate a client secret: device sign-in doesn't need one, and an app shipped to users can't keep one.

An organization that restricts third-party apps has to approve parc before its members' Copilot seats work in it.

## Local dictation (optional)

Install **whisper.cpp** so its `whisper-cli` program is on your PATH (e.g. `brew install whisper-cpp` on macOS, or your Linux distribution's package or an upstream build), or choose where it is in **Settings**. ffmpeg is not needed: the app converts recordings to the 16 kHz WAV whisper.cpp reads.

Then open **Settings → Dictation**:

- **Languages you dictate in.** Add one or more. With one, every recording is in it; with several, a picker next to **Dictate** chooses the language of the next recording (the first is the default, and **Detect automatically** is always offered). With none, whisper detects the language.
- **Speech model.** Five whisper.cpp models are listed with their size, and one is marked **Recommended** for your languages: an English-only model if you only dictate English (faster, and better at English), otherwise Large v3 Turbo, which is the most accurate for other languages but slower on computers without a GPU. **Download** fetches it from Hugging Face with progress and **Cancel**; a cancelled or interrupted download resumes where it stopped. The file is kept only if its SHA-256 matches the hash pinned in the app, and is stored under `models/` in Electron’s user data directory. The first model you download is the one dictation uses; **Use** switches between downloaded models, and the trash button deletes one. You can also choose a model file you already have.

Apart from the Agent, downloading a model is the only time the app uses the network, and only when you press **Download**. If an English-only model is chosen while you dictate in other languages, Settings and the Dictate button say so (it would write English regardless). `PARC_WHISPER_MODEL` and `PARC_WHISPER_BIN` still work for terminal launches; Settings wins when both are set. Until dictation is ready the button reads **Set up dictation** and opens Settings.

**Dictate** checks the configuration before opening the mic. Allow microphone access when prompted (macOS: System Settings → Privacy & Security → Microphone). Click **Stop & transcribe** or let the two-minute limit stop it; **Cancel recording** or Escape discards the recording. whisper-cli transcribes the audio on your machine, in the chosen language. The draft remains editable. There is no remote speech API and no automatic note save. The temporary WAV and transcript are removed after processing, including error exits; a force-kill or system crash may leave a `parc-speech-*` temp directory behind. Typing needs no microphone, speech model or credentials.

Settings live in `settings.json` beside `vaults.json` in Electron’s user data directory.

## Verify

After building the Rust binaries:

```sh
npm run lint:ui      # check both pages' markup against Aihio's schema
npm test             # temporary vault round trips; transcription cleanup & errors; the Agent against a fake OpenAI, GitHub and Copilot SDK
npm run test:smoke   # requires a graphical session
```

The smoke test opens a temporary Electron window with a synthetic microphone and a stub recognizer. It checks that the app starts on Pulse, records, converts the recording to WAV in the renderer, saves, reopens and edits Markdown, searches the new revision, captures a todo and toggles it, reloads after an outside save, restores a draft across a window reload, checks cross-vault browsing, save routing and both graphs, then opens a fragment from Go to… (checking its preview), greps for a heading and opens the match, filters the sidebar, groups it by type and by tag, folds a group and sorts by title and by creation, moves a todo across its board by drag and drop and by Shift+→ and undoes it, places a risk on the matrix and steps it down a row, switches to the risk board and opens it from Pulse, restores a version from History, archives and trashes with Undo, searches as it types and saves from the quick capture window, copies the quick capture command in Settings, previews a theme and cancels it, then adds two dictation languages (saving Dark with them) (checking the model list and its recommendation) and dictates in each, and writes a draft whose related fragments appear with their reasons and marked excerpts, keep their places as the draft grows, open in place and link into the draft. Around the first dictation it checks the panel's two empty states. It then works with tabs: closing all of them (each unsaved one asks), previews that the next click replaces, keeping one, opening beside, moving a tab between groups by key and by drag (to a new group, and along a strip), dropping a fragment from the list, closing and reopening, going to a group by key, and the layout and an unsaved draft in a split coming back after a reload. Last, it signs in to a fake ChatGPT from the Agent view, asks a question, checks the search the agent made and opens the fragment its answer cites; then it starts a new chat, goes back to the saved one (its tab, then in place once that is closed), asks a follow-up, finds it again after a reload, and deletes it and undoes that. Then it switches a new chat to GitHub Copilot, signs in through a fake github.com (checking the code it shows), asks a question on a fake Copilot SDK, and checks that the list marks each chat's provider. It does not test real speech accuracy, microphone hardware, or OpenAI's and GitHub's real sign-ins.

Scope: a desktop workspace prototype, not a packaged installer. Recall comes from parc-server (`recall.context`), not from a desktop-only archive.
