const fs = require('node:fs/promises');
const { constants, existsSync } = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { execFile } = require('node:child_process');

// No shell; note contents travel over stdin, never through process arguments.
function run(file, args, input = '', timeout = 15000) {
  return new Promise((resolve, reject) => {
    const child = execFile(file, args, { timeout, killSignal: 'SIGKILL', maxBuffer: 4 * 1024 * 1024 }, (err, stdout) => {
      if (err?.code === 'ENOENT') reject(new Error(`Cannot find ${path.basename(file)}. See desktop setup instructions.`));
      else if (err) reject(new Error(`${path.basename(file)} failed${err.killed ? ' (timeout)' : ''}. ${String(err.message).split('\n').slice(-3).join(' ').trim()}`));
      else resolve(stdout);
    });
    child.stdin.on('error', () => {}); // early child exit is reported above
    child.stdin.end(input);
  });
}

function binary(name) {
  const local = path.join(__dirname, '..', 'target', 'debug', name);
  return existsSync(local) ? local : name;
}

const CONFLICT = -32010; // parc-server: the fragment changed since the client read it
function validType(type) {
  if (typeof type !== 'string' || !/^[a-z0-9][a-z0-9_-]{0,63}$/i.test(type)) throw new Error('Invalid fragment type');
  return type;
}
function validTags(tags) {
  if (!Array.isArray(tags) || tags.length > 100 || !tags.every(t => typeof t === 'string' && /^[^\s#][^\s]{0,99}$/u.test(t))) throw new Error('Invalid tags');
  return tags;
}
// Schema fields by name; null clears one. The server checks names and values against the schema.
function validFields(fields) {
  if (!fields || typeof fields !== 'object' || Array.isArray(fields)) throw new Error('Invalid fields');
  const entries = Object.entries(fields);
  const value = v => v === null || (typeof v === 'string' && v.length <= 10000) || (Array.isArray(v) && v.every(s => typeof s === 'string'));
  if (entries.length > 50 || !entries.every(([k, v]) => /^[a-z_][a-z0-9_-]{0,63}$/i.test(k) && value(v))) throw new Error('Invalid fields');
  return fields;
}

function archive(vault) {
  if (!path.isAbsolute(vault) || path.basename(vault) !== '.parc') throw new Error('Expected an absolute .parc vault path');
  async function request(method, params) {
    const line = JSON.stringify({ jsonrpc: '2.0', id: 1, method, params });
    const reply = JSON.parse(await run(binary('parc-server'), ['--vault', vault], line + '\n'));
    if (reply.error?.code === CONFLICT) return { conflict: true, updated_at: reply.error.data?.updated_at ?? null };
    if (reply.error) throw new Error(typeof reply.error.data === 'string' ? reply.error.data : reply.error.message);
    return reply.result;
  }
  function validId(id) {
    if (typeof id !== 'string' || !/^[0-9A-HJKMNP-TV-Z]{26}$/.test(id)) throw new Error('Invalid fragment ID');
    return id;
  }
  // The revision the client's copy was read at; the server refuses the write if the file has changed since.
  const expectation = revision => {
    if (revision == null) return {};
    if (typeof revision !== 'string' || !/^[0-9a-f]{16}$/.test(revision)) throw new Error('Invalid revision');
    return { expected_revision: revision };
  };
  // A history snapshot's name: an RFC 3339 time. The server re-checks it before touching a path.
  function validStamp(ts) {
    if (typeof ts !== 'string' || !/^\d{4}-\d{2}-\d{2}T[\d:.]+(?:Z|[+-]\d{2}:\d{2})$/.test(ts)) throw new Error('Invalid version');
    return ts;
  }
  return {
    async init() {
      if (!existsSync(vault)) await run(binary('parc'), ['--vault', vault, 'init']);
    },
    // Resolves to the saved fragment, or { conflict: true } when the file changed since `expected` (a revision).
    async save({ id, title, body, expected, type, fields, tags }) {
      if (typeof title !== 'string' || typeof body !== 'string' || (!title.trim() && !body.trim())) throw new Error('Enter a title or body first.');
      if (title.length + body.length > 1000000) throw new Error('Fragment is too large (one million characters maximum).');
      if (id != null) return request('fragment.update', { id: validId(id), title, body, ...expectation(expected) });
      return request('fragment.create', {
        type: validType(type ?? 'note'), title: title.trim() || body.trim().split('\n')[0].slice(0, 120), body,
        tags: validTags(tags ?? []), fields: validFields(fields ?? {})
      });
    },
    // Metadata only: { fields?, tags? }. Same conflict contract as save.
    async update(id, changes, expected) {
      if (!changes || typeof changes !== 'object') throw new Error('Invalid changes');
      const params = { id: validId(id), ...expectation(expected) };
      if (changes.fields !== undefined) params.fields = validFields(changes.fields);
      if (changes.tags !== undefined) params.tags = validTags(changes.tags);
      return request('fragment.update', params);
    },
    schemas: () => request('schema.list', {}),
    links: () => request('vault.links', {}),
    list: () => request('fragment.search', { query: 'is:all' }),
    search(query) {
      if (typeof query !== 'string' || query.length > 2000) throw new Error('Invalid search query');
      return request('fragment.search', { query });
    },
    get: id => request('fragment.get', { id: validId(id) }),
    async archive(id, archived, expected) {
      if (typeof archived !== 'boolean') throw new Error('Invalid archive flag');
      return request('fragment.archive', { id: validId(id), archived, ...expectation(expected) });
    },
    remove: async id => request('fragment.delete', { id: validId(id) }),     // to the vault's trash
    untrash: async id => request('trash.restore', { id: validId(id) }),
    history: async id => request('history.list', { id: validId(id) }),
    version: async (id, timestamp) => request('history.get', { id: validId(id), timestamp: validStamp(timestamp) }),
    restoreVersion: async (id, timestamp) => request('history.restore', { id: validId(id), timestamp: validStamp(timestamp) })
  };
}

// Settings chosen in the app win over the environment variables, which still work for terminal launches.
async function speechConfig(env = process.env, settings = {}) {
  const model = settings.whisperModel || env.PARC_WHISPER_MODEL;
  if (!model || !path.isAbsolute(model) || !(await fs.stat(model).catch(() => null))?.isFile()) {
    throw new Error('Choose a local whisper.cpp speech model to enable dictation.');
  }
  const command = settings.whisperBin || env.PARC_WHISPER_BIN || 'whisper-cli';
  for (const file of [command, 'ffmpeg']) {
    const options = path.isAbsolute(file) ? [file] : (env.PATH || '').split(path.delimiter).map(dir => path.join(dir, file));
    let found = false;
    for (const option of options) {
      try { await fs.access(option, constants.X_OK); found = true; break; } catch { /* next directory */ }
    }
    if (!found) throw new Error(file === 'ffmpeg' ? 'Install ffmpeg to enable dictation, then restart parc Capture.' : `Can’t find ${path.basename(file)}: install it, or choose where it is.`);
  }
  return { model, command };
}

async function transcribe(audio, config, execute = run) {
  if (!(audio instanceof Uint8Array) || !audio.byteLength || audio.byteLength > 4 * 1024 * 1024) throw new Error('Recording is empty or too large. Record up to two minutes.');
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'parc-speech-'));
  const clip = path.join(dir, 'clip.webm'), wav = path.join(dir, 'clip.wav'), output = path.join(dir, 'words');
  try {
    await fs.writeFile(clip, audio, { mode: 0o600 });
    await execute('ffmpeg', ['-nostdin', '-loglevel', 'error', '-protocol_whitelist', 'file', '-f', 'matroska', '-i', clip, '-t', '120', '-vn', '-ar', '16000', '-ac', '1', '-c:a', 'pcm_s16le', wav]);
    await execute(config.command, ['-m', config.model, '-f', wav, '-otxt', '-of', output], '', 180000);
    const text = (await fs.readFile(output + '.txt', 'utf8')).trim();
    if (!text) throw new Error('No speech detected. Try recording again.');
    return text;
  } finally { await fs.rm(dir, { recursive: true, force: true }); }
}

module.exports = { archive, speechConfig, transcribe };
