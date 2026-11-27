// The Agent's harness against a fake OpenAI (sign-in, token, models, responses) and a real read-only parc-server.
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const os = require('node:os');
const { archive, binary } = require('../backend.cjs');
const agent = require('../agent.cjs');

const DEVICE = '0f8fad5b-d9cb-469f-a165-70867728950e';
const { fakeOpenAI, browser, message, completed } = require('./fake-openai.cjs');

async function setup(t) {
  const { fake, close } = await fakeOpenAI();
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'parc-agent-test-'));
  const servers = [];
  t.after(async () => { close(); for (const s of servers) s.close(); await fs.rm(dir, { recursive: true, force: true }); });
  // A keyring that marks what it seals, so the test can tell the file isn't the token in the clear.
  const seal = text => Buffer.from(`sealed:${Buffer.from(text).toString('base64')}`);
  const unseal = data => { const s = data.toString(); assert.ok(s.startsWith('sealed:')); return Buffer.from(s.slice(7), 'base64').toString(); };
  const keyring = { available: async () => true, seal: async text => seal(text), unseal: async data => unseal(data) };
  const file = path.join(dir, 'chatgpt.bin');
  const auth = agent.chatgpt({ file, keyring, deviceId: () => DEVICE, open: browser, fetch });
  let prefs = {};
  const vault = path.join(dir, '.parc'), store = archive(vault);
  await store.init();
  const server = p => { const s = agent.mcpServer(binary('parc-server'), ['--vault', p, '--mcp', '--read-only']); servers.push(s); return s; };
  const pool = new Map(), toolServer = p => { if (!pool.has(p)) pool.set(p, server(p)); return pool.get(p); };
  const settings = { load: async () => prefs, save: async c => { prefs = { ...prefs, ...c }; } };
  const chats = path.join(dir, 'chats');
  // A harness as the app makes one at launch: the same sign-in, tool servers and saved chats.
  const launch = () => agent.harness({ auth, server: toolServer, read: (p, id) => archive(p).get(id), fetch, settings, store: agent.chatStore(chats) });
  const bot = launch();
  return { fake, auth, bot, launch, file, seal, unseal, keyring, vault, store, dir, chats, prefs: () => prefs };
}

test('Sign in with ChatGPT registers parc, seals the credential, refreshes it and remembers the client', async t => {
  const { fake, auth, file, seal, unseal, keyring } = await setup(t);
  assert.deepEqual(await auth.status(), { signedIn: false, email: '', name: '', signingIn: false, remembered: true });
  const status = await auth.signIn();
  assert.equal(status.signedIn, true); assert.equal(status.email, 'reader@example.com');
  // The first sign-in registers parc by name, asks for the plan grant with PKCE, and names this installation.
  const [first] = fake.seen.authorize;
  assert.equal(first.client_id, 'dynamic_agent_client'); assert.equal(first.agent_name_hint, 'parc');
  assert.equal(first.ext_agent_host_id, `urn:uuid:${DEVICE}`);
  assert.match(first.scope, /\bchatgpt\.tokens\.use\.direct\b/); assert.match(first.scope, /\boffline_access\b/);
  assert.equal(first.resource, 'https://api.openai.com/v1'); assert.equal(first.code_challenge_method, 'S256');
  assert.match(first.redirect_uri, /^http:\/\/127\.0\.0\.1:\d+\/auth\/callback$/);
  const exchange = fake.seen.token[0];
  assert.equal(exchange.grant_type, 'authorization_code'); assert.equal(exchange.redirect_uri, first.redirect_uri); assert.equal(exchange.resource, 'https://api.openai.com/v1');
  // Kept sealed, never in the clear.
  const raw = await fs.readFile(file, 'utf8');
  assert.ok(!raw.includes('at-1')); assert.ok(JSON.parse(unseal(Buffer.from(raw))).access === 'at-1');
  assert.equal(await auth.token(), 'at-1');
  // A second app instance reads the same sign-in.
  const again = agent.chatgpt({ file, keyring, deviceId: () => DEVICE, open: browser, fetch });
  assert.equal((await again.status()).email, 'reader@example.com');
  // An expired access token is refreshed with the issued client ID, once even when asked twice at once.
  const sealed = JSON.parse(unseal(await fs.readFile(file)));
  await fs.writeFile(file, seal(JSON.stringify({ ...sealed, expires: 0 })));
  const stale = agent.chatgpt({ file, keyring, deviceId: () => DEVICE, open: browser, fetch });
  assert.deepEqual(await Promise.all([stale.token(), stale.token()]), ['at-2', 'at-2']);
  const refreshes = fake.seen.token.filter(f => f.grant_type === 'refresh_token');
  assert.equal(refreshes.length, 1); assert.equal(refreshes[0].client_id, 'oaiapp_test'); assert.equal(refreshes[0].refresh_token, 'rt-1');
  assert.equal((await stale.status()).email, 'reader@example.com', 'a refresh without an ID token keeps the account');
  // Signed out, the client stays: the next sign-in reuses it rather than registering parc again.
  await stale.signOut();
  assert.equal((await stale.status()).signedIn, false);
  await stale.signIn();
  assert.equal(fake.seen.authorize[1].client_id, 'oaiapp_test'); assert.equal(fake.seen.authorize[1].agent_name_hint, undefined);
  // A refresh the server refuses for good ends the sign-in.
  fake.refreshError = 'refresh_token_reused';
  await assert.rejects(stale.token({ renew: true }), err => err.signedOut === true);
  assert.equal((await stale.status()).signedIn, false);
});

test('a sign-in without the plan grant is refused, and without a keyring nothing is written', async t => {
  const { fake, dir } = await setup(t);
  fake.scope = 'openid profile email offline_access';
  const file = path.join(dir, 'memory-only.bin');
  const auth = agent.chatgpt({ file, keyring: null, deviceId: () => DEVICE, open: browser, fetch });
  await assert.rejects(auth.signIn(), /didn’t allow parc to use your plan/);
  fake.scope += ' resource.invoke chatgpt.tokens.use.direct';
  assert.equal((await auth.signIn()).remembered, false);
  assert.equal(await auth.token(), 'at-1');
  await assert.rejects(fs.stat(file), { code: 'ENOENT' });
  // Cancelling a sign-in in progress ends it.
  const slow = agent.chatgpt({ file, keyring: { available: async () => false }, deviceId: () => DEVICE, open: async () => {}, fetch });
  const pending = slow.signIn();
  await new Promise(r => setTimeout(r, 50));
  slow.cancel();
  await assert.rejects(pending, err => err.cancelled === true);
});

test('a question runs the vault’s read-only tools, cites what they found and keeps the chat stateless', async t => {
  const { fake, auth, bot, vault, store, prefs } = await setup(t);
  const ferry = await store.save({ title: 'Ferry desk', body: 'A route card to mark in private worked.' });
  await store.save({ title: 'Kitchen timer', body: 'Broken again.' });
  await auth.signIn();
  assert.deepEqual((await bot.models()).map(m => m.id), ['gpt-test', 'gpt-other']);
  await bot.setModel('gpt-other');
  assert.equal(prefs().model, 'gpt-other');
  await assert.rejects(bot.setModel('gpt-hidden'));
  // Round one: the model searches. Round two: it answers from what the search returned.
  fake.respond.push(
    () => [completed([{ type: 'reasoning', id: 'rs_1', encrypted_content: 'opaque', summary: [] }, { type: 'function_call', id: 'fc_1', call_id: 'call_1', name: 'search', arguments: JSON.stringify({ query: 'route card' }), status: 'completed' }])],
    request => {
      const output = request.input.find(i => i.type === 'function_call_output');
      assert.match(output.output, new RegExp(ferry.id));
      const text = `The route cards worked [[${ferry.id}]].`;
      return [{ type: 'response.output_text.delta', delta: text.slice(0, 10) }, { type: 'response.output_text.delta', delta: text.slice(10) }, completed([message(text)])];
    });
  const events = [];
  const reply = await bot.ask('chat-1', { text: 'Did the route cards work?', vaults: [{ path: vault, name: 'Test' }], subject: { vault, id: ferry.id } }, e => events.push(e));
  assert.equal(reply.title, 'Did the route cards work?'); assert.equal(reply.questions, 1);
  assert.deepEqual(events.map(e => e.type), ['shared', 'tool', 'tool-done', 'text', 'text']);
  assert.equal(events[0].title, 'Ferry desk');
  assert.equal(events[2].ok, true); assert.match(events[2].text, /Ferry desk/);
  assert.equal(events.filter(e => e.type === 'text').map(e => e.delta).join(''), `The route cards worked [[${ferry.id}]].`);
  const [one, two] = fake.seen.responses;
  // What the plan's API accepts: stored never, streamed always, no sampling or token limits, no system messages.
  for (const r of [one, two]) {
    assert.equal(r.model, 'gpt-other'); assert.equal(r.store, false); assert.equal(r.stream, true);
    for (const banned of ['temperature', 'top_p', 'max_output_tokens', 'previous_response_id', 'metadata', 'user', 'truncation']) assert.equal(r[banned], undefined, banned);
    assert.ok(!r.input.some(i => i.role === 'system'));
    assert.match(r.instructions, /\[\[ID\]\]/);
  }
  // Only the read tools, as the server lists them.
  assert.deepEqual(one.tools.map(x => x.name).sort(), ['get_fragment', 'list_tags', 'list_types', 'read_attachment', 'recall', 'search']);
  // The fragment the user has open goes along as a developer message, once.
  assert.match(one.input[0].content[0].text, /Ferry desk/); assert.equal(one.input[0].role, 'developer');
  // The second request carries the whole first round back: reasoning (encrypted), the call and its output.
  assert.deepEqual(two.input.map(i => i.type), ['message', 'message', 'reasoning', 'function_call', 'function_call_output']);
  // A follow-up: the history so far, the subject not repeated.
  fake.respond.push(() => [completed([message('Nothing else.')])]);
  await bot.ask('chat-1', { text: 'Anything else?', vaults: [{ path: vault, name: 'Test' }], subject: { vault, id: ferry.id } }, () => {});
  const three = fake.seen.responses[2];
  assert.equal(three.input.filter(i => i.role === 'developer').length, 1);
  assert.deepEqual(three.input.slice(-2).map(i => i.role), ['assistant', 'user']);
});

test('the model can’t reach a write tool or another vault, and errors leave the chat as it was', async t => {
  const { fake, auth, bot, vault, store, dir } = await setup(t);
  const other = path.join(dir, 'other', '.parc');
  await archive(other).init();
  await archive(other).save({ title: 'Elsewhere', body: 'In the second vault.' });
  await store.save({ title: 'Here', body: 'In the first vault.' });
  await auth.signIn();
  const vaults = [{ path: vault, name: 'One' }, { path: other, name: 'Two' }];
  fake.respond.push(
    () => [completed([
      { type: 'function_call', id: 'fc_1', call_id: 'c1', name: 'create_fragment', arguments: '{"type":"note","title":"x","vault":"One"}' },
      { type: 'function_call', id: 'fc_2', call_id: 'c2', name: 'search', arguments: '{"query":"vault","vault":"Nowhere"}' },
      { type: 'function_call', id: 'fc_3', call_id: 'c3', name: 'search', arguments: '{"query":"vault","vault":"Two"}' }])],
    request => {
      const out = Object.fromEntries(request.input.filter(i => i.type === 'function_call_output').map(i => [i.call_id, i.output]));
      assert.match(out.c1, /no tool called create_fragment/);
      assert.match(out.c2, /name the vault, one of: One, Two/);
      assert.match(out.c3, /Elsewhere/); assert.doesNotMatch(out.c3, /Here/);
      return [completed([message('Done.')])];
    });
  await bot.ask('chat-2', { text: 'Make a note', vaults, subject: null }, () => {});
  // With two vaults every tool must say which one.
  assert.ok(fake.seen.responses[0].tools.every(x => x.parameters.required.includes('vault') && x.parameters.properties.vault.enum.join() === 'One,Two'));
  assert.equal((await archive(vault).list()).length, 1, 'nothing was written');
  // The plan's limit, before streaming: a clear message, and the failed question isn't kept.
  const before = fake.seen.responses.length;
  fake.respond.push({ status: 429, body: { error: { code: 'subscription_sharing_usage_limit_exceeded', message: 'limit' } } });
  await assert.rejects(bot.ask('chat-2', { text: 'Again?', vaults, subject: null }, () => {}), /reached what your ChatGPT plan lets parc use/);
  // A failure after streaming started.
  fake.respond.push(() => [{ type: 'response.output_text.delta', delta: 'Part' }, { type: 'response.failed', response: { error: { code: 'subscription_sharing_user_not_eligible', message: 'no' } } }]);
  await assert.rejects(bot.ask('chat-2', { text: 'Once more?', vaults, subject: null }, () => {}), /can’t be used in other apps/);
  fake.respond.push(() => [completed([message('Fine.')])]);
  await bot.ask('chat-2', { text: 'Last one', vaults, subject: null }, () => {});
  const last = fake.seen.responses.at(-1);
  assert.deepEqual(last.input.filter(i => i.role === 'user').map(i => i.content[0].text), ['Make a note', 'Last one']);
  assert.equal(fake.seen.responses.length, before + 3);
  // Stopping takes the question back too.
  fake.respond.push({ hang: true });
  const stopped = bot.ask('chat-3', { text: 'Slow', vaults, subject: null }, () => {});
  await new Promise(r => setTimeout(r, 100));
  bot.stop('chat-3');
  await assert.rejects(stopped, err => err.cancelled === true);
});

test('chats are saved after each answer, listed, reopened after a restart and continued; deleting can be undone', async t => {
  const { fake, auth, bot, launch, vault, store, dir, chats } = await setup(t);
  const ferry = await store.save({ title: 'Ferry desk', body: 'Route cards worked.' });
  const other = path.join(dir, 'other', '.parc');
  await archive(other).init();
  await auth.signIn();
  const vaults = [{ path: vault, name: 'Test' }];
  // A question that fails isn't saved: the chat has nothing to keep yet.
  fake.respond.push({ status: 429, body: { error: { code: 'subscription_sharing_usage_limit_exceeded' } } });
  await assert.rejects(bot.ask('c-1', { text: 'Too soon', vaults, subject: null }, () => {}));
  assert.deepEqual(await bot.chats(), []);
  assert.equal(await bot.vaultsOf('c-1'), null);
  // An answer saves it, named for its first question, with the vaults it searched.
  fake.respond.push(
    () => [completed([{ type: 'function_call', id: 'fc_1', call_id: 'k1', name: 'search', arguments: '{"query":"route"}' }])],
    () => [completed([{ type: 'reasoning', id: 'rs_1', encrypted_content: 'x', summary: [] }, message(`They worked [[${ferry.id}]].`)])]);
  const long = 'Did the route cards at the ferry desk work out in the end, and should we try them anywhere else this spring?';
  const summary = await bot.ask('c-1', { text: long, vaults, subject: { vault, id: ferry.id } }, () => {});
  assert.equal(summary.title, 'Did the route cards at the ferry desk work out in the end, and should we try…');
  assert.deepEqual(summary.vaults, [vault]); assert.equal(summary.questions, 1);
  assert.deepEqual((await bot.chats()).map(c => c.id), ['c-1']);
  // The file is the app's, readable only by the user.
  const saved = await fs.stat(path.join(chats, 'c-1.json'));
  assert.equal(saved.mode & 0o077, 0);
  // After a restart: the chat as it was shown, and the vaults it keeps.
  const again = launch();
  assert.deepEqual(await again.vaultsOf('c-1'), [vault]);
  const shown = await again.chat('c-1');
  assert.equal(shown.title, summary.title);
  assert.deepEqual(shown.turns.map(x => x.role), ['user', 'agent']);
  assert.equal(shown.turns[0].text, long); assert.equal(shown.turns[0].shared, 'Ferry desk');
  assert.deepEqual(shown.turns[1].parts.map(p => p.kind), ['steps', 'text']);
  const [found] = shown.turns[1].parts[0].steps;
  assert.equal(found.name, 'search'); assert.equal(found.ok, true); assert.match(found.text, /Ferry desk/);
  assert.equal(shown.turns[1].parts[1].text, `They worked [[${ferry.id}]].`);
  // Going on: the model gets the whole chat back, and the fragment isn't shared twice.
  fake.respond.push(() => [completed([message('Nowhere else yet.')])]);
  const next = await again.ask('c-1', { text: 'Anywhere else?', vaults, subject: { vault, id: ferry.id } }, () => {});
  assert.equal(next.questions, 2); assert.equal(next.title, summary.title);
  const sent = fake.seen.responses.at(-1).input;
  assert.deepEqual(sent.map(i => i.type), ['message', 'message', 'function_call', 'function_call_output', 'reasoning', 'message', 'message']);
  assert.equal(sent.filter(i => i.role === 'developer').length, 1);
  // A second chat; the list is most recent first.
  fake.respond.push(() => [completed([message('Empty.')])]);
  await again.ask('c-2', { text: 'What is in the other vault?', vaults: [{ path: other, name: 'Other' }], subject: null }, () => {});
  assert.deepEqual((await again.chats()).map(c => [c.id, c.vaults[0]]), [['c-2', other], ['c-1', vault]]);
  // Deleted until the next launch, so it can come back.
  await again.remove('c-1');
  assert.deepEqual((await again.chats()).map(c => c.id), ['c-2']);
  assert.equal(await again.chat('c-1'), null);
  await again.restore('c-1');
  assert.deepEqual((await again.chats()).map(c => c.id), ['c-2', 'c-1']);
  assert.equal((await again.chat('c-1')).turns.length, 4);
  await again.remove('c-2');
  const later = launch();
  assert.deepEqual((await later.chats()).map(c => c.id), ['c-1']);
  await assert.rejects(later.restore('c-2'), 'gone for good after a restart');
  // Chat IDs name files: nothing else gets through.
  await assert.rejects(later.remove('../config'), /Invalid chat/);
});

test('analyze reads attachment via read-only read_attachment and returns sanitized proposals without writing', async t => {
  const { fake, auth, bot, vault, store, dir } = await setup(t);
  const existing = await store.save({ title: 'Autumn ferry schedule', body: 'Check timetable updates before October.' });
  const src = await store.save({ title: 'Quarterly report', body: '', type: 'source', tags: ['existing'] });
  const docPath = path.join(dir, 'report.txt');
  await fs.writeFile(docPath, 'Page one findings.\n\nAction: review ferry timetable before Q4 launch.');
  await store.attach(src.id, docPath);
  await auth.signIn();

  fake.respond.push(
    () => [completed([
      { type: 'function_call', id: 'fc_1', call_id: 'a1', name: 'read_attachment', arguments: JSON.stringify({ id: src.id, filename: 'report.txt', from: 1, to: 2 }) }
    ])],
    request => {
      const out = request.input.find(i => i.type === 'function_call_output');
      assert.match(out.output, /ferry timetable/);
      const json = JSON.stringify({
        summary: 'Reports Q4 launch readiness and highlights ferry timetable review.',
        tags: ['existing', 'q4-launch', '#ferry'],
        links: [
          { id: existing.id, reason: 'Discusses autumn ferry schedule.' },
          { id: src.id, reason: 'Self link should be stripped.' },
          { id: 'not-a-ulid', reason: 'Invalid ID should be stripped.' }
        ],
        todos: [{ title: 'Review ferry timetable before Q4 launch', body: 'See p. 1 of report.txt.' }]
      });
      return [completed([message(`\`\`\`json\n${json}\n\`\`\``)])];
    }
  );

  const res = await bot.analyze({ vault: { path: vault, name: 'Test' }, id: src.id, filename: 'report.txt', provider: 'chatgpt' });
  assert.equal(res.ok, true);
  assert.match(res.proposal.summary, /Q4 launch/);
  assert.deepEqual(res.proposal.tags, ['q4-launch', 'ferry']);
  assert.deepEqual(res.proposal.links, [{ id: existing.id, reason: 'Discusses autumn ferry schedule.' }]);
  assert.deepEqual(res.proposal.todos, [{ title: 'Review ferry timetable before Q4 launch', body: 'See p. 1 of report.txt.' }]);
  // Read-only guarantee: analyze never wrote to the vault.
  assert.equal((await store.list()).length, 2);
  assert.deepEqual((await store.get(src.id)).tags, ['existing']);
});
