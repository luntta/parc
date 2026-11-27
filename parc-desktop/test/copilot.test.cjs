// GitHub Copilot: sign-in with GitHub's device flow against a fake github.com, and chats on a fake Copilot SDK with
// a real read-only parc-server behind parc's tools.
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const os = require('node:os');
const { archive, binary } = require('../backend.cjs');
const agent = require('../agent.cjs');
const copilot = require('../copilot.cjs');
const { fakeGitHub, fakeSdk } = require('./fake-copilot.cjs');

const instant = async (_ms, signal) => { if (signal?.aborted) throw new Error('aborted'); await new Promise(r => setImmediate(r)); };
const seal = text => Buffer.from(`sealed:${Buffer.from(text).toString('base64')}`);
const unseal = data => Buffer.from(data.toString().slice(7), 'base64').toString();
const keyring = { available: async () => true, seal: async t => seal(t), unseal: async d => unseal(d) };

async function setup(t) {
  const gh = await fakeGitHub();
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'parc-copilot-test-'));
  const servers = [];
  t.after(async () => { gh.close(); for (const s of servers) s.close(); await fs.rm(dir, { recursive: true, force: true }); });
  const opened = [];
  const auth = copilot.github({ file: path.join(dir, 'github.bin'), keyring, open: async url => { opened.push(url); }, fetch, wait: instant });
  const sdk = fakeSdk();
  const runtime = copilot.copilotRuntime({ auth, sdk: () => sdk.module, home: path.join(dir, 'copilot'), work: path.join(dir, 'copilot', 'work') });
  const vault = path.join(dir, '.parc'), store = archive(vault);
  await store.init();
  const pool = new Map(), server = p => {
    if (!pool.has(p)) { const s = agent.mcpServer(binary('parc-server'), ['--vault', p, '--mcp', '--read-only']); servers.push(s); pool.set(p, s); }
    return pool.get(p);
  };
  let prefs = {};
  const chatgpt = { token: async () => { throw new agent.AgentError('Sign in with ChatGPT first.', { signedOut: true }); } };
  const launch = () => agent.harness({
    auth: chatgpt, server, read: (p, id) => archive(p).get(id), fetch,
    settings: { load: async () => prefs, save: async c => { prefs = { ...prefs, ...c }; } },
    store: agent.chatStore(path.join(dir, 'chats')), copilot: runtime
  });
  return { gh, auth, sdk, runtime, launch, bot: launch(), vault, store, dir, opened, prefs: () => prefs };
}

test('Sign in with GitHub: device flow with parc’s own app, the code shown, slow_down obeyed, the token sealed', async t => {
  const { gh, auth, dir, opened } = await setup(t);
  assert.deepEqual(await auth.status(), { signedIn: false, login: '', name: '', signingIn: false, remembered: true, available: true });
  const codes = [];
  const status = await auth.signIn(c => codes.push(c));
  assert.equal(status.signedIn, true); assert.equal(status.login, 'octo');
  assert.deepEqual(gh.fake.seen.device, [{ client_id: 'Ov23liparctest00000', scope: 'read:user' }]);
  assert.equal(codes.length, 1); assert.equal(codes[0].code, 'ABCD-1234'); assert.equal(codes[0].url, `${gh.base}/login/device`);
  assert.deepEqual(opened, [`${gh.base}/login/device`]);
  // Polled until GitHub agreed: pending, slow down, then the token.
  assert.equal(gh.fake.seen.token.length, 3);
  assert.ok(gh.fake.seen.token.every(b => b.client_id === 'Ov23liparctest00000' && b.device_code === 'dev-1' && b.grant_type === 'urn:ietf:params:oauth:grant-type:device_code'));
  assert.equal(await auth.token(), 'gho_parctest');
  const raw = await fs.readFile(path.join(dir, 'github.bin'), 'utf8');
  assert.ok(!raw.includes('gho_parctest'));
  // Signed out: forgotten, file gone.
  await auth.signOut();
  assert.equal((await auth.status()).signedIn, false);
  await assert.rejects(fs.stat(path.join(dir, 'github.bin')), { code: 'ENOENT' });
  // Refused on GitHub; expired; device flow off; cancelled from parc.
  gh.fake.polls = []; gh.fake.error = 'access_denied';
  await assert.rejects(auth.signIn(() => {}), err => err.cancelled === true);
  gh.fake.error = 'device_flow_disabled';
  await assert.rejects(auth.signIn(() => {}), /Enable Device Flow/);
  gh.fake.error = null; gh.fake.polls = Array(50).fill('authorization_pending');
  const slow = copilot.github({ file: path.join(dir, 'other.bin'), keyring, open: async () => {}, fetch, wait: (ms, signal) => new Promise((r, j) => { const x = setTimeout(r, 5); signal.addEventListener('abort', () => { clearTimeout(x); j(new Error('aborted')); }); }) });
  const pending = slow.signIn(() => {});
  await new Promise(r => setTimeout(r, 30));
  slow.cancel();
  await assert.rejects(pending, err => err.cancelled === true);
  // Without parc's app registered, sign-in says so instead of trying.
  const id = copilot.GITHUB.clientId;
  copilot.GITHUB.clientId = '';
  try {
    assert.equal((await slow.status()).available, false);
    await assert.rejects(slow.signIn(() => {}), /no GitHub app/);
  } finally { copilot.GITHUB.clientId = id; }
});

test('a Copilot chat runs only parc’s read-only tools in an empty session, is saved, and goes on after a restart', async t => {
  const { gh, auth, sdk, runtime, launch, bot, vault, store, prefs } = await setup(t);
  gh.fake.polls = [];
  const ferry = await store.save({ title: 'Ferry desk', body: 'Route cards worked.' });
  await auth.signIn(() => {});
  // The plan's models, without the ones switched off.
  assert.deepEqual((await bot.models(false, 'copilot')).map(m => m.id), ['claude-sonnet-x', 'gpt-y']);
  await bot.setModel('gpt-y', 'copilot');
  assert.equal(prefs().copilotModel, 'gpt-y');
  // The runtime: empty mode, a child process with its data in parc's directory, the user's token, no telemetry.
  const [client] = sdk.fake.seen.clients;
  assert.equal(client.options.mode, 'empty');
  assert.equal(client.options.runtime.kind, 'spawned-stdio');
  assert.match(client.options.runtime.baseDirectory, /copilot$/);
  assert.equal(client.options.runtime.env.COPILOT_TELEMETRY_DISABLED, 'true');
  assert.deepEqual(client.options.github, { token: 'gho_parctest', useLoggedInUser: false });
  sdk.fake.script.push(async session => {
    const found = await session.tool('parc_search', { query: 'route' });
    assert.match(found, /Ferry desk/);
    const wrong = await session.tool('parc_get_fragment', { id: 'ZZZZZZZZ' });
    assert.equal(wrong.resultType, 'failure');
    session.say(`They worked [[${ferry.id}]].`);
    session.emit('session.idle');
  });
  const events = [];
  const vaults = [{ path: vault, name: 'Test' }];
  const summary = await bot.ask('cp-1', { text: 'Did the route cards work?', vaults, subject: { vault, id: ferry.id }, provider: 'copilot' }, e => events.push(e));
  assert.equal(summary.provider, 'copilot'); assert.equal(summary.questions, 1);
  const [session] = sdk.fake.seen.sessions;
  assert.equal(session.options.model, 'gpt-y');
  // Nothing but parc's tools, each read-only and named for parc; permission asks are refused.
  assert.deepEqual(session.options.tools.availableTools, ['custom:*']);
  assert.deepEqual(session.options.tools.definitions.map(d => d.name).sort(), ['parc_get_fragment', 'parc_list_tags', 'parc_list_types', 'parc_read_attachment', 'parc_recall', 'parc_search']);
  assert.ok(session.options.tools.definitions.every(d => d.skipPermission === true));
  assert.equal(session.options.permissions.onRequest({ kind: 'shell' }).kind, 'reject');
  assert.equal(session.options.systemMessage.mode, 'replace');
  assert.match(session.options.systemMessage.content, /\[\[ID\]\]/);
  assert.equal(session.options.instructions.skipCustomInstructions, true);
  assert.equal(session.options.github.telemetryEnabled, false);
  // The open fragment goes with the question.
  assert.match(sdk.fake.seen.prompts[0].prompt, /<open_fragment[\s\S]*Ferry desk[\s\S]*<\/open_fragment>\s+Did the route cards work\?$/);
  assert.deepEqual(events.map(e => e.type).filter((x, i, all) => x !== 'text' || all[i - 1] !== 'text'), ['shared', 'tool', 'tool-done', 'tool', 'tool-done', 'text']);
  assert.equal(events.filter(e => e.type === 'text').map(e => e.delta).join(''), `They worked [[${ferry.id}]].`);
  assert.equal(events[1].name, 'search');
  // Saved, and shown again as it was.
  const shown = await bot.chat('cp-1');
  assert.equal(shown.provider, 'copilot');
  assert.deepEqual(shown.turns.map(x => x.role), ['user', 'agent']);
  assert.equal(shown.turns[0].shared, 'Ferry desk');
  assert.deepEqual(shown.turns[1].parts.map(p => p.kind), ['steps', 'text']);
  assert.deepEqual(shown.turns[1].parts[0].steps.map(s => [s.name, s.ok]), [['search', true], ['get_fragment', false]]);
  assert.equal(shown.turns[1].parts[1].text, `They worked [[${ferry.id}]].`);
  // A follow-up in the same run: the same session, the fragment not shared twice.
  sdk.fake.script.push(session => { session.say('Nowhere else.'); session.emit('session.idle'); });
  await bot.ask('cp-1', { text: 'Anywhere else?', vaults, subject: { vault, id: ferry.id }, provider: 'chatgpt' }, () => {});
  assert.equal(sdk.fake.seen.sessions.length, 1, 'the live session goes on');
  assert.equal(sdk.fake.seen.prompts[1].prompt, 'Anywhere else?');
  assert.equal((await bot.chat('cp-1')).provider, 'copilot', 'a saved chat keeps its provider');
  // After a restart: a new session, told what was said.
  bot.forget('cp-1');
  await new Promise(r => setImmediate(r));
  assert.deepEqual(sdk.fake.seen.deleted, [session.sessionId], 'a session leaves the runtime with its chat');
  const again = launch();
  sdk.fake.script.push(s => { s.say('Still the ferry desk.'); s.emit('session.idle'); });
  await again.ask('cp-1', { text: 'Remind me?', vaults, subject: null }, () => {});
  const fresh = sdk.fake.seen.sessions[1];
  assert.match(fresh.options.systemMessage.content, /Earlier in this conversation[\s\S]*User: Did the route cards work\?\nYou: They worked \[\[/);
  assert.match(fresh.options.systemMessage.content, /User: Anywhere else\?\nYou: Nowhere else\./);
  assert.equal((await again.chat('cp-1')).questions, 3);
  // Both providers' chats are listed together.
  assert.deepEqual((await again.chats()).map(c => [c.id, c.provider]), [['cp-1', 'copilot']]);
  await runtime.stop();
});

test('Copilot failures: a failed or stopped question isn’t kept, its session goes, and a refused sign-in ends it', async t => {
  const { gh, auth, sdk, bot, vault, store } = await setup(t);
  gh.fake.polls = [];
  await store.save({ title: 'Here', body: 'Something.' });
  await auth.signIn(() => {});
  const vaults = [{ path: vault, name: 'Test' }];
  sdk.fake.script.push(s => { s.say('Fine.'); s.emit('session.idle'); });
  await bot.ask('cp-2', { text: 'First', vaults, subject: null, provider: 'copilot' }, () => {});
  sdk.fake.script.push(s => { s.say('Partly'); s.emit('session.error', { errorType: 'quota', message: 'You have exceeded your premium request allowance.', statusCode: 402 }); });
  await assert.rejects(bot.ask('cp-2', { text: 'Second', vaults, subject: null }, () => {}), /limit for your plan is reached/);
  assert.equal(sdk.fake.seen.deleted.length, 1, 'the half-done session was dropped');
  assert.equal((await bot.chat('cp-2')).questions, 1, 'the failed question isn’t kept');
  // Stop.
  sdk.fake.script.push(() => new Promise(() => {}));
  const stopped = bot.ask('cp-2', { text: 'Slow', vaults, subject: null }, () => {});
  await new Promise(r => setTimeout(r, 30));
  bot.stop('cp-2');
  await assert.rejects(stopped, err => err.cancelled === true);
  assert.ok(sdk.fake.seen.sessions.at(-1).aborted);
  assert.equal((await bot.chat('cp-2')).questions, 1);
  // GitHub refuses the token: signed out, and the view is told so.
  sdk.fake.script.push(() => { const err = new Error('Authentication failed: Bad credentials'); err.statusCode = 401; throw err; });
  await assert.rejects(bot.ask('cp-2', { text: 'Again', vaults, subject: null }, () => {}), err => err.signedOut === true);
  assert.equal((await auth.status()).signedIn, false);
  // Not signed in: models say so.
  await assert.rejects(bot.models(false, 'copilot'), err => err.signedOut === true);
});
