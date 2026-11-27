# Inbox: implementation plan

How to build roadmap §6 ("Inbox: files you throw in"). The roadmap says what the feature is; this file says how, in order. Written 2026-11-26.

## Decisions

| What | Choice | Fragment |
|---|---|---|
| Text extraction | xberg, pinned, configured in code | `01M8CP0YMG` (accepted) |
| Where extraction runs | a child process of the running parc binary | `01M8CGXNPD` (risk) |
| Inbox folder | fixed at `<vault>/inbox/`; a file moves out once parc has picked it up | `01M8B5STXP` (accepted) |
| Formats | PDF, docx, pptx, HTML, plain text, Markdown | `01M8CP0YMG` |
| Rejected alternative | pdf-extract + htmd + a docx reader written for parc | `01M8CGXNP1` (superseded) |

### What was measured

Run on two public EU reports (73 and 328 pages), a small docx, a two-slide pptx and an HTML page, with xberg 1.3.10 (`default-features = false`, features `pdf`, `office`, `html`).

- **PDF**: good page-by-page text, paragraphs rejoined, headings marked. 0.5 s for 73 pages, 2.6 s for 328. The reports' PDFs have no Title metadata, so `title` was empty (correct).
- **docx**: headings, nested lists, tables, links and the document title, as Markdown; pages only at explicit page breaks.
- **pptx**: one page per slide, the slide title as a heading, bullets kept. The test deck's speaker note did not appear in the slide text. xberg's code reads notes, but where they end up is unconfirmed: check with a real deck.
- **HTML**: good Markdown, but a broken `## title:` header at the top. `extract_metadata: false` removes it, and the title with it.
- **Safety**: no network crates and no external programs in that feature set. No files written once `use_cache` is off (it defaults to on). It reads a config file only through `ExtractionConfig::discover`, and environment variables only through `apply_env_overrides`; parc calls neither.
- **Timeout**: `extraction_timeout_secs` is ignored without the `tokio-runtime` feature, and even with it, a `tokio::time::timeout` cannot stop a parser stuck in a loop. Hence the child process.
- **Cost**: 212 crates parc-core doesn't already have (579 for 1.5.4); a 32 MB stripped test binary. Size was judged not to matter.
- **Churn**: 86 releases since June 2026; 1.4 raised the minimum Rust to 1.99.

## Before step 1

- **Update Rust on this machine**: it has 1.92 and xberg ≥ 1.4 needs 1.99 (`rustup update stable`). CI already uses the latest stable.

## Step 0: groundwork

Two gaps in today's code that the inbox runs into. Separate commits, each useful on its own.

- **Attach and detach through `ops::Writer`** (todo `01M8CGXNPN`).
  - Today `attachment::attach_file` writes the frontmatter itself (`parc-core/src/attachment.rs:105`), so attaching skips hooks and history, and `parc-cli/src/commands/attach.rs` and `parc-server/src/methods/attachment.rs` reindex by hand.
  - Add `Writer::attach(id, source, Copy | Move)`, `Writer::detach(id, filename)` and `Writer::move_attachment(from, to, filename)` (for **Attach to…**). File handling stays in `attachment.rs`; the frontmatter change goes through the Writer's update path. The CLI and server call these and drop their own reindex.
- **Emptying the trash, in core** (todo `01M8B5STXZK4`).
  - Today it lives in `parc-server/src/methods/trash.rs:79` and removes only `trash/<id>.md`.
  - Add `Writer::purge(id)` and `Writer::empty_trash()`, which also remove `attachments/<id>/` (with its `.text/` sidecars, once step 1 adds them) and `history/<id>/`. The server and the CLI's `parc trash` call them.

## Step 1: attachment text

For every attachment in the vault, not only the inbox. Useful on its own: search finds words inside your PDFs.

### Extraction: `parc-core/src/extract/`

- **Dependencies**: `xberg = { version = "=1.5.x", default-features = false, features = ["pdf", "office", "html"] }` with an exact pin, so upgrades are deliberate, and `pollster` to drive xberg's async `extract` without tokio.
- **One call, built in code**:
  - `use_cache: false`, OCR off (`disable_ocr`), `pages.extract_pages: true`.
  - `security_limits` set: content size, zip entries, total expansion and compression ratio.
  - HTML with `html_options.extract_metadata: false`; the title then comes from the first heading.
  - Markdown output for docx, pptx and HTML; plain text for PDF.
  - Never `ExtractionConfig::discover()` or `apply_env_overrides()`.
- **Formats** are detected from content (xberg's default policy) and checked against parc's list: PDF, docx, pptx, HTML, plain text, Markdown. Anything else is "no text", not an error.
- **Parts** (what "page" means for each format):
  - PDF: a page.
  - pptx: a slide.
  - docx, HTML and Markdown: a section between headings. xberg splits docx only at explicit page breaks, so parc splits the Markdown itself.
  - Plain text: chunks of a few thousand characters, at paragraph breaks.
- **Result**: `Extracted { title: Option<String>, unit: Page | Slide | Section, format: Text | Markdown, parts: Vec<String> }`.
- **Child process** (`01M8CGXNPD`):
  - `extract::isolated(path, limits)` runs `std::env::current_exe()` with a hidden argument. Both `parc` and `parc-server` check for it first thing in `main` and call `extract::child_main()`. The program is parc itself, so a vault cannot choose another.
  - The child sets `RLIMIT_AS` and `RLIMIT_CPU`, reads the file's bytes on stdin (it gets no path), and writes the result as JSON on stdout.
  - The parent kills it after a wall-clock limit and turns a kill, crash or bad exit into an error such as "too large", "took too long" or "unreadable".
  - One extraction at a time per process, queued, so a folder of 50 PDFs doesn't start 50 children.
  - Tests call `extract::in_process` directly; one integration test runs the child through the `parc` binary.
- **Failures are remembered** (see the index below), so a broken file isn't retried on every fill. It is retried when the file or parc's text format changes.

### Storing it: sidecars or index only

- **Setting**: `attachments: { text: sidecar | index }` in `config.yml`, default `sidecar`. Save with a `config::save_attachments` that rewrites only that block, like `save_types`.
- **Sidecar path**: `attachments/<id>/.text/<filename>.txt` for PDF and plain text, `.md` for docx, pptx, HTML and Markdown. Parts are separated by form feeds.
- **Header**: a first line recording the file's sha256, parc's `TEXT_FORMAT` number and the unit. The viewer strips it. The exact syntax is to settle when writing it.
- **When a sidecar is redone**: when the file's hash differs, or when its `TEXT_FORMAT` is lower than this parc's. Never when it is higher, so two computers on different parc versions don't keep rewriting each other's sidecars. `TEXT_FORMAT` is parc's own number, bumped by hand when output changes; it is never xberg's version.
- **Who writes sidecars**: `Writer::attach`, `parc reindex`, and the fill step below, never `index::open_index`.
- **Housekeeping**: written with `secure_fs`. `.text/` is left out of `list_attachments`, `attachment_count` and attachment references. Detach removes the file's sidecar.

### Index

- **In `SCHEMA_SQL`** (which changes the stamp, so every index rebuilds once):
  - `attachment_files(fragment_id, filename, sha256, size, mtime_ns, unit, parts, error)`, keyed by `(fragment_id, filename)`.
  - `attachment_fts USING fts5(fragment_id UNINDEXED, filename UNINDEXED, part UNINDEXED, text)`.
- **Outside `SCHEMA_SQL`**: `attachment_text(sha256, text_format, unit, parts_json)`, the extracted text for the `index` setting, keyed by file hash. It is kept across rebuilds, like the meaning vectors.
- **Sync on open**: when an attachment's mtime or size changed, index it from a fresh sidecar or cached text if there is one, else mark it pending. It never extracts.
- **`extract::fill(vault, conn)`**, like `embed::fill`, extracts pending files, writes the sidecar or cache, and indexes them. Called by:
  - `Writer::attach`, for that one file;
  - `parc reindex`;
  - the server, in the background after opening a vault and whenever a sync leaves files pending (a `git pull` brought new PDFs).
- **Bump `INDEX_VERSION`** only if stored data changes without the schema text changing.

### Search, recall, CLI, server

- **Search**:
  - A filter for attachment text: `in:files`, the name to settle. Plain words keep meaning fragment text, so today's results don't change.
  - Results carry `file_hits: [{ filename, part, snippet }]`. The CLI prints `p. 12` (or `slide 3`, or `§ Heading`) under a result, and `--json` includes the hits.
- **Recall**: attachment parts become one more full-text signal, so a source is found by its contents, and its excerpt names the page. Meaning vectors for parts come later, measured first: a 328-page PDF is 328 more texts to embed.
- **CLI**: `parc attachments <id> --text <filename> [--pages 3-5]` prints extracted text. `parc attachments` shows each file's text status (parts, or the error).
- **Server**: `attachment.text { id, filename, from, to }`. `attachment.list` gains each file's text status.
- **TUI**: an attachment opens its extracted text in the pager.

### Tests

- **Fixtures** in `parc-core/tests/fixtures/attachments/`: a small PDF, docx, pptx and HTML page made for the tests, not downloaded. Their expected text is checked in, so an xberg upgrade that changes output fails a test and gets a deliberate `TEXT_FORMAT` bump.
- **Sidecars**: a stale one is redone, a newer one is left alone, and the `index` setting writes none.
- **Limits**: a zip bomb is refused, and an oversized file is refused before extraction.
- **Child process**: a crash or timeout in the child becomes an error and leaves the server running.

## Step 2: the `source` type, the folder, and the Inbox view

- **`builtin/schemas/source.yml`**:
  - status `inbox` (open), `reading` (active), `processed` (done); default `inbox`.
  - Added to `vault.rs`'s built-in list so `parc init` copies it. Older vaults get it on first use of the inbox.
- **The folder**:
  - `parc init` creates `<vault>/inbox/` and adds `<vault>/inbox/` to the repository's `.gitignore`, as it already does for `index.db`.
  - Older vaults get both on first use.
  - The location is fixed, never read from `config.yml`.
- **`parc-core/src/inbox.rs`: `sweep(&mut Writer) -> SweepReport { created, waiting, skipped, failed, pending, completed, more }`**:
  - Takes regular files only (checked with `symlink_metadata`). Leaves alone dotfiles, `.part`, `.crdownload`, symlinks and folders.
  - A file has settled when it was last modified more than a second ago, checked again just before the move. Until then it is reported as `waiting`.
  - Per file: `Writer::create` a `source` (title from the filename), then commit the original attachment through the Writer. A failed transfer may trash an empty wrapper; no automatic cleanup purges original bytes.
  - CLI extraction runs after the import commits. The server imports up to 16 files per call and queues extraction; the filename title is immediately usable. Subsequent sweeps reconcile document titles only if the imported revision is unchanged.
- **CLI**: `parc inbox` sweeps, then lists open sources (`--json` too).
- **Server**: `inbox.sweep` reports per-file failures separately from skipped entries. Continue while `more` is true; poll while `pending` or `waiting` is nonzero/nonempty. `inbox.attachments` returns one metadata snapshot per vault.
- **Desktop**:
  - The main process watches `<vault>/inbox/` with `fs.watch` while the vault is open, debounced, and calls `inbox.sweep`. It also sweeps when the app starts.
  - Settings shows the folder's path with **Copy** and **Open in file manager**.
  - **Inbox view**, a type view like the boards, with a sidebar entry and a count. Rows show title, kind, size, "p. 34 of 120" and age.
  - **Keys**:
    - Enter opens the source.
    - `d` discards it (`Writer::delete`, Undo in a toast, a dialog only if it has notes).
    - `a` is **Attach to…**: `Writer::move_attachment` into the chosen fragment, its sidecar moving too, then the source is removed.
    - `l` is **Link to…**.
    - Status keys mark it reading or processed.
  - The last page read is kept per computer (the app's user data, keyed by vault and fragment), never in the vault.
- **Tests**: the skip rules, the settled rule, undoing a failed move, and discard, restore and purge with the file.

## Step 3: viewer and comments

- **PDF**:
  - `pdfjs-dist` (Apache-2.0) with its viewer parts: `PDFViewer`, `EventBus`, `PDFLinkService` and `PDFFindController` for lazy pages, the text layer and find.
  - The bytes come over IPC (`attachment.bytes`, with a size cap); `parc-attach:` stays images only.
  - Run with `isEvalSupported: false` and no PDF scripting. Links go through the existing open-external path.
  - `pageColors` recolours pages to the theme, with a toggle back to paper.
  - **Check first**: recent pdf.js decodes JPEG 2000 images and colour profiles with WebAssembly, which the CSP (`script-src 'self'`) blocks without `'wasm-unsafe-eval'`. Find out whether pdf.js falls back to plain JavaScript, or whether the CSP must allow WebAssembly.
- **docx, pptx, HTML**: the extracted Markdown, through the app's Markdown renderer, with one block per slide for a pptx. Nothing in the file runs or loads from the network. **Open original** hands the file to the system viewer.
- **Layout**: a source opens in a tab as its document, with a rail for **Notes** (the source's body, in the usual editor) and **Related**.
- **Quotes**:
  - Syntax: `> p. 12 · "…"`, `> slide 3 · "…"` and `> § Heading · "…"`. When a source has more than one document, the quote names the file: `> report.pdf p. 12 · "…"`.
  - The parser lives in core (`quote.rs`), so the CLI, TUI, recall and the Agent read them too.
  - Select text and press a key: the quote is added to Notes and you write the comment under it.
  - Quotes show as highlights on the page. A click on a highlight goes to the quote in Notes, and a click on a quote opens the page.
- **Finding a quote again**:
  - Core does it (`attachment.locate { id, filename, quote, part_hint } → { part, offset }`) with one normalisation: NFKC, ligatures, curly quotes to straight, dashes, collapsed whitespace and line-end hyphens.
  - pdf.js's text never matches xberg's exactly (xberg straightens quotes; pdf.js keeps them).
  - The renderer's copy of the normalisation is tested against the same fixture strings as core's.
- **Promote a comment** to a todo, idea or decision with the existing `Writer::promote`, linked back to the source.
- **Related follows the page**: recall seeded with the page on screen and your comments, refreshed on page change, debounced.

## Step 4: the Agent

- **`read_attachment { id, filename, from, to }`**: a read-only MCP tool, capped at a few pages and a character budget per call. Search hits already name the page, so answers cite `[[id]] p. 12`.
- **Ask about this document** from the viewer.
- **Analyze**: the Agent proposes links with reasons, tags, todos found in the text, and a summary. You tick what you want, and the app writes it through `Writer`. The Agent's tools stay read-only (risk `01M8B5STY9`).
- **A document goes to the model only when you ask**, never when it arrives.

## Later

- OCR for scanned PDFs (xberg's `ocr` feature; downloads models, so opt-in like the meaning model).
- "Already in your archive as [[X]]", by file hash.
- A warning for large files, with a note on git-lfs.
- More formats xberg already reads: ODT, EPUB, XLSX.
- Meaning vectors for attachment parts.

## Making test files

LibreOffice converts locally:

- **HTML to docx**: `soffice --headless --convert-to docx:"MS Word 2007 XML" file.html`. A `page-break-before: always` paragraph gives a docx page break.
- **Flat ODP to pptx**: `soffice --headless --convert-to pptx:"Impress MS PowerPoint 2007 XML" deck.fodp`.
