const test = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');
const { archive, rpcClient, whileBusy } = require('../backend.cjs');

const fake = [process.execPath, path.join(__dirname, 'fake-server.cjs')];
const VAULT = '/nowhere/.parc';
async function until(check, ms = 3000) {
  for (const end = Date.now() + ms; Date.now() < end; await new Promise(r => setTimeout(r, 10))) if (check()) return;
  throw new Error(`Timeout: ${check}`);
}
function withEnv(vars, fn) {
  return async () => {
    Object.assign(process.env, vars);
    try { await fn(); } finally { for (const k of Object.keys(vars)) delete process.env[k]; }
  };
}

test('requests queued behind a slow one wait without a clock, and the slow one has a longer one', withEnv({ FAKE_SLOW_MS: '400' }, async () => {
  const c = rpcClient(VAULT, { command: fake, timeout: 200, slowTimeout: 2000, idleMs: 0 });
  try {
    const replies = await Promise.all([c.send('vault.reindex', {}), ...Array.from({ length: 5 }, () => c.send('fragment.get', {}))]);
    assert.equal(new Set(replies.map(r => r.result.pid)).size, 1, 'one server answered them all, never restarted');
  } finally { c.close(); }
}));

test('a hung read times out alone; undispatched requests continue on a fresh server', async () => {
  const c = rpcClient(VAULT, { command: fake, timeout: 150, idleMs: 0 });
  try {
    const hung = c.send('hang', {}), behind = c.send('fragment.get', {});
    const first = c.pid();
    await assert.rejects(hung, /timeout: hang/);
    assert.notEqual((await behind).result.pid, first);
    const again = await c.send('fragment.get', {});
    assert.notEqual(again.result.pid, first);
  } finally { c.close(); }
});

test('an answer that arrives in many pieces is read whole', async () => {
  const c = rpcClient(VAULT, { command: fake, idleMs: 0 });
  try {
    const [big, small] = await Promise.all([c.send('big', {}), c.send('fragment.get', {})]);
    assert.equal(big.result.text.length, 2_000_000);
    assert.equal(small.result.method, 'fragment.get');
  } finally { c.close(); }
});

test('an idle server stays while its background work goes on, and closes once it is done', withEnv({ FAKE_LEFT: '2' }, async () => {
  let checks = 0;
  const busy = whileBusy();
  const c = rpcClient(VAULT, { command: fake, idleMs: 30, keepAlive: send => { checks++; return busy(send); } });
  try {
    await c.send('fragment.get', {});
    assert.ok(c.pid());
    await until(() => c.pid() === null);
    assert.equal(checks, 3, 'kept at 2 and 1 left, closed at 0');
  } finally { c.close(); }
}));

test('background work that stops moving no longer keeps an idle server', withEnv({ FAKE_LEFT: '5', FAKE_STUCK: '1' }, async () => {
  let checks = 0;
  const busy = whileBusy({ stalled: 2 });
  const c = rpcClient(VAULT, { command: fake, idleMs: 30, keepAlive: send => { checks++; return busy(send); } });
  try {
    await c.send('fragment.get', {});
    await until(() => c.pid() === null);
    assert.equal(checks, 3, 'kept on the first look and one unchanged one, closed on the second unchanged one');
  } finally { c.close(); }
}));

test('a timed-out document read cannot abort or delay a save on the writer', withEnv({ FAKE_SLOW_MS: '1200' }, async () => {
  const db = archive(VAULT, { command: fake, slowTimeout: 600, idleMs: 0 });
  try {
    let done = false;
    const hung = assert.rejects(db.grep('x'), /timeout/).then(() => { done = true; });
    assert.equal((await db.save({ title: 'new', body: '' })).method, 'fragment.create');
    assert.equal(done, false);
    await hung;
  } finally { db.close(); }
}));

test('a write with a lost acknowledgement is never replayed; unsent requests are identified', withEnv({ FAKE_SLOW_WRITE: '1', FAKE_SLOW_MS: '450' }, async () => {
  const c = rpcClient(VAULT, { command: fake, timeout: 200, idleMs: 0 });
  try {
    await c.send('ready', {});
    await Promise.all([
      assert.rejects(c.send('fragment.create', {}), /may already have been applied/),
      assert.rejects(c.send('fragment.update', {}), /not sent/i)
    ]);
    await assert.rejects(c.send('fragment.get', {}), /not sent/i);
    const pid = c.pid();
    await new Promise(r => setTimeout(r, 350));
    assert.equal((await c.send('fragment.get', {})).result.pid, pid);
  } finally { c.close(); }
}));
