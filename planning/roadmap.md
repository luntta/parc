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

## Next

### 1. Recall

The feature that sets parc apart, and the product test in [ai-second-brain.md](ai-second-brain.md): dictate a thought, then find a related note from two years ago that used different words.

Each step can be reviewed on its own:

1. Benchmark first, on the existing fixture `parc-core/tests/fixtures/recall/ferry.json`.
2. A `recall.context` call on the server, on top of a `parc-core` operation.
3. A local embedding model, downloaded in the app like the whisper models.
4. A "related notes" panel in the desktop app.

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
