const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const os = require('node:os');
const crypto = require('node:crypto');
const { archive, speechConfig, speechLanguage, checkWav, transcribe } = require('../backend.cjs');
const models = require('../models.cjs');
const wav = import('../wav.mjs');

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
  assert.equal((await store.list()).length, 1);
  assert.match(await fs.readFile(path.join(vault, 'fragments', note.id + '.md'), 'utf8'), /Volunteer orientation/);
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
