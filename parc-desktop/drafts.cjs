// Crash-safe unsaved draft persistence, on this computer only (never inside a vault).
// One JSON file per dirty tab under <userData>/drafts/, keeping localStorage small and fast.
const fs = require('node:fs/promises');
const fss = require('node:fs');
const path = require('node:path');
const { writeAtomic, writeAtomicSync, isStaleTemp } = require('./files.cjs');

const DRAFT_ID = /^[A-Za-z0-9_-]{1,64}$/;
const MAX_TEXT = 1_000_000;

function validId(id) {
  if (typeof id !== 'string' || !DRAFT_ID.test(id)) throw new Error('Invalid draft ID');
  return id;
}

function cleanDraft(draft) {
  if (!draft || typeof draft !== 'object' || Array.isArray(draft)) throw new Error('Invalid draft');
  const title = typeof draft.title === 'string' ? draft.title : '';
  const body = typeof draft.body === 'string' ? draft.body : '';
  if (title.length + body.length > MAX_TEXT) throw new Error('Draft is too large.');
  const base = typeof draft.base === 'string' && /^[0-9a-f]{16}$/.test(draft.base) ? draft.base : null;
  const at = typeof draft.at === 'string' && draft.at.length <= 64 ? draft.at : new Date().toISOString();
  return { title, body, base, at };
}

function draftStore(dir) {
  const file = id => path.join(dir, `${validId(id)}.json`);
  const latest = new Map();

  async function save(id, draft) {
    const target = file(id), payload = JSON.stringify(cleanDraft(draft));
    const token = Symbol();
    latest.set(id, token); // before the first await, including directory creation
    try {
      await fs.mkdir(dir, { recursive: true, mode: 0o700 });
      if (latest.get(id) === token) await writeAtomic(target, payload, { publish: () => latest.get(id) === token });
    } finally { if (latest.get(id) === token) latest.delete(id); }
  }

  function saveSync(id, draft) {
    const target = file(id), payload = JSON.stringify(cleanDraft(draft));
    latest.delete(id);
    fss.mkdirSync(dir, { recursive: true, mode: 0o700 });
    writeAtomicSync(target, payload);
  }

  async function load(id) {
    try {
      const raw = JSON.parse(await fs.readFile(file(id), 'utf8'));
      if (!raw || typeof raw !== 'object' || typeof raw.body !== 'string') throw new Error('Invalid draft file');
      return cleanDraft(raw);
    } catch (err) {
      if (err.code === 'ENOENT') return null;
      // A JSON parse error can quote private draft text in Electron's IPC logs.
      if (err instanceof SyntaxError) throw new Error('Draft file contains invalid JSON.');
      throw err;
    }
  }

  function removeSync(id) {
    const target = file(id);
    latest.delete(id);
    fss.rmSync(target, { force: true });
  }

  // The layout is a separate file and can be lost. Unreferenced drafts are recoverable
  // documents, not garbage. Only explicit save/discard may remove a completed draft.
  async function recoverable() {
    let names;
    try { names = await fs.readdir(dir); } catch (err) { if (err.code === 'ENOENT') return []; throw err; }
    const ids = [];
    for (const name of names) {
      if (name.endsWith('.tmp')) {
        if (await isStaleTemp(path.join(dir, name))) await fs.rm(path.join(dir, name), { force: true }).catch(() => {});
        continue;
      }
      if (!name.endsWith('.json')) continue;
      const id = name.slice(0, -5);
      if (DRAFT_ID.test(id)) ids.push(id);
    }
    return ids;
  }

  return { save, saveSync, load, remove: removeSync, removeSync, recoverable };
}

module.exports = { draftStore, DRAFT_ID };
