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

  async function save(id, draft) {
    const target = file(id), payload = JSON.stringify(cleanDraft(draft));
    await fs.mkdir(dir, { recursive: true, mode: 0o700 });
    await writeAtomic(target, payload);
  }

  function saveSync(id, draft) {
    const target = file(id), payload = JSON.stringify(cleanDraft(draft));
    fss.mkdirSync(dir, { recursive: true, mode: 0o700 });
    writeAtomicSync(target, payload);
  }

  async function load(id) {
    try {
      const raw = JSON.parse(await fs.readFile(file(id), 'utf8'));
      if (!raw || typeof raw !== 'object' || typeof raw.body !== 'string') return null;
      return cleanDraft(raw);
    } catch { return null; }
  }

  async function remove(id) {
    await fs.rm(file(id), { force: true }).catch(() => {});
  }

  function removeSync(id) {
    try { fss.rmSync(file(id), { force: true }); } catch { /* ignore */ }
  }

  // Drop stale draft files (and interrupted .tmp files) not referenced by any open tab.
  async function prune(keepIds = []) {
    const keep = new Set(Array.isArray(keepIds) ? keepIds.filter(id => typeof id === 'string' && DRAFT_ID.test(id)) : []);
    let names = [];
    try { names = await fs.readdir(dir); } catch { return; }
    for (const name of names) {
      if (name.endsWith('.tmp')) {
        if (await isStaleTemp(path.join(dir, name))) await fs.rm(path.join(dir, name), { force: true }).catch(() => {});
        continue;
      }
      if (!name.endsWith('.json')) continue;
      const id = name.slice(0, -5);
      if (!keep.has(id)) await fs.rm(path.join(dir, name), { force: true }).catch(() => {});
    }
  }

  return { save, saveSync, load, remove, removeSync, prune };
}

module.exports = { draftStore, DRAFT_ID };
