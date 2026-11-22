const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const os = require('node:os');
const { archive, speechConfig, transcribe } = require('../backend.cjs');

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
    let clip;
    const mock = async (file, args) => {
      if (file === 'ffmpeg') { clip = args[args.indexOf('-i') + 1]; return; }
      if (fail) throw new Error('model failed');
      await fs.writeFile(args[args.indexOf('-of') + 1] + '.txt', ' synthetic transcript\n');
    };
    const result = transcribe(new Uint8Array([1]), { model: 'local-model', command: 'whisper-cli' }, mock);
    if (fail) await assert.rejects(result, /model failed/);
    else assert.equal(await result, 'synthetic transcript');
    await assert.rejects(fs.access(path.dirname(clip)), { code: 'ENOENT' });
  });
}
