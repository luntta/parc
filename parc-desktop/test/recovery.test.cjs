const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const os = require('node:os');
const { draftStore } = require('../drafts.cjs');

test('only the latest requested draft may publish, including unload and deletion', async t => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'parc-recovery-'));
  t.after(() => fs.rm(dir, { recursive: true, force: true }));
  const store = draftStore(dir);
  const save = body => store.save('one', { body });
  const pending = [save('first'), save('second')];
  store.saveSync('one', { body: 'at unload' });
  await Promise.all(pending);
  assert.equal((await store.load('one')).body, 'at unload');
  const old = save('old'); await store.remove('one'); await old;
  assert.equal(await store.load('one'), null);
  await Promise.all([save('older'), save('last')]);
  assert.equal((await store.load('one')).body, 'last');
  await fs.writeFile(path.join(dir, 'corrupt.json'), 'oops');
  await assert.rejects(store.load('corrupt'));
  assert.deepEqual((await store.recoverable()).sort(), ['corrupt', 'one']);
});

test('restore and unload cannot publish an empty layout until startup succeeds', async () => {
  const { workspacePersistence } = await import('../workspace-persistence.mjs');
  let layout = 'good', snapshot = 'empty', error;
  const store = workspacePersistence({ snapshot: () => snapshot, saveLayout: s => { layout = s; },
    saveDrafts: sync => { if (sync) throw new Error('disk full'); }, report: err => { error = err; } });
  store.remember(true); store.remember(true, { syncDisk: true });
  assert.equal(layout, 'good');
  snapshot = 'restored'; store.activate(); assert.equal(layout, 'restored');
  snapshot = 'too early';
  assert.equal(store.remember(true, { syncDisk: true }), false);
  assert.equal(error.message, 'disk full'); assert.equal(layout, 'restored');
});

test('summary and editor writes use the text baseline across metadata changes; even overwrite compares revisions', async () => {
  const { textBase, saveDocumentText } = await import('../document-revision.mjs');
  const disk = { id: 'one', type: 'source', title: 'title', body: 'body', revision: 'v1' };
  const base = textBase(disk), writes = [];
  Object.assign(disk, { revision: 'attachment', body: 'outside edit' });
  let choice = 'cancel', dialogs = 0;
  const api = { note: () => ({ ...disk }), conflict: () => { dialogs++; return choice; },
    save: (_v, data) => {
      writes.push(data.expected);
      if (data.expected !== disk.revision) return { conflict: true };
      return Object.assign(disk, { revision: 'saved', body: data.body });
    } };
  const save = () => saveDocumentText(api, '/vault', base, 'title', 'body + summary');
  assert.deepEqual(await save(), { choice: 'cancel' }); assert.equal(disk.body, 'outside edit');
  choice = 'overwrite'; await save(); assert.deepEqual(writes, ['v1', 'v1', 'attachment']);
  // Attachment-only update: compare against the new revision without prompting.
  Object.assign(disk, { body: 'body', revision: 'metadata' }); await save();
  assert.equal(dialogs, 2); assert.equal(writes.at(-1), 'metadata');
  assert.deepEqual(textBase(disk, 'stale'), { id: 'one', revision: 'stale' });
});
