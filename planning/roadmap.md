# Roadmap: what's next

Updated 2026-11-24. From a ranked list of what would make parc a world-class app.

## Done

- **Self-correcting index**: `index::open_index` syncs the index with `fragments/` on every open, so no reindex is needed after a pull or an outside edit (f000851).
- **MCP server**: `parc mcp` lets agents use a vault as memory (6ae20a4).
- **Dictation, steps 1–3** (8fa1ee4):
  - dictation languages, with a picker next to Dictate and `whisper-cli -l`
  - recordings converted to WAV in the app, so ffmpeg is no longer needed
  - in-app model downloads that resume and are checked against a pinned SHA-256
- **Capture from anywhere, the desktop half** (2026-11-24):
  - Hotkey to window, measured on Hyprland: about 140 ms with the app running, 230 ms from a cold start, 300 ms with the app's files dropped from the disk cache. The text box has focus by then. No background mode is needed, so there isn't one.
  - The app's own shortcut doesn't work on Hyprland: the desktop portal refuses it because there is no installed `parc-capture.desktop`, yet Electron reports success. Settings now says the desktop was only asked, and offers the launch command with **Copy**.
  - A Hyprland binding (Lua) that focuses an open capture window or opens one, and a window rule that floats, centres and pins it: in `parc-desktop/README.md`.
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

## Next

### 1. Recall

The feature that sets parc apart, and the product test in [ai-second-brain.md](ai-second-brain.md): dictate a thought, then find a related note from two years ago that used different words.

Each step can be reviewed on its own:

1. ~~Benchmark~~ (done, above).
2. ~~`recall.context`~~ (done, above).
3. A local embedding model, downloaded in the app like the whisper models. It has to be **multilingual**: the benchmark has Finnish↔English cases, because dictation is in both. It joins `recall` as one more ranking, and gets its own scorecard next to the lexical one.
4. ~~The Related panel~~ (done, above). Still to do:
   - the quick capture window (Super+N) has no panel yet
   - in a debug build each request starts a `parc-server` (about 50–75 ms on 10,000 notes); a long-running server child would cut that, and step 3 needs one anyway to keep its model loaded

Cheap lexical improvements to measure with the benchmark before step 3: prefix matching for long words (Finnish inflection) and ranking tag matches by something other than newest first (the tagged case misses at rank 4).

## Later

- **Sync and phone capture**, together: a `parc sync` command and a way to handle conflicting copies.
- **Packaging and interface roles**:
  - a desktop installer. Ship `parc-capture.desktop` with it: the portal then knows the app, and the app's own shortcut can work on Wayland
  - bundle `whisper-cli` in the packaged app (dictation step 4; waits on the installer)
  - define what each interface (CLI, TUI, desktop, MCP) is for
  - split up `renderer.js`
- **Polish**:
  - `CLAUDE.md` points to `planning/parc-prd.md`, which doesn't exist.
  - Performance targets.

## Loose ends

- Super+N for quick capture hasn't been pressed for real: Hyprland ignores keys typed by `wtype`, and the screen was locked while it was tested, so no window could take focus. Does the capture window take focus with a real key press?
- Real dictation hasn't been confirmed yet: does a recording transcribe correctly in both Finnish and English? `whisper-cli` is installed from Arch's `extra/whisper-cpp`; `ggml-vulkan` adds GPU acceleration.
