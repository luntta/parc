// A stand-in for parc-server's stdio loop: one request at a time, answered in order. `vault.reindex` takes
// FAKE_SLOW_MS, `hang` never answers, `vault.background` reports FAKE_LEFT (one less each time, to 0, unless FAKE_STUCK).
const readline = require('node:readline');
let left = Number(process.env.FAKE_LEFT || 0);
const queue = [];
let busy = false;
const reply = (id, result) => process.stdout.write(`${JSON.stringify({ jsonrpc: '2.0', id, result })}\n`);
async function next() {
  if (busy || !queue.length) return;
  busy = true;
  const { id, method } = queue.shift();
  if (method === 'hang') return; // stays busy: every later request waits behind it
  if (method === 'vault.reindex') await new Promise(r => setTimeout(r, Number(process.env.FAKE_SLOW_MS || 300)));
  if (method === 'vault.background') { reply(id, { extract: 0, meaning: left }); if (!process.env.FAKE_STUCK) left = Math.max(0, left - 1); }
  else if (method === 'big') reply(id, { text: 'x'.repeat(2_000_000) });
  else reply(id, { method, pid: process.pid });
  busy = false;
  next();
}
readline.createInterface({ input: process.stdin }).on('line', line => { queue.push(JSON.parse(line)); next(); });
