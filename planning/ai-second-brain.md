# Voice capture and long-term recall (proposed milestones)

Product test: dictate a thought, correct and save it, then discover a relevant observation from two years ago that used different words. Open both notes and judge the connection. An external client should be able to retrieve the same evidence and any accepted reflection without launching Electron.

## Boundaries

- Electron is the first-party capture UI and LLM harness. The renderer owns recording controls, transcript editing and evidence display. A sandboxed renderer uses narrow preload methods; the main process controls the stdio `parc-server` child, transcription adapter, provider credentials, model loop and its tool allowlist. Never expose raw IPC/RPC dispatch to model output.
- Rust owns vault operations, retrieval and shared result schemas. Existing `fragment.create` saves confirmed notes through `ops::Writer`. Saving never waits for recall. Model suggestions are not edits.
- Prefer synthetic data; no provider traffic with repository vault contents by default. Use a disposable copy for later opt-in local exploratory testing. Do not retain raw recordings by default; stop the mic on stop/cancel. Confirm local vs hosted policy separately for speech and text.

## Reviewable increments

1. **Recall fixture → reproducible benchmark.** Start with `parc-core/tests/fixtures/recall/ferry.json`. Load into a temporary vault with real IDs and fixed dates; reindex; keep labels out of retrieval and exclude future fragments. Add cases at 1 and 3 years, plus dated notes that disagree and a noisy transcript. Score old-source rank and abstention separately. Never target the working `.parc` in CI.
2. **Electron capture → real Markdown note.** Typed draft → save via JSON-RPC → read back using the CLI. Add push-to-talk → fake transcriber → editable draft using the same save path, then one real STT adapter once privacy and target platform are settled. Confirm denied microphone, offline model or cancellation does not block typed capture; recording alone must not create a note.
3. **Long-term evidence retrieval.** Implement bounded `recall.context` (proposed RPC) on top of a `parc-core` operation. Return seed and ranked source IDs, dates, excerpt locations, verbatim title/body excerpts and retrieval reasons. Search across the whole vault, not just a recent window; 1–3 years is a quality target, not an age filter. Compare lexical/tag/link baseline with semantic retrieval on paraphrase misses; do not hard-code fixture terms or expect FTS alone to recover analogies.
4. **One cited reflection.** Electron invokes one model adapter with read-only, bounded context. Show a proposed connection or no match. Validate quoted evidence against original fragments; compare content fingerprints before accepting. Fake provider in CI. Evaluate interpretation separately from retrieval; citations alone don't prove two events are related.
5. **Portable results.** On explicit acceptance, save a Markdown synthesis with source references and provenance via `Writer`, not Electron storage or a pair of symmetric links. Verify CLI and JSON-RPC clients can read it after `reindex` and with Electron closed. Provide a narrow read-only adapter (e.g. MCP for compatible harnesses) over the same server contracts; third parties can either consume the accepted result or run their own model on `recall.context`.

First demo needs no provider key: fixture, typed/fake-dictated save, evidence preview. Real speech and real long-range recall each need their own acceptance test. Defer graph visualization, background listening and autonomous writes.
