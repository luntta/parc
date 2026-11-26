const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const os = require('node:os');
const crypto = require('node:crypto');
const { archive, speechConfig, speechLanguage, checkWav, transcribe } = require('../backend.cjs');
const { draftStore } = require('../drafts.cjs');
const models = require('../models.cjs');
// The parc-server children never see a meaning model this computer may have downloaded.
process.env.PARC_MODELS_DIR = '/nonexistent/parc-test-models';
const wav = import('../wav.mjs');
const graphLayout = import('../graph-layout.mjs');

test('explicit save is readable from a separate RPC process in an isolated vault', async t => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'parc-desktop-test-'));
  t.after(() => fs.rm(dir, { recursive: true, force: true }));
  const vault = path.join(dir, '.parc'), store = archive(vault);
  await store.init(); await store.init();
  assert.deepEqual(await store.list(), []);
  await assert.rejects(store.save({ title: '', body: '   ' }));
  const note = await store.save({ title: 'Volunteer orientation', body: 'People asked me afterwards.' });
  const other = archive(vault);
  assert.equal((await other.get(note.id)).body.trim(), 'People asked me afterwards.');
  assert.equal((await other.list())[0].id, note.id);
  const updated = await other.save({ id: note.id, title: note.title, body: '## Questions\nWhat next?' });
  assert.equal(updated.id, note.id);
  assert.equal((await other.search('Questions'))[0].id, note.id);
  // Go to…'s / mode: lines of bodies containing the text, with line and column, ignoring case unless it has capitals.
  const found = await store.grep('what next');
  assert.equal(found.matches[0].id, note.id);
  assert.deepEqual(found.matches[0].lines.map(l => [l.line, l.column, l.text, l.ranges]), [[2, 1, 'What next?', [[0, 9]]]]);
  assert.equal((await store.grep('What Next')).matches.length, 0);
  await assert.rejects(store.grep('x'.repeat(1001)));
  // The sidebar sorts by creation as well as by last edit, so rows carry both.
  const [listed] = await store.list();
  assert.equal(listed.created_at, (await store.get(note.id)).created_at);
  assert.ok(listed.updated_at >= listed.created_at);
  assert.equal((await store.list()).length, 1);
  assert.match(await fs.readFile(path.join(vault, 'fragments', note.id + '.md'), 'utf8'), /Volunteer orientation/);
});

test('recall finds what a draft or a saved fragment relates to, and checks what it sends', async t => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'parc-recall-test-'));
  t.after(() => fs.rm(dir, { recursive: true, force: true }));
  const store = archive(path.join(dir, '.parc'));
  await store.init();
  const ferry = await store.save({ title: 'Ferry desk', body: 'The queue was watching.\nA route card to mark in private worked.' });
  const capture = await store.save({ title: 'Orientation', body: 'Try private route cards.' });
  await store.save({ title: 'Kitchen timer', body: 'Broken again.' });
  // A draft that isn't saved: the related fragment, why, and the line that says so.
  const draft = await store.recall({ text: 'Nobody asks in front of everyone. Private route cards?' });
  assert.deepEqual(draft.sources.map(s => s.title).sort(), ['Ferry desk', 'Orientation']);
  const source = draft.sources.find(s => s.id === ferry.id);
  assert.deepEqual(source.reasons.shared_words, ['private', 'route']);
  assert.deepEqual(source.excerpts.map(e => [e.line, e.text]), [[2, 'A route card to mark in private worked.']]);
  // A saved fragment, as a draft of itself: never in its own list.
  const saved = await store.recall({ id: capture.id, text: 'Orientation\nTry private route cards.', limit: 1 });
  assert.deepEqual(saved.sources.map(s => s.id), [ferry.id]);
  for (const bad of [{}, { text: 'x', limit: 0 }, { text: 'x', limit: 21 }, { id: 'nope', text: 'x' }, { text: 'x', tags: ['two words'] }, { text: 'x'.repeat(1000001) }]) {
    await assert.rejects(store.recall(bad), JSON.stringify(bad).slice(0, 60));
  }
});

test('workspace lists archived and non-note fragments and editing preserves their metadata', async t => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'parc-workspace-test-'));
  t.after(() => fs.rm(dir, { recursive: true, force: true }));
  const vault = path.join(dir, '.parc'), store = archive(vault);
  await store.init();
  // Seed through the ordinary CLI; the workspace must interoperate with its files and index.
  const { execFileSync } = require('node:child_process');
  const local = path.resolve(__dirname, '../../target/debug/parc');
  const cli = require('node:fs').existsSync(local) ? local : 'parc';
  const run = (...args) => execFileSync(cli, ['--vault', vault, ...args], { encoding: 'utf8' });
  run('new', 'todo', '--title', 'Buy supplies', '--body', 'Paper', '--status', 'open', '--tag', 'errand');
  const [todo] = await store.list();
  assert.equal(todo.type, 'todo');
  run('archive', todo.id);
  assert.equal((await store.list())[0].id, todo.id);
  await store.save({ id: todo.id, title: 'Buy paper', body: '- A4' });
  const edited = await store.get(todo.id);
  assert.equal(edited.type, 'todo'); assert.equal(edited.archived, true);
  assert.equal(edited.status, 'open'); assert.deepEqual(edited.tags, ['errand']);
});

test('typed capture, field edits, stale-write conflicts and vault links', async t => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'parc-fields-test-'));
  t.after(() => fs.rm(dir, { recursive: true, force: true }));
  const vault = path.join(dir, '.parc'), store = archive(vault);
  await store.init();

  const todo = schemaOf(await store.schemas(), 'todo');
  assert.deepEqual(todo.toggle, { field: 'status', off: 'open', on: 'done' });
  assert.deepEqual(todo.fields.find(f => f.name === 'priority').values, ['low', 'medium', 'high', 'critical']);

  // A new capture carries its type, schema fields and tags into the first save.
  const task = await store.save({ title: 'Ship it', body: 'Soon', type: 'todo', fields: { priority: 'high', due: '2030-01-02' }, tags: ['release'] });
  assert.equal(task.type, 'todo'); assert.equal(task.status, 'open'); assert.equal(task.priority, 'high');
  assert.equal(task.due, '2030-01-02'); assert.deepEqual(task.tags, ['release']);

  // Field edits name the revision they were based on; null clears an optional field.
  const done = await store.update(task.id, { fields: { status: 'done', due: null }, tags: ['release', 'q1'] }, task.revision);
  assert.equal(done.status, 'done'); assert.equal(done.due, undefined); assert.deepEqual(done.tags, ['release', 'q1']);
  assert.notEqual(done.revision, task.revision);

  // A copy older than the file is refused, for text and for fields, and nothing changes.
  assert.equal((await store.save({ id: task.id, title: 'Ship it', body: 'stale', expected: task.revision })).conflict, true);
  assert.equal((await store.update(task.id, { fields: { status: 'open' } }, task.revision)).conflict, true);
  assert.equal((await store.get(task.id)).status, 'done');
  // Saving from the revision the last save returned is not a conflict, even within the same second.
  const shipped = await store.save({ id: task.id, title: 'Ship it', body: 'Shipped', expected: done.revision });
  assert.equal(shipped.body, 'Shipped');
  assert.equal((await store.get(task.id)).revision, shipped.revision);
  await assert.rejects(store.save({ id: task.id, title: 'x', body: 'y', expected: '2001-01-01T00:00:00Z' }), /Invalid revision/);

  // Archive, trash and history round trips.
  assert.equal((await store.archive(task.id, true, shipped.revision)).archived, true);
  assert.equal((await store.list()).find(r => r.id === task.id).archived, true);
  const back = await store.archive(task.id, false);
  await store.remove(task.id);
  await assert.rejects(store.get(task.id));
  assert.equal((await store.untrash(task.id)).revision, back.revision);
  const versions = await store.history(task.id);
  assert.ok(versions.length >= 3);
  const oldest = versions.map(v => v.timestamp).sort()[0];
  assert.equal((await store.version(task.id, oldest)).body.trim(), 'Soon');
  await store.restoreVersion(task.id, oldest);
  assert.equal((await store.get(task.id)).body.trim(), 'Soon');
  await assert.rejects(store.version(task.id, '../../etc'), /Invalid version/);

  await assert.rejects(store.save({ title: 'x', body: '', type: '../todo' }), /Invalid fragment type/);
  await assert.rejects(store.update(task.id, { tags: ['two words'] }), /Invalid tags/);
  await assert.rejects(store.update(task.id, { fields: { status: 42 } }), /Invalid fields/);

  const note = await store.save({ title: 'See also', body: `Follows [[${task.id}]].` });
  assert.deepEqual(await store.links(), [{ source: note.id, target: task.id }]);
});
function schemaOf(list, name) { return list.find(s => s.name === name); }

test('vault settings: types turn off, statuses rename with their fragments, new types take captures', async t => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'parc-types-test-'));
  t.after(() => fs.rm(dir, { recursive: true, force: true }));
  const vault = path.join(dir, '.parc'), store = archive(vault);
  await store.init();

  // Every built-in type starts on, notes by default; statuses say what they mean.
  assert.deepEqual(await store.vaultSettings(), { types: { default: 'note', disabled: [] } });
  const todo = schemaOf(await store.schemas(), 'todo');
  assert.equal(todo.enabled, true); assert.equal(todo.builtin, true);
  assert.deepEqual(todo.fields[0].categories, { open: 'open', 'in-progress': 'active', done: 'done', cancelled: 'closed' });

  // A turned-off type keeps its fragments, which still change; new ones are refused.
  const risk = await store.save({ title: 'Disk fills up', body: '', type: 'risk' });
  await store.updateVaultSettings({ types: { disabled: ['risk'] } });
  assert.equal(schemaOf(await store.schemas(), 'risk').enabled, false);
  await assert.rejects(store.save({ title: 'Another', body: '', type: 'risk' }), /turned off/);
  assert.equal((await store.update(risk.id, { fields: { status: 'resolved' } })).status, 'resolved');
  await assert.rejects(store.updateVaultSettings({ types: { default: 'risk' } }), /can.t be turned off/);
  await assert.rejects(store.updateVaultSettings({ hooks: { enabled: true } }), /Invalid vault settings/);
  await store.updateVaultSettings({ types: { default: 'todo' } });
  assert.equal(schemaOf(await store.schemas(), 'todo').default, true);

  // Renaming a status moves the fragments holding it; removing one asks where they go.
  const doing = await store.get((await store.save({ title: 'Doing', body: '', type: 'todo', fields: { status: 'in-progress' } })).id);
  assert.deepEqual(await store.schemaUsage('todo'), { total: 1, values: { status: { 'in-progress': 1 }, priority: { medium: 1 } } });
  const edited = {
    ...todo, fields: todo.fields.map(f => f.name !== 'status' ? f : {
      ...f, values: ['open', 'doing', 'done'], categories: { open: 'open', doing: 'active', done: 'done' }
    })
  };
  await assert.rejects(store.saveSchema({ schema: edited, renames: [] }), /invalid/);
  const saved = await store.saveSchema({ schema: edited, renames: [{ field: 'status', from: 'in-progress', to: 'doing' }] });
  assert.equal(saved.migrated, 1);
  assert.deepEqual(saved.schema.fields[0].values, ['open', 'doing', 'done']);
  assert.equal((await store.get(doing.id)).status, 'doing');
  assert.equal((await store.get(doing.id)).updated_at, doing.updated_at);
  assert.equal((await store.builtinSchema('todo')).fields[0].values.length, 4);

  // A new type takes captures at once, and can be deleted while nothing uses it.
  const bug = { name: 'bug', alias: 'b', fields: [{ name: 'status', kind: 'enum', values: ['new', 'fixed'], required: true, default: 'new', toggle: ['new', 'fixed'], categories: { new: 'open', fixed: 'done' } }] };
  await store.saveSchema({ schema: bug, create: true });
  await assert.rejects(store.saveSchema({ schema: bug, create: true }), /exists/);
  assert.equal((await store.save({ title: 'Crash', body: '', type: 'bug' })).status, 'new');
  await assert.rejects(store.deleteSchema('bug'), /1 fragment is of type/);
  await store.saveSchema({ schema: { name: 'chore', fields: [] }, create: true });
  assert.deepEqual(await store.deleteSchema('chore'), { deleted: true });

  await assert.rejects(store.saveSchema({ schema: { name: 'x', fields: [{ name: 'a', kind: 'number' }] } }), /Invalid field/);
  await assert.rejects(store.saveSchema({ schema: { name: '../x', fields: [] } }), /Invalid fragment type/);
  await assert.rejects(store.saveSchema({ schema: bug, renames: [{ field: 'status', from: 'a b', to: 'c' }] }), /Invalid value changes/);
});

test('no local model leaves speech unavailable', async () => {
  await assert.rejects(speechConfig({}), /speech model/);
  await assert.rejects(speechConfig({}, { whisperModel: 'relative/model.bin' }), /speech model/);
});
for (const fail of [true, false]) {
  test(`temporary speech files are removed after ${fail ? 'failure' : 'success'}`, async () => {
    const { encodeWav } = await wav;
    let clip, args;
    const mock = async (file, given) => {
      args = given; clip = given[given.indexOf('-f') + 1];
      assert.equal(file, 'whisper-cli');
      if (fail) throw new Error('model failed');
      await fs.writeFile(given[given.indexOf('-of') + 1] + '.txt', ' synthetic transcript\n');
    };
    const audio = encodeWav(new Float32Array(1600).fill(0.25));
    const result = transcribe(audio, { model: 'local-model', command: 'whisper-cli', language: 'fi' }, mock);
    if (fail) await assert.rejects(result, /model failed/);
    else assert.equal(await result, 'synthetic transcript');
    assert.equal(args[args.indexOf('-l') + 1], 'fi');
    assert.match(clip, /\.wav$/);
    await assert.rejects(fs.access(path.dirname(clip)), { code: 'ENOENT' });
  });
}

test('only the WAV the renderer writes is accepted', async () => {
  const { encodeWav, SAMPLE_RATE, MAX_SECONDS } = await wav;
  const good = encodeWav(Float32Array.from({ length: 800 }, (_, i) => Math.sin(i / 10)));
  assert.equal(checkWav(good), good);
  const samples = encodeWav(Float32Array.of(0, 1, -1, 0.5, 2)), view = new DataView(samples.buffer);
  assert.deepEqual([0, 1, 2, 3, 4].map(i => view.getInt16(44 + 2 * i, true)), [0, 32767, -32768, 16383, 32767], 'clipped to 16-bit');
  const mutate = (offset, write) => { const copy = good.slice(); write(new DataView(copy.buffer), offset); return copy; };
  for (const bad of [
    new Uint8Array(0), good.subarray(0, 44), good.subarray(0, 100),          // empty, header only, truncated
    mutate(24, (v, o) => v.setUint32(o, 44100, true)),                       // another sample rate
    mutate(22, (v, o) => v.setUint16(o, 2, true)),                           // stereo
    mutate(34, (v, o) => v.setUint16(o, 8, true)),                           // 8-bit
    mutate(0, (v, o) => v.setUint8(o, 0x58)),                                 // not RIFF
    'not bytes'
  ]) assert.throws(() => checkWav(bad), /recording/i);
  assert.throws(() => checkWav(encodeWav(new Float32Array(SAMPLE_RATE * MAX_SECONDS + 1))), /two minutes/);
  assert.equal(checkWav(encodeWav(new Float32Array(SAMPLE_RATE * MAX_SECONDS))).byteLength, 44 + SAMPLE_RATE * MAX_SECONDS * 2);
});

test('the dictation language follows the preferred languages', () => {
  assert.equal(speechLanguage({}), 'auto');
  assert.equal(speechLanguage({ speechLanguages: ['fi'], speechLanguage: 'auto' }), 'fi', 'one language is always used');
  assert.equal(speechLanguage({ speechLanguages: ['fi', 'en'], speechLanguage: '' }), 'fi', 'the first is the default');
  assert.equal(speechLanguage({ speechLanguages: ['fi', 'en'], speechLanguage: 'en' }), 'en');
  assert.equal(speechLanguage({ speechLanguages: ['fi', 'en'], speechLanguage: 'auto' }), 'auto');
  assert.equal(speechLanguage({ speechLanguages: ['fi', 'en'], speechLanguage: 'de' }), 'fi', 'a choice no longer preferred falls back');
  assert.equal(speechLanguage({ speechLanguages: ['xx'] }), 'auto', 'unknown codes are ignored');
});

test('an English-only model warns when other languages are preferred', async t => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'parc-desktop-test-'));
  t.after(() => fs.rm(dir, { recursive: true, force: true }));
  const english = path.join(dir, 'ggml-base.en.bin'), multi = path.join(dir, 'ggml-small.bin'), bin = path.join(dir, 'whisper-cli');
  for (const f of [english, multi]) await fs.writeFile(f, 'model');
  await fs.writeFile(bin, '#!/bin/sh\n', { mode: 0o700 });
  const config = settings => speechConfig({ PATH: '' }, { whisperBin: bin, ...settings });
  assert.equal((await config({ whisperModel: english, speechLanguages: ['en'] })).warning, null);
  assert.match((await config({ whisperModel: english, speechLanguages: ['fi'] })).warning, /only understands English/);
  assert.equal((await config({ whisperModel: multi, speechLanguages: ['fi'] })).warning, null);
  assert.equal((await config({ whisperModel: multi, speechLanguages: ['fi'] })).language, 'fi');
  await assert.rejects(speechConfig({ PATH: dir + '-nowhere' }, { whisperModel: multi }), /whisper-cli/);
});

// ── Model downloads, against a fake server ──
function fakeModel(bytes) {
  return { id: 'test', file: 'ggml-test.bin', size: bytes.length, sha256: crypto.createHash('sha256').update(bytes).digest('hex'), english: false };
}
/** A fetch serving `bytes` in 1 KiB chunks. honorRange: answer Range with 206. failAt: throw after this many bytes. */
function fakeFetch(bytes, { honorRange = true, failAt = Infinity, status = 200 } = {}) {
  const calls = [];
  const fetch = async (url, init = {}) => {
    calls.push({ url, range: init.headers?.Range });
    if (status !== 200) return { ok: false, status, body: null };
    const start = honorRange && init.headers?.Range ? Number(/bytes=(\d+)-/.exec(init.headers.Range)[1]) : 0;
    const body = (async function* () {
      for (let i = start; i < bytes.length; i += 1024) {
        if (i >= failAt) throw new Error('connection reset');
        if (init.signal?.aborted) throw new DOMException('aborted', 'AbortError');
        yield bytes.subarray(i, Math.min(i + 1024, bytes.length));
      }
    })();
    return { ok: true, status: start ? 206 : 200, body };
  };
  return { fetch, calls };
}

test('a model download is verified, resumes after a failure, and lands atomically', async t => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'parc-desktop-test-'));
  t.after(() => fs.rm(dir, { recursive: true, force: true }));
  const bytes = crypto.randomBytes(10_000), model = fakeModel(bytes), target = path.join(dir, model.file);

  const broken = fakeFetch(bytes, { failAt: 4096 });
  await assert.rejects(models.download(model, { dir, fetch: broken.fetch }), /connection reset/);
  assert.equal((await fs.stat(target + '.part')).size, 4096);
  await assert.rejects(fs.access(target));
  assert.equal(await models.installed(model, dir), false);

  const progress = [];
  const resumed = fakeFetch(bytes);
  assert.equal(await models.download(model, { dir, fetch: resumed.fetch, onProgress: r => progress.push(r) }), target);
  assert.equal(resumed.calls[0].range, 'bytes=4096-');
  assert.equal(resumed.calls[0].url, models.SOURCE + model.file);
  assert.deepEqual(await fs.readFile(target), bytes);
  assert.equal(progress.at(-1), bytes.length);
  await assert.rejects(fs.access(target + '.part'));
  assert.equal(await models.installed(model, dir), true);
  assert.equal((await fs.stat(target)).mode & 0o777, 0o600);

  assert.equal(await models.remove(model, dir), target);
  assert.equal(await models.installed(model, dir), false);
});

test('a server that ignores the resume request restarts the download', async t => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'parc-desktop-test-'));
  t.after(() => fs.rm(dir, { recursive: true, force: true }));
  const bytes = crypto.randomBytes(5000), model = fakeModel(bytes);
  await assert.rejects(models.download(model, { dir, fetch: fakeFetch(bytes, { failAt: 2048 }).fetch }));
  await models.download(model, { dir, fetch: fakeFetch(bytes, { honorRange: false }).fetch });
  assert.deepEqual(await fs.readFile(path.join(dir, model.file)), bytes);
});

test('a file with its own pinned URL (the meaning model) downloads from it', async t => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'parc-desktop-test-'));
  t.after(() => fs.rm(dir, { recursive: true, force: true }));
  const bytes = crypto.randomBytes(3000), url = 'https://huggingface.co/org/model/resolve/0123abc/tokenizer.json';
  const file = { ...fakeModel(bytes), file: 'tokenizer.json', url }, fetched = fakeFetch(bytes);
  await models.download(file, { dir, fetch: fetched.fetch });
  assert.equal(fetched.calls[0].url, url);
  assert.deepEqual(await fs.readFile(path.join(dir, 'tokenizer.json')), bytes);
});

test('meaning.status names the pinned model files and where they go, without loading the model', async t => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'parc-desktop-test-'));
  t.after(() => fs.rm(dir, { recursive: true, force: true }));
  const store = archive(path.join(dir, '.parc'));
  await store.init();
  const status = await store.meaningStatus();
  assert.equal(status.installed, false);
  assert.equal(status.loaded, false);
  assert.deepEqual(status.coverage, { embedded: 0, total: 0 });
  assert.ok(status.model.files.length >= 3);
  for (const f of status.model.files) {
    assert.ok(f.url.startsWith('https://huggingface.co/') && f.url.includes(status.model.revision), f.url);
    assert.match(f.sha256, /^[a-f0-9]{64}$/);
  }
  assert.equal(status.model.size, status.model.files.reduce((n, f) => n + f.size, 0));
  store.close?.();
});

test('a download that does not match its hash is deleted', async t => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'parc-desktop-test-'));
  t.after(() => fs.rm(dir, { recursive: true, force: true }));
  const bytes = crypto.randomBytes(3000), model = { ...fakeModel(bytes), sha256: '0'.repeat(64) };
  await assert.rejects(models.download(model, { dir, fetch: fakeFetch(bytes).fetch }), /didn’t match/);
  await assert.rejects(fs.access(path.join(dir, model.file + '.part')));
  await assert.rejects(fs.access(path.join(dir, model.file)));

  const tooBig = fakeModel(bytes.subarray(0, 1000));
  await assert.rejects(models.download(tooBig, { dir, fetch: fakeFetch(bytes).fetch }), /larger than expected/);
  await assert.rejects(models.download(fakeModel(bytes), { dir, fetch: fakeFetch(bytes, { status: 404 }).fetch }), /HTTP 404/);
});

test('cancelling keeps the partial download for later', async t => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'parc-desktop-test-'));
  t.after(() => fs.rm(dir, { recursive: true, force: true }));
  const bytes = crypto.randomBytes(8192), model = fakeModel(bytes), controller = new AbortController();
  const download = models.download(model, {
    dir, fetch: fakeFetch(bytes).fetch, signal: controller.signal,
    onProgress: received => { if (received >= 2048) controller.abort(); }
  });
  await assert.rejects(download, { name: 'AbortError' });
  assert.ok((await fs.stat(path.join(dir, model.file + '.part'))).size >= 2048);
});

test('the catalog recommends by language and knows English-only models', () => {
  assert.equal(models.recommended([]), 'small.en');
  assert.equal(models.recommended(['en']), 'small.en');
  assert.equal(models.recommended(['en', 'fi']), 'large-v3-turbo-q5_0');
  assert.ok(models.MODELS.every(m => /^[0-9a-f]{64}$/.test(m.sha256) && m.size > 1e8 && m.file.startsWith('ggml-')));
  assert.ok(models.MODELS.some(m => m.id === models.recommended(['fi'])));
  assert.equal(models.englishOnly('/x/ggml-small.en.bin'), true);
  assert.equal(models.englishOnly('/x/ggml-small.en-q5_1.bin'), true);
  assert.equal(models.englishOnly('/x/ggml-large-v3-turbo-q5_0.bin'), false);
  assert.equal(models.englishOnly('/x/ggml-medium.bin'), false);
});

test('persistent parc-server child is reused across pipelined calls and recovers after crash or close', async t => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'parc-persistent-test-'));
  t.after(() => fs.rm(dir, { recursive: true, force: true }));
  const store = archive(path.join(dir, '.parc'), { idleMs: 60000 });
  t.after(() => store.close());
  await store.init();
  assert.equal(store.pid(), null, 'server starts lazily on first RPC');

  const first = await store.save({ title: 'First', body: 'Alpha' });
  const pid1 = store.pid();
  assert.ok(Number.isInteger(pid1) && pid1 > 0);

  // Concurrent pipelined calls reuse the exact same child process.
  const [second, list, got, schemas] = await Promise.all([
    store.save({ title: 'Second', body: 'Beta' }),
    store.list(),
    store.get(first.id),
    store.schemas()
  ]);
  assert.equal(store.pid(), pid1);
  assert.equal(got.title, 'First');
  assert.equal(second.title, 'Second');
  assert.ok(list.length >= 1 && schemas.length >= 5);

  // If the child crashes unexpectedly, the next request transparently spawns a fresh one.
  process.kill(pid1, 'SIGKILL');
  await new Promise(r => setTimeout(r, 80));
  const afterCrash = await store.get(second.id);
  const pid2 = store.pid();
  assert.equal(afterCrash.title, 'Second');
  assert.ok(Number.isInteger(pid2) && pid2 !== pid1);

  // Explicit close terminates the child; a subsequent call restarts cleanly.
  store.close();
  assert.equal(store.pid(), null);
  assert.equal((await store.list()).length, 2);
  assert.ok(store.pid() !== null);
});

test('disk draft store saves atomically, flushes synchronously, prunes orphans and validates IDs', async t => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'parc-drafts-test-'));
  t.after(() => fs.rm(dir, { recursive: true, force: true }));
  const store = draftStore(path.join(dir, 'drafts'));

  await store.save('tab-1', { title: 'Draft 1', body: 'Unsent text', base: '0123456789abcdef', at: '2026-11-25T00:00:00Z' });
  const loaded = await store.load('tab-1');
  assert.deepEqual(loaded, { title: 'Draft 1', body: 'Unsent text', base: '0123456789abcdef', at: '2026-11-25T00:00:00Z' });
  assert.equal((await fs.stat(path.join(dir, 'drafts', 'tab-1.json'))).mode & 0o777, 0o600);

  store.saveSync('tab-2', { title: 'Sync draft', body: 'Saved on unload', base: null, at: '2026-11-25T00:01:00Z' });
  assert.equal((await store.load('tab-2')).body, 'Saved on unload');

  await fs.writeFile(path.join(dir, 'drafts', 'stale.tmp'), 'partial');
  await store.prune(['tab-2']);
  assert.equal(await store.load('tab-1'), null);
  assert.equal((await store.load('tab-2')).body, 'Saved on unload');
  await assert.rejects(fs.access(path.join(dir, 'drafts', 'stale.tmp')));

  store.removeSync('tab-2');
  assert.equal(await store.load('tab-2'), null);

  await assert.rejects(store.save('../escape', { title: 'x', body: 'y' }), /Invalid draft ID/);
  await assert.rejects(store.save('huge', { title: '', body: 'x'.repeat(1_000_001) }), /too large/);
});

test('graph layout is deterministic for both small exact and large Barnes-Hut graphs', async () => {
  const { layout, ring } = await graphLayout;
  const make = n => ({
    nodes: Array.from({ length: n }, (_, i) => ({ key: `n${i}` })),
    links: Array.from({ length: n - 1 }, (_, i) => ({ source: `n${i}`, target: `n${(i * 7 + 1) % n}` }))
  });

  // Small exact path (<= 200)
  const s1 = make(30), s2 = make(30);
  layout(s1.nodes, s1.links); layout(s2.nodes, s2.links);
  assert.deepEqual(s1.nodes.map(d => [d.x, d.y]), s2.nodes.map(d => [d.x, d.y]));

  // Large Barnes-Hut path (> 200)
  const b1 = make(450), b2 = make(450);
  const t0 = Date.now();
  layout(b1.nodes, b1.links);
  const elapsed = Date.now() - t0;
  layout(b2.nodes, b2.links);
  assert.ok(b1.nodes.every(d => Number.isFinite(d.x) && Number.isFinite(d.y)));
  assert.deepEqual(b1.nodes.map(d => [d.x, d.y]), b2.nodes.map(d => [d.x, d.y]));
  assert.ok(elapsed < 3000, `Barnes-Hut layout on 450 nodes took ${elapsed}ms`);

  const loose = [{ key: 'u0' }, { key: 'u1' }];
  ring(loose, b1.nodes);
  assert.ok(loose.every(d => Number.isFinite(d.x) && Number.isFinite(d.y)));
});

test('promote, templates, attachments, trash management, doctor and reindex', async t => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'parc-parity-test-'));
  t.after(() => fs.rm(dir, { recursive: true, force: true }));
  const vault = path.join(dir, '.parc'), store = archive(vault);
  t.after(() => store.close());
  await store.init();

  // Schemas include built-in body templates (decision has sections, note is blank).
  const schemas = await store.schemas();
  assert.match(schemaOf(schemas, 'decision').template || '', /## Context/);
  assert.equal(schemaOf(schemas, 'note').template, null);

  // Promote converts a note into a todo while preserving body/tags and checking expected_revision.
  const note = await store.save({ title: 'Convert me', body: 'Keep this text.', tags: ['ops'] });
  const promoted = await store.promote(note.id, 'todo', note.revision, { assignee: 'Alex' });
  assert.equal(promoted.type, 'todo');
  assert.equal(promoted.status, 'open');
  assert.equal(promoted.assignee, 'Alex');
  assert.deepEqual(promoted.tags, ['ops']);
  assert.equal(promoted.body.trim(), 'Keep this text.');
  assert.notEqual(promoted.revision, note.revision);
  assert.equal((await store.promote(note.id, 'idea', note.revision)).conflict, true);

  // List fields (e.g. decision.deciders) save and clear cleanly.
  const dec = await store.save({ title: 'Pick engine', body: '## Context\nFast.', type: 'decision', fields: { deciders: ['Ada', 'Linus'] } });
  assert.deepEqual((await store.get(dec.id)).deciders, ['Ada', 'Linus']);
  const cleared = await store.update(dec.id, { fields: { deciders: null } }, dec.revision);
  assert.equal(cleared.deciders, undefined);

  // Attachments: attach updates index + revision, lists size, and detach cleans up.
  const sample = path.join(dir, 'diagram.txt');
  await fs.writeFile(sample, 'hello attachment');
  const attached = await store.attach(promoted.id, sample);
  assert.equal(attached.filename, 'diagram.txt');
  assert.deepEqual(attached.attachments, ['diagram.txt']);
  assert.ok(/^[0-9a-f]{16}$/.test(attached.revision));
  const listedAtt = await store.attachments(promoted.id);
  assert.deepEqual(listedAtt, [{ filename: 'diagram.txt', size: 16 }]);
  assert.equal((await store.search('has:attachments')).some(r => r.id === promoted.id), true);
  await assert.rejects(store.detach(promoted.id, '../escape'), /Invalid attachment/);
  const detached = await store.detach(promoted.id, 'diagram.txt');
  assert.equal(detached.detached, true);
  assert.deepEqual(detached.attachments, []);
  assert.deepEqual(await store.attachments(promoted.id), []);

  // Trash list & purge (single and empty all).
  const t1 = await store.save({ title: 'Discard one', body: 'one' });
  const t2 = await store.save({ title: 'Discard two', body: 'two' });
  await store.remove(t1.id);
  await store.remove(t2.id);
  assert.equal((await store.trashList()).length, 2);
  assert.equal((await store.trashPurge(t1.id)).purged, t1.id);
  assert.deepEqual((await store.trashList()).map(x => x.id), [t2.id]);
  assert.equal((await store.trashPurge(null)).purged_count, 1);
  assert.deepEqual(await store.trashList(), []);

  // Doctor detects broken links and reindex rebuilds cleanly.
  const broken = await store.save({ title: 'Dangling', body: 'See [[01AAAAAAAAAAAAAAAAAAAAAAAA]].' });
  const health = await store.doctor();
  assert.ok(health.fragments_checked >= 3);
  assert.ok(health.findings.some(f => f.type === 'broken_link' && f.source_id === broken.id));
  const reindexed = await store.reindex();
  assert.ok(reindexed.indexed >= 3);
  assert.deepEqual(reindexed.problems, []);
});
