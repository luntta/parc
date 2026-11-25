// Stand-ins for github.com's device flow and for the Copilot SDK, for the Copilot tests and the UI smoke test.
// fakeGitHub() points copilot.cjs's GITHUB at itself; fakeSdk() is a module with CopilotClient and RuntimeConnection
// whose sessions run a script: each send() takes the next step, which can call parc's tools and stream an answer.
const http = require('node:http');
const copilot = require('../copilot.cjs');

function fakeGitHub({ clientId = 'Ov23liparctest00000' } = {}) {
  const seen = { device: [], token: [], user: 0, opened: [] };
  const fake = { seen, polls: ['authorization_pending', 'slow_down'], error: null, interval: 1, login: 'octo' };
  const server = http.createServer(async (req, res) => {
    const url = new URL(req.url, 'http://x');
    let body = '';
    for await (const chunk of req) body += chunk;
    const json = (status, value) => { res.writeHead(status, { 'content-type': 'application/json' }); res.end(JSON.stringify(value)); };
    if (req.method === 'POST' && url.pathname === '/login/device/code') {
      seen.device.push(JSON.parse(body));
      return json(200, { device_code: 'dev-1', user_code: 'ABCD-1234', verification_uri: `${base}/login/device`, expires_in: 900, interval: fake.interval });
    }
    if (req.method === 'POST' && url.pathname === '/login/oauth/access_token') {
      seen.token.push(JSON.parse(body));
      if (fake.error) return json(200, { error: fake.error });
      if (fake.polls.length) return json(200, { error: fake.polls.shift(), ...(seen.token.length > 1 ? { interval: fake.interval } : {}) });
      return json(200, { access_token: 'gho_parctest', token_type: 'bearer', scope: 'read:user' });
    }
    if (url.pathname === '/user') {
      seen.user++;
      if (req.headers.authorization !== 'Bearer gho_parctest') return json(401, { message: 'Bad credentials' });
      return json(200, { login: fake.login, name: 'Octo Cat' });
    }
    json(404, {});
  });
  let base = '';
  return new Promise(resolve => server.listen(0, '127.0.0.1', () => {
    base = `http://127.0.0.1:${server.address().port}`;
    Object.assign(copilot.GITHUB, { clientId, device: `${base}/login/device/code`, token: `${base}/login/oauth/access_token`, api: base });
    resolve({ fake, base, close: () => { server.close(); server.closeAllConnections(); } });
  }));
}

function fakeSdk() {
  const seen = { clients: [], sessions: [], deleted: [], prompts: [], stopped: 0 };
  const fake = { seen, script: [], authFails: false };
  let n = 0;
  class CopilotSession {
    constructor(options) { this.options = options; this.sessionId = `session-${++n}`; this.handlers = new Map(); this.aborted = false; }
    on(type, handler) {
      if (!this.handlers.has(type)) this.handlers.set(type, new Set());
      this.handlers.get(type).add(handler);
      return () => this.handlers.get(type)?.delete(handler);
    }
    emit(type, data = {}) { for (const h of [...(this.handlers.get(type) || [])]) h({ type, data }); }
    async send({ prompt }) {
      seen.prompts.push({ session: this.sessionId, prompt });
      const step = fake.script.shift();
      setTimeout(() => Promise.resolve().then(() => { if (!step) throw new Error('nothing scripted'); return step(this, prompt); })
        .catch(err => this.emit('session.error', { errorType: 'error', message: err.message, statusCode: err.statusCode })), 0);
      return `message-${n}`;
    }
    /** What Copilot does when the model calls a tool: runs the handler parc registered. */
    async tool(name, args, id = `call-${name}-${Math.random().toString(36).slice(2, 6)}`) {
      const tool = this.options.tools.definitions.find(t => t.name === name);
      if (!tool) throw new Error(`no tool ${name}`);
      return tool.handler(args, { sessionId: this.sessionId, toolCallId: id, toolName: name, arguments: args });
    }
    say(text) { for (const part of text.match(/.{1,12}/gs)) this.emit('assistant.message_delta', { deltaContent: part, messageId: 'm' }); }
    async abort() { this.aborted = true; }
    async disconnect() { this.disconnected = true; }
  }
  class CopilotClient {
    constructor(options) { this.options = options; seen.clients.push(this); }
    async start() {}
    async stop() { seen.stopped++; return []; }
    async listModels() {
      if (fake.authFails) throw new Error('Request models.list failed with message: Not authenticated. Please authenticate first.');
      return [{ id: 'claude-sonnet-x', name: 'Claude Sonnet X', policy: { state: 'enabled' } }, { id: 'disabled-model', name: 'Off', policy: { state: 'disabled' } }, { id: 'gpt-y', name: 'GPT Y' }];
    }
    async createSession(options) { const s = new CopilotSession(options); seen.sessions.push(s); return s; }
    async deleteSession(id) { seen.deleted.push(id); }
  }
  const RuntimeConnection = { forSpawnedStdio: options => ({ kind: 'spawned-stdio', ...options }) };
  return { fake, module: { CopilotClient, RuntimeConnection } };
}

module.exports = { fakeGitHub, fakeSdk };
