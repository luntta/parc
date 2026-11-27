// The main process's parts that need no Electron: attachment paths, reading positions, atomic writes, the vault
// watcher and the inbox sweeper.
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const { attachmentFile } = require('../attachment-file.cjs');
const { positionStore } = require('../positions.cjs');
const { writeJson, writeAtomic, isStaleTemp } = require('../files.cjs');
const { vaultWatcher } = require('../vault-watch.cjs');
const { inboxSweeper } = require('../inbox-report.cjs');

const ID = '01JQ7V3XKP5GQZ2N8R6T1WBMVH';
const tempDir = () => fs.mkdtemp(path.join(os.tmpdir(), 'parc-main-test-'));
async function until(check, ms = 3000) {
  for (const end = Date.now() + ms; Date.now() < end; await new Promise(r => setTimeout(r, 10))) if (await check()) return;
  throw new Error(`Timeout: ${check}`);
}

test('an attachment is found only as a regular file in real folders inside the vault', async () => {
  const dir = await tempDir();
  try {
    const vault = path.join(dir, '.parc'), outside = path.join(dir, 'outside');
    await fs.mkdir(path.join(vault, 'attachments', ID), { recursive: true });
    await fs.mkdir(outside);
    await fs.writeFile(path.join(vault, 'attachments', ID, 'memo.pdf'), 'pdf');
    await fs.writeFile(path.join(outside, 'id_test'), 'secret');
    assert.deepEqual(await attachmentFile(vault, ID, 'memo.pdf'), { file: path.join(vault, 'attachments', ID, 'memo.pdf'), size: 3 });
    for (const bad of ['../memo.pdf', '.text', '', '..', 'a/b']) assert.equal(await attachmentFile(vault, ID, bad), null, bad);
    assert.equal(await attachmentFile(vault, 'not-an-id', 'memo.pdf'), null);
    assert.equal(await attachmentFile('relative/.parc', ID, 'memo.pdf'), null);

    // A symlinked file, a symlinked fragment folder, and a symlinked attachments/ all lead nowhere.
    await fs.symlink(path.join(outside, 'id_test'), path.join(vault, 'attachments', ID, 'link.pdf'));
    assert.equal(await attachmentFile(vault, ID, 'link.pdf'), null);
    const other = '01JQ7V3XKP5GQZ2N8R6T1WBMVJ';
    await fs.symlink(outside, path.join(vault, 'attachments', other));
    assert.equal(await attachmentFile(vault, other, 'id_test'), null);
    await fs.rm(path.join(vault, 'attachments'), { recursive: true });
    await fs.mkdir(path.join(outside, ID));
    await fs.writeFile(path.join(outside, ID, 'id_test'), 'secret');
    await fs.symlink(outside, path.join(vault, 'attachments'));
    assert.equal(await attachmentFile(vault, ID, 'id_test'), null);
  } finally { await fs.rm(dir, { recursive: true, force: true }); }
});

test('atomic writes land in the order asked, never share a temporary file, and a failure stops only itself', async () => {
  const dir = await tempDir();
  try {
    const file = path.join(dir, 'settings.json');
    await Promise.all(Array.from({ length: 20 }, (_, i) => writeJson(file, { n: i })));
    assert.deepEqual(JSON.parse(await fs.readFile(file, 'utf8')), { n: 19 });
    assert.deepEqual((await fs.readdir(dir)).sort(), ['settings.json'], 'no temporary files left');
    const missing = path.join(dir, 'gone', 'x.json');
    await assert.rejects(writeAtomic(missing, 'a'));
    await fs.mkdir(path.join(dir, 'gone'));
    await writeAtomic(missing, 'b'); // the failed write doesn't hold up the file's next one
    assert.equal(await fs.readFile(missing, 'utf8'), 'b');
    const old = path.join(dir, 'x.json.1.ab.tmp');
    await fs.writeFile(old, '');
    assert.equal(await isStaleTemp(old), false, 'a fresh one may belong to a write in progress');
    await fs.utimes(old, new Date(Date.now() - 120_000), new Date(Date.now() - 120_000));
    assert.equal(await isStaleTemp(old), true);
    assert.equal(await isStaleTemp(file), false);
  } finally { await fs.rm(dir, { recursive: true, force: true }); }
});

test('reading positions are checked, kept per vault and fragment, and each save is written', async () => {
  const dir = await tempDir();
  try {
    const file = path.join(dir, 'positions.json');
    const store = positionStore(file);
    await store.set('/a/.parc', ID, { part: 3, total: 10, unit: 'slide', filename: 'deck.pptx' });
    await store.set('/b/.parc', ID, { part: 1, unit: 'nonsense', filename: '../x' });
    await assert.rejects(store.set('/a/.parc', 'bad', { part: 1 }), /target/);
    await assert.rejects(store.set('/a/.parc', ID, { part: 0 }), /page number/);
    const listed = await store.list(p => p === '/a/.parc');
    assert.deepEqual(Object.keys(listed), [JSON.stringify(['/a/.parc', ID])]);
    assert.equal(Object.values(listed)[0].unit, 'slide');
    const reread = await positionStore(file).list(() => true);
    assert.equal(reread[JSON.stringify(['/b/.parc', ID])].unit, 'page');
    assert.equal(reread[JSON.stringify(['/b/.parc', ID])].filename, null);
  } finally { await fs.rm(dir, { recursive: true, force: true }); }
});

test('the watcher reports outside edits and inbox arrivals, and stays quiet about the app’s own writes', async () => {
  const dir = await tempDir();
  const vault = path.join(dir, '.parc');
  for (const d of ['fragments', 'schemas', 'inbox']) await fs.mkdir(path.join(vault, d), { recursive: true });
  await fs.writeFile(path.join(vault, 'config.yml'), 'types: {}\n');
  const changes = [], arrivals = [];
  const watcher = vaultWatcher({ changed: (p, kind) => changes.push([p, kind]), inbox: p => arrivals.push(p) });
  try {
    watcher.watch(vault);
    watcher.watch(vault); // once is enough
    assert.equal(watcher.watching(vault), true);
    const own = path.join(vault, 'fragments', `${ID}.md`);
    await fs.writeFile(own, 'mine');
    await watcher.recordOwn(vault, `fragments/${ID}.md`);
    await new Promise(r => setTimeout(r, 400));
    assert.deepEqual(changes, [], 'own write suppressed');
    await fs.writeFile(own, 'theirs');
    await until(() => changes.length === 1);
    assert.deepEqual(changes[0], [vault, 'fragments']);
    await fs.writeFile(path.join(vault, 'config.yml'), 'types: { default: note }\n');
    await until(() => changes.some(([, kind]) => kind === 'schemas'));
    await fs.writeFile(path.join(vault, 'inbox', 'memo.txt.part'), 'partial');
    await new Promise(r => setTimeout(r, 500));
    assert.deepEqual(arrivals, [], 'partial downloads are not arrivals');
    await fs.writeFile(path.join(vault, 'inbox', 'memo.txt'), 'hi');
    await until(() => arrivals.length === 1);
    watcher.close(vault);
    assert.equal(watcher.watching(vault), false);
    const before = changes.length;
    await fs.writeFile(own, 'after close');
    await new Promise(r => setTimeout(r, 300));
    assert.equal(changes.length, before, 'nothing after close');
  } finally { watcher.closeAll(); await fs.rm(dir, { recursive: true, force: true }); }
});

test('the inbox sweeper runs one sweep per vault at a time, sweeps again when asked mid-sweep, and retries settling files', async () => {
  const events = [];
  let sweeps = 0, release;
  // 1: a document imported (and the sweep asked for again meanwhile); 2: a file still settling; 3: its text ready.
  const reports = [{ created: [{ id: 'a' }] }, { waiting: ['b'] }, { completed: 1 }];
  const sweeper = inboxSweeper({
    sweep: async () => { sweeps++; if (sweeps === 1) await new Promise(r => { release = r; }); return reports[sweeps - 1] || {}; },
    isOpen: p => p === 'v', tell: (p, e) => events.push(e.kind), retryMs: 20
  });
  try {
    const first = sweeper.run('v');
    assert.equal(sweeper.run('v'), first, 'a sweep in progress is shared');
    sweeper.background('v'); // asked again while sweeping
    await until(() => release);
    release();
    await first;
    await until(() => sweeps === 3 && events.length === 2);
    await new Promise(r => setTimeout(r, 60));
    assert.equal(sweeps, 3, 'nothing left to wait for: no more sweeps');
    assert.deepEqual(events, ['inbox', 'attachments']);
    await assert.rejects(sweeper.run('closed'), /opened vault/);
  } finally { sweeper.closeAll(); }
});

test('settings changes are checked, and leave what they don’t name as it was', () => {
  const { cleanSettings, DEFAULTS } = require('../settings.cjs');
  const next = cleanSettings(DEFAULTS, { theme: 'dark', speechLanguages: ['fi', 'en'], quickCaptureShortcut: ' CommandOrControl+Shift+Space ' });
  assert.equal(next.theme, 'dark');
  assert.deepEqual(next.speechLanguages, ['fi', 'en']);
  assert.equal(next.quickCaptureShortcut, 'CommandOrControl+Shift+Space');
  assert.equal(next.whisperModel, '');
  assert.equal(cleanSettings(next, { speechLanguage: 'auto' }).theme, 'dark');
  assert.deepEqual(DEFAULTS.speechLanguages, [], 'the defaults are not changed');
  for (const bad of [{ theme: 'blue' }, { whisperModel: 'relative/model.bin' }, { speechLanguages: ['xx'] }, { speechLanguages: ['fi', 'fi'] },
    { speechLanguage: 'klingon' }, { quickCaptureShortcut: 'Ctrl+$(rm -rf)' }]) {
    assert.throws(() => cleanSettings(DEFAULTS, bad), undefined, JSON.stringify(bad));
  }
});
