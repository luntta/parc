// GitHub Copilot for the Agent view, in the main process: sign-in with GitHub and the Copilot SDK.
// Sign-in is GitHub's device flow with parc's own OAuth app: GitHub shows a code page, the user types the code parc
// shows, and parc gets a token for their account. The Copilot SDK then runs Copilot's agent on the user's plan, in
// its "empty" mode: nothing ambient (no shell, files, instructions or memory), only parc's custom tools, each a
// read-only vault tool. The renderer never sees the token. No Electron here, so the tests can pass in fakes.
const fs = require('node:fs/promises');
const { AgentError, sealedBox } = require('./agent.cjs');

// parc's GitHub OAuth app (Settings → Developer settings → OAuth Apps, with "Enable Device Flow" on). Its client ID is
// public; device flow needs no secret. With none, sign-in says so instead of trying.
const GITHUB = {
  clientId: 'Ov23liH4nFopbPRJZRhx',
  device: 'https://github.com/login/device/code',
  token: 'https://github.com/login/oauth/access_token',
  api: 'https://api.github.com'
};
const SCOPE = 'read:user';
const DEVICE_GRANT = 'urn:ietf:params:oauth:grant-type:device_code';

// ── Sign in with GitHub (device flow) ──
/**
 * file: where the sealed token is kept. keyring: as chatgpt() in agent.cjs. open(url): the browser. fetch: the network.
 * wait(ms, signal): a pause (the tests make it instant).
 */
function github({ file, keyring, open, fetch, wait = pause }) {
  const box = sealedBox(file, keyring);
  let signingIn = null;

  async function post(url, body, signal) {
    const reply = await fetch(url, {
      method: 'POST', signal,
      headers: { accept: 'application/json', 'content-type': 'application/json', 'user-agent': 'parc-desktop' },
      body: JSON.stringify(body)
    });
    const json = await reply.json().catch(() => ({}));
    if (!reply.ok && !json.error) throw new AgentError(`GitHub sign-in failed (${reply.status}).`);
    return json;
  }
  async function who(token) {
    const reply = await fetch(`${GITHUB.api}/user`, { headers: { accept: 'application/vnd.github+json', authorization: `Bearer ${token}`, 'x-github-api-version': '2022-11-28', 'user-agent': 'parc-desktop' } });
    if (!reply.ok) throw new AgentError(`GitHub didn’t accept the sign-in (${reply.status}).`);
    const user = await reply.json();
    return { login: String(user.login || ''), name: String(user.name || '') };
  }

  /** onCode({ code, url, expires }): show the code to type at url. Resolves with the status once GitHub agrees. */
  async function signIn(onCode) {
    if (!GITHUB.clientId) throw new AgentError('This copy of parc has no GitHub app to sign in with yet. See the desktop README, “GitHub Copilot”.');
    if (signingIn) throw new AgentError('Already signing in: enter the code on GitHub, or cancel.');
    const controller = new AbortController(), signal = controller.signal;
    signingIn = controller;
    try {
      const start = await post(GITHUB.device, { client_id: GITHUB.clientId, scope: SCOPE }, signal);
      if (start.error) throw refusal(start.error);
      if (!start.device_code || !start.user_code || !start.verification_uri) throw new AgentError('GitHub’s answer was incomplete. Try again.');
      const until = Date.now() + (Number(start.expires_in) || 900) * 1000;
      let interval = Math.max(Number(start.interval) || 5, 1);
      onCode({ code: start.user_code, url: start.verification_uri, expires: new Date(until).toISOString() });
      await open(start.verification_uri);
      for (;;) {
        // GitHub asks for at least `interval` seconds between polls; a second more keeps clear of slow_down.
        await wait((interval + 1) * 1000, signal);
        if (Date.now() > until) throw new AgentError('The code expired before it was entered. Try again.');
        const reply = await post(GITHUB.token, { client_id: GITHUB.clientId, device_code: start.device_code, grant_type: DEVICE_GRANT }, signal);
        if (reply.access_token) {
          const account = await who(reply.access_token);
          await box.keep({ token: reply.access_token, scope: String(reply.scope || ''), ...account });
          return status();
        }
        if (reply.error === 'authorization_pending') continue;
        if (reply.error === 'slow_down') { interval = Math.max(Number(reply.interval) || interval + 5, interval + 5); continue; }
        throw refusal(reply.error);
      }
    } catch (err) {
      if (signal.aborted) throw new AgentError('Sign-in cancelled.', { cancelled: true });
      throw err instanceof AgentError ? err : new AgentError(`Couldn’t sign in to GitHub: ${err.message}`);
    } finally { signingIn = null; }
  }
  function refusal(code) {
    const text = {
      access_denied: ['Sign-in was cancelled on GitHub.', { cancelled: true }],
      expired_token: ['The code expired before it was entered. Try again.'],
      device_flow_disabled: ['parc’s GitHub app doesn’t allow device sign-in. Turn on “Enable Device Flow” in its settings.'],
      incorrect_client_credentials: ['GitHub doesn’t know parc’s app. Check the client ID.'],
      unsupported_grant_type: ['GitHub refused the sign-in (unsupported_grant_type).']
    }[code];
    return text ? new AgentError(text[0], { code, ...text[1] }) : new AgentError(`GitHub refused the sign-in (${code || 'no reason given'}).`, { code });
  }
  async function token() {
    const c = await box.load();
    if (!c?.token) throw new AgentError('Sign in with GitHub first.', { signedOut: true });
    return c.token;
  }
  function status() {
    const c = box.value;
    return { signedIn: !!c?.token, login: c?.login || '', name: c?.name || '', signingIn: !!signingIn, remembered: box.remembered(), available: !!GITHUB.clientId };
  }
  return {
    status: async () => { await box.load(); return status(); },
    signIn, token,
    cancel() { signingIn?.abort(); },
    // The token can only be revoked on GitHub (Settings → Applications); parc forgets it.
    async signOut() { await box.load(); await box.keep(null); return status(); }
  };
}
function pause(ms, signal) {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) return reject(new Error('aborted'));
    const timer = setTimeout(resolve, ms);
    signal?.addEventListener('abort', () => { clearTimeout(timer); reject(new Error('aborted')); }, { once: true });
  });
}

// ── Copilot, through its SDK ──
// One runtime for the app, started when first needed with the signed-in token, and restarted when the token changes.
// A chat has one live session while the app runs; a chat continued after a restart (or after a failed question)
// starts a new session that is told what was asked and answered so far. Sessions are deleted when their chat
// leaves memory, so the runtime's own store (in parc's user data) doesn't keep conversations.
/**
 * auth: github(). sdk(): the @microsoft/copilot-sdk module. home: the runtime's data directory. work: an empty
 * directory for sessions to run in (no tool can reach it, but every session needs one).
 */
function copilotRuntime({ auth, sdk, home, work }) {
  let client = null, clientToken = null, starting = null, models = null;
  const live = new Map(); // chat id → { session, model, off }

  async function ready() {
    const token = await auth.token();
    if (client && clientToken === token) return client;
    if (starting) return starting;
    starting = (async () => {
      await stopClient();
      await fs.mkdir(work, { recursive: true, mode: 0o700 });
      const { CopilotClient, RuntimeConnection } = sdk();
      const next = new CopilotClient({
        mode: 'empty',
        runtime: RuntimeConnection.forSpawnedStdio({
          baseDirectory: home, workingDirectory: work,
          env: { PATH: process.env.PATH || '', HOME: home, COPILOT_TELEMETRY_DISABLED: 'true' }
        }),
        github: { token, useLoggedInUser: false },
        observability: { logLevel: 'error' }
      });
      await next.start();
      client = next; clientToken = token; models = null;
      return client;
    })().finally(() => { starting = null; });
    return starting;
  }
  async function stopClient() {
    for (const id of [...live.keys()]) await close(id);
    const old = client;
    client = null; clientToken = null; models = null;
    if (old) await old.stop().catch(() => {});
  }
  async function close(id) {
    const s = live.get(id);
    if (!s) return;
    live.delete(id);
    s.off?.();
    await s.session.disconnect().catch(() => {});
    await client?.deleteSession(s.session.sessionId).catch(() => {});
  }

  // The plan's models that are switched on, as { id, label }.
  async function listModels(force = false) {
    if (models && !force) return models;
    const c = await ready();
    let list;
    try { list = await c.listModels(); } catch (err) { throw failure(err); }
    models = list.filter(m => m && m.id && m.policy?.state !== 'disabled').map(m => ({ id: String(m.id), label: String(m.name || m.id) }));
    if (!models.length) throw new AgentError('GitHub Copilot lists no models for this account.');
    return models;
  }

  /**
   * One question. tools: [{ name, description, parameters, run(args, callId) → { ok, text } }]. instructions: the
   * system message. recap: what the chat said before, for a new session. emit: text deltas. signal: Stop.
   */
  async function ask(chatId, { text, model, tools, instructions, recap, emit, signal }) {
    const c = await ready();
    let s = live.get(chatId);
    if (s && s.model !== model) { await close(chatId); s = null; }
    if (!s) {
      const definitions = tools.map(t => ({
        name: `parc_${t.name}`, description: t.description, parameters: t.parameters, skipPermission: true,
        handler: async (args, invocation) => {
          const r = await t.run(args, invocation?.toolCallId || `${t.name}-${Date.now()}`);
          return r.ok ? r.text : { textResultForLlm: r.text, resultType: 'failure', error: r.text };
        }
      }));
      let session;
      try {
        session = await c.createSession({
          model,
          tools: { availableTools: ['custom:*'], definitions },
          systemMessage: { mode: 'replace', content: recap ? `${instructions}\n\n${recap}` : instructions },
          workingDirectory: work,
          // Every tool is parc's and skips this; anything else that asks is refused.
          permissions: { onRequest: () => ({ kind: 'reject', feedback: 'Only parc’s vault tools are available.' }) },
          instructions: { skipCustomInstructions: true },
          github: { telemetryEnabled: false },
          events: { streaming: true }
        });
      } catch (err) { throw failure(err); }
      s = { session, model };
      live.set(chatId, s);
    }
    const { session } = s;
    const done = new Promise((resolve, reject) => {
      const offs = [
        session.on('assistant.message_delta', e => { if (e.data?.deltaContent) emit({ type: 'text', delta: e.data.deltaContent }); }),
        session.on('session.error', e => reject(failure(e.data))),
        session.on('session.idle', () => resolve())
      ];
      s.off = () => offs.forEach(off => off?.());
    });
    done.catch(() => {}); // raced against Stop below; a late failure has no one to tell
    const stop = () => session.abort().catch(() => {});
    signal.addEventListener('abort', stop, { once: true });
    try {
      await session.send({ prompt: text });
      await Promise.race([done, new Promise((_, reject) => signal.addEventListener('abort', () => reject(new AgentError('Stopped.', { cancelled: true })), { once: true }))]);
    } catch (err) {
      // A session that failed or was stopped holds half a turn: drop it, so the next question starts clean.
      await close(chatId);
      const out = err instanceof AgentError ? err : failure(err);
      if (out.signedOut) { await stopClient(); await auth.signOut(); }
      throw out;
    } finally {
      s.off?.(); s.off = null;
      signal.removeEventListener('abort', stop);
    }
  }

  return {
    models: listModels, ask,
    forget: id => close(id),
    async stop() { await stopClient(); },
    signedOut() { return stopClient(); }
  };
}

// What Copilot's errors mean to the person asking.
function failure(err) {
  const message = String(err?.message || err || ''), status = err?.statusCode;
  if (status === 401 || /not authenticated|bad credentials|authentication failed/i.test(message)) {
    return new AgentError('GitHub no longer accepts this sign-in. Sign in again.', { signedOut: true });
  }
  if (status === 402 || status === 429 || /quota|rate limit|premium request|exceeded/i.test(message)) {
    return new AgentError(`GitHub Copilot’s limit for your plan is reached for now. ${message.slice(0, 200)}`.trim());
  }
  if (/copilot.*(not|isn’t|is not).*(enabled|access|subscri)|no copilot/i.test(message)) {
    return new AgentError('This GitHub account has no Copilot access. It needs a Copilot plan, or a seat from an organization.');
  }
  return new AgentError(`GitHub Copilot: ${message.slice(0, 300) || 'something went wrong.'}`);
}

module.exports = { GITHUB, github, copilotRuntime, failure };
