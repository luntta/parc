// A stand-in for auth.openai.com and api.openai.com, for the agent's tests and the UI smoke test: it records what it
// is sent and answers from a script. Points agent.cjs's ENDPOINTS at itself.
const http = require('node:http');
const crypto = require('node:crypto');
const agent = require('../agent.cjs');

const jwt = claims => `e30.${Buffer.from(JSON.stringify(claims)).toString('base64url')}.sig`;
const sse = events => events.map(e => `event: ${e.type}\ndata: ${JSON.stringify(e)}\n\n`).join('');

// A stand-in for auth.openai.com and api.openai.com: records what it is sent, answers from a script.
function fakeOpenAI() {
  const seen = { authorize: [], token: [], responses: [], models: 0 };
  const fake = { seen, respond: [], refreshError: null, access: 1, scope: 'openid profile email offline_access resource.invoke chatgpt.tokens.use.direct' };
  let challenge = null;
  const server = http.createServer(async (req, res) => {
    const url = new URL(req.url, 'http://x');
    let body = '';
    for await (const chunk of req) body += chunk;
    const json = (status, value) => { res.writeHead(status, { 'content-type': 'application/json' }); res.end(JSON.stringify(value)); };
    if (url.pathname === '/authorize') {
      const q = Object.fromEntries(url.searchParams);
      seen.authorize.push(q);
      challenge = q.code_challenge;
      const back = new URL(q.redirect_uri);
      back.search = new URLSearchParams({ code: 'the-code', state: q.state, ...(q.client_id === 'dynamic_agent_client' ? { client_id: 'oaiapp_test' } : {}) });
      res.writeHead(302, { location: back.toString() }); return res.end();
    }
    if (url.pathname === '/token') {
      const form = Object.fromEntries(new URLSearchParams(body));
      seen.token.push(form);
      if (form.grant_type === 'authorization_code') {
        const ok = form.code === 'the-code' && form.client_id === 'oaiapp_test' && crypto.createHash('sha256').update(form.code_verifier).digest('base64url') === challenge;
        if (!ok) return json(400, { error: 'invalid_grant' });
        return json(200, { access_token: `at-${fake.access}`, refresh_token: 'rt-1', id_token: jwt({ email: 'reader@example.com', name: 'Reader' }), expires_in: 3600, scope: fake.scope, token_type: 'Bearer' });
      }
      if (fake.refreshError) return json(400, { error: fake.refreshError });
      return json(200, { access_token: `at-${++fake.access}`, refresh_token: `rt-${fake.access}`, expires_in: 3600, scope: fake.scope });
    }
    if (req.headers.authorization !== `Bearer at-${fake.access}`) return json(401, { error: { code: 'invalid_token', message: 'no' } });
    if (url.pathname === '/v1/models') {
      seen.models++;
      return json(200, { data: [{ slug: 'gpt-test', display_name: 'GPT Test', visibility: 'list' }, { slug: 'gpt-hidden', visibility: 'hide' }, { slug: 'gpt-other', display_name: 'GPT Other', visibility: 'list' }] });
    }
    if (url.pathname === '/v1/responses') {
      const request = JSON.parse(body);
      seen.responses.push(request);
      const next = fake.respond.shift();
      if (!next) return json(500, { error: { message: 'nothing scripted' } });
      if (next.status) return json(next.status, next.body);
      if (next.hang) { res.writeHead(200, { 'content-type': 'text/event-stream' }); res.write(': thinking\n\n'); return; }
      res.writeHead(200, { 'content-type': 'text/event-stream' });
      return res.end(sse(next(request)));
    }
    json(404, {});
  });
  return new Promise(resolve => server.listen(0, '127.0.0.1', () => {
    const base = `http://127.0.0.1:${server.address().port}`;
    Object.assign(agent.ENDPOINTS, { authorize: `${base}/authorize`, token: `${base}/token`, api: `${base}/v1`, port: 0 });
    resolve({ fake, close: () => { server.close(); server.closeAllConnections(); } });
  }));
}
// The browser: follows the sign-in page's redirect to parc's callback.
async function browser(url) {
  const first = await fetch(url, { redirect: 'manual' });
  await fetch(first.headers.get('location'));
}
const message = text => ({ type: 'message', id: 'msg_1', role: 'assistant', status: 'completed', content: [{ type: 'output_text', text, annotations: [] }] });
const completed = output => ({ type: 'response.completed', response: { id: 'resp_1', status: 'completed', output } });

module.exports = { fakeOpenAI, browser, message, completed };
