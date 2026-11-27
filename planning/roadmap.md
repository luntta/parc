# Roadmap: what's next

Updated 2026-11-26. From a ranked list of what would make parc a world-class app.

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
- **Recall, step 3: the meaning model** (2026-11-26):
  - **Model:** `sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2` (Apache-2.0, 480 MB), pinned to one revision with SHA-256s, run in plain Rust in `parc_core::embed`: the HF `tokenizers` crate, then BERT with `gemm` for the matrix products. No ONNX, no native library, no outside program. Its vectors match PyTorch's to six decimals. The weights file is memory-mapped and its 384 MB word table read in place, so vault servers share one copy.
  - **Chosen by measurement** over `multilingual-e5-small` and `granite-embedding-107m-multilingual`, each calibrated the same way. On the benchmark: 7 of 11 sources found against 3 (E5) and 5 (Granite). On 14 notes of this repo's vault paraphrased (4 in Finnish), the original came first 8 times and in the top 3 12 times (MRR 0.73; E5 0.67, Granite 0.68). From Finnish to English it was far ahead: E5 ranked those 9th, 22nd, 7th and 8th.
  - **Long notes** are read in overlapping 128-token windows, the length the model was trained on (at most 16 per note); two notes are as close as their closest windows.
  - **Evidence:** a similarity of 0.47 counts on its own (just above the 99th percentile of unrelated pairs in the benchmark). Below 0.2 the model reads two notes as unrelated, and shared words alone no longer count; that fixes "client pitch, not a football pitch". Both bars, and the spread they come from, are in `scorecard-meaning.txt`.
  - **Vectors** live in `index.db` (`meaning_*` tables, outside the index's own schema), keyed by file revision so an index rebuild keeps them. `embed::fill` computes missing ones, never the sync on open. Long-running servers (JSON-RPC and MCP) fill in the background at low priority, with a lease so two servers on one vault don't both do it. `parc recall` fills before answering.
  - **Where:** one folder per computer, `$PARC_MODELS_DIR` or `~/.local/share/parc/models`, never a path a vault names. **Settings → Recall** in the desktop app downloads it (resumable, hash-checked, only huggingface.co) and shows how much it has read. The Related panel says *Similar*. `parc recall`, `recall.context` and the MCP `recall` tool use it when it is there and say when it isn't. New RPC: `meaning.status`.
  - **Speed** with the model, in a long-running server: 8 ms for a short draft and 24 ms for 70 words in a 100-note vault; 36 and 50 ms with 10,000 notes (lexical alone: 15). Loading the model takes under 0.1 s; filling 10,000 notes took 26 s on 32 threads.
  - **Memory:** about 115 MB per server with the model loaded. parc has its own tokenizer for these models (`embed::Tokenizer`, checked token for token against the `tokenizers` crate by `tests/tokenizer.rs`), because the crate's took 280 MB, and a server runs per vault.
  - **Lexical, measured on the benchmark:** tag matches now tie (and weigh by rarity) instead of ranking newest first, and links tie, so other signals order them; ties go to the older note. Prefix matching for inflected words was tried and dropped: no case gained a source, and three gained chance matches (*leaves* ~ *leave*, *starts* ~ *starter*). The meaning model handles inflection.
  - **Scorecards:** lexical 2 of 11 found, 3 of 4 abstained; with meaning 7 of 11, 4 of 4, and 5 other fragments within the limits. Still missed: the ferry insight (similarity 0.29), Finnish→English potluck (0.32), and both early-run notes (0.39, 0.34).
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
3. ~~A local multilingual embedding model~~ (done, above). Follow-ups:
   - CI never runs the meaning scorecard (no model there); cache the 480 MB download in CI, or run it before each release.
   - Keep vectors in memory in a long-running server: at 10,000 notes, reading them from SQLite is most of the 20 ms meaning adds.
   - Batch several notes per forward pass when filling, so weights are read once per batch rather than once per note (filling is memory-bound).
   - The benchmark has 12 cases and decided both thresholds; add cases (more paraphrases, longer notes, Finnish↔English) before tuning further.
   - Deleting the model takes effect when parc restarts; the servers keep it loaded until then.
4. ~~The Related panel~~ (done, above). Still to do:
   - the quick capture window (`Super+N`) has no panel yet (covered under step 3 below)
   - ~~a long-running `parc-server` child~~ (done in Step 1 above)

Cheap lexical improvements were measured (see step 3 above): tied tag and link ranks kept, prefix matching dropped.

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

### 6. Inbox: files you throw in

Drop PDFs, Word and PowerPoint files and the like into a vault, read them in parc, comment on them, link or file them, and let the Agent use them as sources. Shape agreed 2026-11-26; how to build it, step by step, is in [`inbox-plan.md`](inbox-plan.md).

- **A file becomes a fragment when it lands.** A new built-in type, `source`: the file is its attachment, its body is your notes on it. The Inbox is the view of `source` fragments with an open status, as boards and the risk matrix are views of their types. Links, tags, backlinks, history, search, git and the Agent's tools work on it unchanged.
  - Statuses: `inbox` (open), `reading` (active), `processed` (done). No closed status: a file you don't want is discarded.
  - The title is the document's own title when its metadata has one, else the filename.
- **Discarding is one key** in the Inbox, with Undo and no dialog (a dialog only once you've written notes on it). It goes to the trash with its file, so Restore works.
  - Emptying the trash must then remove `attachments/<id>/` and `history/<id>/` too. Today `trash.purge` removes only the `.md`, so a discarded PDF would stay on disk.
  - A file committed to git before it was discarded stays in git's history. Nothing in parc commits, so a discard before your next commit leaves no trace.
- **One inbox per vault, with a folder**: `<vault>/inbox/`, git-ignored, made by `parc init` (and on first use in older vaults). Bookmark it in the file manager; GTK bookmarks also show in browsers' Save dialogs, so downloads can go straight in.
  - It is a mail slot. A file that has settled there (size and mtime unchanged for a second) moves into a new `source` fragment's attachments, so the folder empties itself. Partial downloads (`.part`, `.crdownload`), dotfiles, symlinks and folders are left alone.
  - The desktop app watches the folder while open and sweeps it at launch; `parc inbox` sweeps it from the CLI. The move stays on one filesystem, so it is a rename.
  - The location is fixed, never read from `config.yml`: a cloned vault that named `~/Documents` would have parc swallow it.
  - Settings shows the path with **Copy** and **Open in file manager**.
- **Triage**, per item: keep it as a source; **Attach to…** an existing fragment (the file moves there and the source goes); **Link to…**; discard.
- **Attachment text, for every attachment** in the vault, not just the inbox. Useful on its own, so it comes first:
  - Extracted in `parc-core`, so the CLI, server, MCP and desktop see the same text. PDF, docx, pptx, plain text, Markdown and HTML, with [xberg](https://github.com/xberg-io/xberg) (pure Rust, pinned, only its `pdf`, `office` and `html` features, configured in code and never from a file or environment variable) and limits on size, pages and zip expansion.
  - It runs in a child process of the running parc binary, with memory, CPU and time limits, so a hostile or broken file can't hang or crash `parc-server`. The program is parc itself, so a vault cannot choose one.
  - Indexed in its own full-text table, page by page (a slide for pptx, a section between headings for docx and HTML), so a match knows its page. A search filter for attachment text.
  - Kept as the vault's `attachments.text` setting says: `sidecar` (the default) writes it beside the file, as `attachments/<id>/.text/<filename>.txt` for a PDF (pages split by form feeds) or `.md` for docx, pptx and HTML (headings, lists and tables kept, which the viewer shows), so it travels over git and another machine needn't extract again; `index` keeps it only in `index.db`.
  - A sidecar records the file's hash and parc's own text-format version (never the extractor crate's, so a dependency bump doesn't rewrite every sidecar in git); a stale one is ignored and redone, a newer one is left alone. Sidecars are written when a file is attached, or by a reindex after switching to `sidecar`, never by the index sync on open. Attachment listings leave `.text/` out.
- **Reading and comments**:
  - A source opens in a tab as its document, with a rail beside it for **Notes** (the source's body, in the usual editor) and **Related**.
  - **PDF**: pdf.js (`pdfjs-dist`, Apache-2.0) with its viewer parts (lazy pages, text layer, find), not Chromium's built-in viewer, which gives no selection, current page or highlights to the page around it. The bytes come over IPC, so `parc-attach:` stays images only. In the sandboxed renderer, under a CSP without eval, with `isEvalSupported: false` and no PDF scripting; a link in a PDF goes through the narrow open-external path. `pageColors` can recolour text pages to the theme, with a toggle back to paper.
  - Your comments show as highlights on the page; a click on one goes to it in Notes, and a click on a quote in Notes goes to the page. The last page read is kept per computer, not in the vault, and the Inbox shows it ("p. 34 of 120").
  - **docx, pptx and HTML**: shown as the Markdown that core extracts (a pptx one block per slide), not as HTML, so nothing in the file runs or loads from the network, and quotes match the indexed text exactly. **Open original** hands the file to the system viewer, as **Open** does today.
  - The CLI and TUI read the extracted text.
  - Select text and press a key: the source's body gets a quote with its page (`> p. 12 · "…"`) and you write the comment under it. A click on the quote opens the page; the quoted words find the place again if the pages move.
  - A docx has no pages until a word processor lays it out, so its quotes name the nearest heading instead, and its "pages" for recall are sections between headings. A pptx quote names its slide (`> slide 3 · "…"`).
  - **Promote a comment** to its own todo, idea or decision, linked back to the source.
- **Related follows the page**: `recall` on the page on screen and on your comments, not the whole document, so the Related panel shows older notes about what you're reading. Local, no model.
- **Agent**:
  - A read-only `read_attachment` MCP tool that reads a range of pages, so a question sends only the pages it needs. With attachment text indexed, `search` and `recall` already find documents, and answers cite `[[id]] p. 12`.
  - **Ask about this document** from the viewer.
  - **Analyze**: the Agent proposes links (and why), tags, todos found in the text, and a summary; you tick what you want and the app writes it through `Writer`. Its tools stay read-only, because a document can carry text aimed at the model.
  - A document goes to the model only when you ask, never when it arrives.
- **Later**: OCR for scanned PDFs; "already in your archive as [[X]]" by file hash; a warning for large files, with a note on git-lfs; ODT, EPUB and XLSX, which xberg also reads.

Order: groundwork (attach and trash purge through `Writer`) → attachment text → the `source` type, folder and Inbox view → viewer and comments → Agent.

## Later

- **Sync and phone capture**, together: a `parc sync` command and a way to handle conflicting copies.
- **Interface roles**: define what each interface (CLI, TUI, desktop, MCP) is for.
- **Polish**:
  - `CLAUDE.md` points to `planning/parc-prd.md`, which doesn't exist.
  - Performance targets.
