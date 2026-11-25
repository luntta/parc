# Roadmap: what's next

Updated 2026-11-25. From a ranked list of what would make parc a world-class app.

## Done

- **Self-correcting index**: `index::open_index` syncs the index with `fragments/` on every open, so no reindex is needed after a pull or an outside edit (f000851).
- **MCP server**: `parc mcp` lets agents use a vault as memory (6ae20a4).
- **Dictation, steps 1–3** (8fa1ee4; real dictation verified 2026-11-25):
  - dictation languages, with a picker next to Dictate and `whisper-cli -l`
  - recordings converted to WAV in the app, so ffmpeg is no longer needed
  - in-app model downloads that resume and are checked against a pinned SHA-256
  - real microphone recording and transcription tested and confirmed working
- **Capture from anywhere, the desktop half** (2026-11-24; Super+N verified 2026-11-25):
  - Hotkey to window, measured on Hyprland: about 140 ms with the app running, 230 ms from a cold start, 300 ms with the app's files dropped from the disk cache. The text box has focus by then. No background mode is needed, so there isn't one.
  - The app's own shortcut doesn't work on Hyprland: the desktop portal refuses it because there is no installed `parc-capture.desktop`, yet Electron reports success. Settings now says the desktop was only asked, and offers the launch command with **Copy**.
  - A Hyprland binding (Lua) that focuses an open capture window or opens one, and a window rule that floats, centres and pins it: in `parc-desktop/README.md`. Tested and verified with a real `Super+N` keypress.
- **Recall, step 1: the benchmark** (2026-11-24):
  - `cargo test -p parc-core --test recall_bench` loads every `parc-core/tests/fixtures/recall/*.json` into one temporary vault, with real ULIDs dated by `created_at`. It then scores each case and compares the result with the committed `scorecard-lexical.txt`. `PARC_RECALL_BLESS=1` saves a new scorecard.
  - 12 cases: the ferry case, the same idea in other words 1 and 3 years later, two notes that disagree, a noisy dictated capture, Finnish↔English pairs, a Finnish pair where inflection leaves no word in common, a tagged pair, a control case that shares words, and four captures with nothing related.
  - `parc_core::recall` is the first retriever. It ranks by shared words (full-text index, BM25), shared tags and links, merges the rankings by reciprocal rank, and needs a link, a shared tag or two shared words before it returns anything.
  - Baseline: **1 of 11** sources found (only the control) and **3 of 4** correct abstentions. The client pitch matches a football pitch. Most sources the baseline misses share no words with their capture; the scorecard says which words each one shares.
- **Recall, step 2: one call for the evidence** (2026-11-24):
  - Recall works from a saved fragment or from **draft text** (title and body, its `#tags` and `[[links]]`), optionally naming the fragment being edited, which is then left out. On every benchmark case, a draft gives the same results as the saved fragment.
  - `recall::context` returns each source's type, title, dates, status, archived flag, revision, reasons, and up to two verbatim body lines with line, column and the shared words marked. Default 5 sources, at most 20, with `truncated`.
  - Three ways to call it: the `recall.context` server call, the read-only `recall` MCP tool, and `parc recall [ID] [--text …|-] [--tag …] [--json]`.
  - Speed, measured on 10,000 synthetic notes with a fresh `parc-server` per call (as the desktop calls it now), including process start and index sync: about 17 ms for a short draft and 25 ms for 70 words in a release build, 54 and 76 ms in a debug build. `fragment.search` takes 13 ms on the same vault.
- **Recall, step 4: the Related panel** in the desktop app (2026-11-24):
  - **Placement:** beside the text when the window is wide enough, under it when it isn't. It is there whenever the editor is, so the text never moves when results come and go.
  - **When it asks:** after each pause in typing (150 ms, and at least every 600 ms while typing goes on), as soon as a dictation lands, and when the vault changes on disk.
  - **Calm list:** answers that a newer one overtook are dropped. Sources still related keep their place; newcomers join below with a short fade. It never takes focus.
  - **Each source:** type, title and date, why (*Shares* words, *Tagged*, *Linked*), and the line with the most shared words, marked. A click opens the source in place without leaving the draft, with **Link** (inserts its `[[id]]` at the caret) and **Open**.
  - **Which vaults:** the open fragment's vault, or every vault in view for a new capture.
  - **Code:** `parc-desktop/related.js`; the backend's `recall` validates what it sends to `recall.context`.
  - **Tests:** a backend test, and smoke-test checks for both empty states around a dictation, reasons and marks, places kept while typing, focus left in the editor, reading in place and Link.
- **Tabs and split view** in the desktop app (2026-11-24):
  - Fragments and views open in tabs, in up to four groups side by side, each with its own strip. A click opens the group's **preview** tab (italic), which the next click replaces; editing, a double-click or moving it keeps it. A fragment is open in one tab at most: opening it again shows that tab.
  - **Beside:** Ctrl+click, or Ctrl+Enter in the list and Go to…, opens in the next group. Tabs drag along a strip, onto another group, or onto a page's left or right third to split; fragments drag in from the list. Keys as in a code editor: Ctrl+W, Ctrl+Tab, Ctrl+1–4, Ctrl+Alt+←/→, Ctrl+Shift+T, and a tab context menu. Ctrl+W no longer reaches the default menu's Close Window.
  - The layout and every tab's unsaved text are kept locally and come back at launch. This replaces the single remembered draft: `parc.draft` moves into `parc.workspace` once.
  - **Code:** `parc-desktop/tabs.js` (groups, strips, drag and drop, keys, context menu). In `renderer.js` each fragment's state (`opened`, `dirty`, its editor and Related panel) lives on its tab instead of in globals.
  - **Tests:** smoke-test checks for previews, keeping, beside, moving by key and by drag, dropping from the list, closing (with unsaved text) and reopening, groups by key, and the layout and a draft in a split after a reload.

- **Agent view: Sign in with ChatGPT** in the desktop app (2026-11-24):
  - **Sign-in:** OpenAI's Sign in with ChatGPT for open-source apps. OAuth with PKCE through the browser and a loopback callback; the first sign-in registers parc (`client_id=dynamic_agent_client`, `agent_name_hint=parc`) and keeps the issued client ID for later ones. A sign-in without `chatgpt.tokens.use.direct` is refused. The credential is sealed with the system keyring (async `safeStorage`; on desktops Chromium doesn't recognise, such as Hyprland, parc asks for libsecret) and refreshed one at a time.
  - **Loop:** the Responses API with `store: false`, the whole chat sent back each time (reasoning items encrypted), at most 10 rounds of tool calls. The plan's error codes become plain messages; a failed or stopped question is taken back out of the chat.
  - **Tools:** one `parc-server --mcp --read-only` child per vault, so the agent can only search, recall, read and list. With several vaults each tool takes a `vault` naming one of them.
  - **View:** a chat per Agent tab: each search and read above the answer (select it for what it returned), `[[ID]]` citations shown as titles that open the fragment, the fragment used last shared on request, the plan's models in a menu.
  - **Code:** `parc-desktop/agent.cjs` (main process: sign-in, loop, MCP), `parc-desktop/agent.js` (the view).
  - **Tests:** `test/agent.test.cjs` against a fake OpenAI and a real read-only parc-server (sign-in, sealing, refresh, the requests the plan accepts, the tool allowlist, errors, stop); the smoke test signs in, asks and opens a citation.
  - **Saved chats:** one JSON file per chat in the app's user data (`chats/`), never in a vault: a chat holds model state and fragment excerpts, and a vault travels over git. Saved after each answered question and named for the first one; a failed or stopped question isn't. A chat keeps the vaults it started in. An empty chat lists recent ones; picking one shows its tab, or loads it in place; Ctrl+click opens it beside. Tabs remember their chat across restarts; Go to… lists chats; deleting can be undone until the app quits.
  - **GitHub Copilot** as a second provider (2026-11-25): GitHub's device sign-in with parc's own OAuth app (`read:user`), then GitHub's Copilot SDK (`@microsoft/copilot-sdk`, ~200 MB of native runtime) in `empty` mode, as a child process with its data in parc's user data: `tools.availableTools: ['custom:*']`, parc's read-only tools as custom tools named `parc_*`, permission requests refused, custom instructions and telemetry off. A chat keeps its provider; Copilot chats save what was shown (`turns`) and a new session after a restart is told the earlier questions and answers. Code: `parc-desktop/copilot.cjs`. Chosen over Pi's way (VS Code's Copilot client ID and headers), which impersonates VS Code.
- **Desktop architecture & latency (Step 1)** (2026-11-25):
  - **Persistent `parc-server` client per vault**: `backend.cjs` keeps a long-running newline-delimited stdio `parc-server` child per vault with pipelined requests, idle shutdown, transparent crash recovery, and automatic newest-mtime selection between `target/release` and `target/debug`.
  - **Non-blocking concurrency**: dropped the global `action()` editor lock; replaced with independent `speechPhase`, per-tab `saving`, `navSeq`/`scopeSeq` stale-navigation cancellation, and per-tab `metaQueue` serialization so typing and switching tabs never freeze.
  - **Scale & smooth UI**: lazy-loaded History versions (fetches one version on selection instead of up to 60 up front), virtualized sidebar (`sidebar.js`), SHA-256 self-write watcher suppression plus `schemas/` and `config.yml` watching, off-main-thread Barnes-Hut graph layout (`graph-layout.mjs`, `graph-worker.js`), atomic disk-backed drafts in `userData/drafts/` (`drafts.cjs`), incremental Agent streaming updates, and dictation insert-at-caret.
- **Core feature & schema parity (Step 2)** (2026-11-25):
  - **`string` & `list` schema field editors**: inline editors for Text and List fields (`risk.mitigation`, `todo.assignee`, `decision.deciders`, and custom fields from Vault settings).
  - **Attachments**: file picker, drag-and-drop onto fragments, clipboard image paste, compact `#docAttachments` strip (Open / Insert / Remove / Attach…), inline `![[attach:filename]]` raster image previews & chips in CodeMirror served via a restricted `parc-attach://` protocol (raster only, never SVG, opaque per-session vault tokens, `realpath` containment), and immediate SQLite index + revision updates in `parc-server`.
  - **Promote / Convert Type & Templates**: `fragment.promote` JSON-RPC + saved-fragment type switcher dropdown + `Go to…` conversion commands; untouched new captures pre-fill `<vault>/templates/<type>.md` without marking dirty or prompting on close.
  - **Trash, Vault Health, Vault Management & Actionable Pulse** (`views.js`): Trash view (`trash.list`, `trash.purge`, Restore & Empty), Vault health view (`vault.doctor` clickable findings + `vault.reindex`), **New vault…** and **Remove vault…** (files on disk untouched), and daily-review Pulse sections (**Needs attention**, **Stale open work**, **From the archive**).

## Next

### 1. Recall

The feature that sets parc apart, and the product test in [ai-second-brain.md](ai-second-brain.md): dictate a thought, then find a related note from two years ago that used different words.

Each step can be reviewed on its own:

1. ~~Benchmark~~ (done, above).
2. ~~`recall.context`~~ (done, above).
3. A local embedding model, downloaded in the app like the whisper models. It has to be **multilingual**: the benchmark has Finnish↔English cases, because dictation is in both. It joins `recall` as one more ranking, and gets its own scorecard next to the lexical one.
4. ~~The Related panel~~ (done, above). Still to do:
   - the quick capture window (`Super+N`) has no panel yet (covered under step 3 below)
   - ~~a long-running `parc-server` child~~ (done in Step 1 above)

Cheap lexical improvements to measure with the benchmark before step 3: prefix matching for long words (Finnish inflection) and ranking tag matches by something other than newest first (the tagged case misses at rank 4).

### 2. ~~Agent~~ (done)

Core Agent view (Sign in with ChatGPT, GitHub Copilot provider, read-only MCP vault tools, saved chats) is done (see `## Done` above). Follow-up workflow enhancements (saving answers to the vault, context pruning, additional providers) are tracked under **Step 4** below.

### 3. Writing, Navigation & Quick Capture Craft

Where daily writing happens — turning `parc-desktop` into a fast, low-friction writing environment:

- **Elevate the Markdown editor (`editor.js`)**:
  - In-document Find & Replace (without losing fast vault-wide Search).
  - Clickable external links (`http:`/`https:`/`mailto:` validated via a narrow `open-external` IPC).
  - `#tag` autocomplete in the body and `[[Create new…]]` when typing a wiki-link that doesn't exist yet.
  - Formatting shortcuts & ergonomics: `Ctrl+B` bold, `Ctrl+I` italic, ``Ctrl+` `` code, `Ctrl+L` toggle checkbox, `Tab`/`Shift+Tab` list indent, GFM table styling, and a toggleable heading **Outline** for long notes.
- **Backlinks with context**: split outgoing links vs. incoming **Backlinks** under the editor, showing the quoting line/snippet from each fragment that links here.
- **First-class Quick Capture (`capture.html`, `capture.js`)**: add **Dictate**, `#tag` / `[[link]]` helpers, quick tag/due/priority controls, and a compact **Related** strip so `Super+N` → capture/speak → see older context → save works in one stroke.
- **Saved Searches & Bulk Actions**: pin frequent DSL queries in the sidebar as smart views (e.g. `due:this-week`, `type:todo priority:high`), and multi-select fragments to batch-tag, batch-archive, change status, or trash.

### 4. Agent & Dictation Loop

- **Close the Agent → Vault loop (`agent.js`, `agent.cjs`)**:
  - **Save as fragment** (with verified `[[ID]]` provenance through `ops::Writer`, step 5 of [ai-second-brain.md](ai-second-brain.md)) and **Copy Markdown** on every answer.
  - Tool-output context pruning for long chats, multi-fragment `@` pinning in the composer, and a **"Synthesize connection"** button on the Related panel.
  - Optional extra providers (API key for Anthropic / OpenAI-compatible endpoints, local model server) and measuring the agent on the recall benchmark.
- **Dictation UX (`speech.js`, `backend.cjs`)**:
  - Live audio level meter (`AnalyserNode`) while recording and a microphone input picker in Settings.
  - Chunking / warm `whisper-server` to lift the 2-minute cap and model cold-load delay.

### 5. Production Desktop Polish & Packaging

- **Desktop ergonomics**: persist window bounds and maximized state across restarts, zoom controls (`Ctrl+`/`-`/`0`), **Export / Print** (resolving `[[ULID]]`s to titles via `parc-core`), and an opt-in autosave toggle alongside explicit revisions.
- **Maintainability**: split `renderer.js` into focused modules and add `tsc --noEmit --allowJs --checkJs` static checking.
- **Packaging & OS integration**:
  - Desktop installer shipping `parc-capture.desktop` (so the Wayland portal knows the app and global shortcuts work natively) and bundled release `parc` + `parc-server` binaries.
  - Bundle `whisper-cli` in the packaged app (dictation step 4; waits on the installer).

## Later

- **Sync and phone capture**, together: a `parc sync` command and a way to handle conflicting copies.
- **Interface roles**: define what each interface (CLI, TUI, desktop, MCP) is for.
- **Polish**:
  - `CLAUDE.md` points to `planning/parc-prd.md`, which doesn't exist.
  - Performance targets.
