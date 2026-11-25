const fs = require('node:fs/promises');
const { constants, existsSync } = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { execFile } = require('node:child_process');
const { LANGUAGES } = require('./languages.cjs');
const { englishOnly } = require('./models.cjs');

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

// Vault settings. The server checks meaning (names that exist, values a field has); this checks shape and size.
const CATEGORIES = ['open', 'active', 'done', 'closed'], KINDS = ['string', 'date', 'enum', 'list'];
const word = (v, max = 64) => typeof v === 'string' && v.length > 0 && v.length <= max && !/[\s\0]/.test(v);
function validVaultSettings(changes) {
  const types = changes?.types;
  if (!changes || typeof changes !== 'object' || Object.keys(changes).some(k => k !== 'types') || !types || typeof types !== 'object') throw new Error('Invalid vault settings');
  const out = {};
  if (types.default !== undefined) out.default = validType(types.default);
  if (types.disabled !== undefined) {
    if (!Array.isArray(types.disabled) || types.disabled.length > 256) throw new Error('Invalid vault settings');
    out.disabled = types.disabled.map(validType);
  }
  return { types: out };
}
function validSchema(schema) {
  const ok = schema && typeof schema === 'object' && !Array.isArray(schema) && Array.isArray(schema.fields) && schema.fields.length <= 32
    && (schema.alias == null || (typeof schema.alias === 'string' && schema.alias.length <= 16))
    && (schema.editor_skip === undefined || typeof schema.editor_skip === 'boolean');
  if (!ok) throw new Error('Invalid type definition');
  const field = f => {
    const values = f.values ?? null, categories = f.categories ?? null, toggle = f.toggle ?? null;
    const fine = f && typeof f === 'object' && word(f.name) && KINDS.includes(f.kind) && typeof (f.required ?? false) === 'boolean'
      && (values === null || (Array.isArray(values) && values.length <= 32 && values.every(v => word(v))))
      && (f.default == null || (typeof f.default === 'string' && f.default.length <= 200))
      && (toggle === null || (Array.isArray(toggle) && toggle.length === 2 && toggle.every(v => word(v))))
      && (categories === null || (typeof categories === 'object' && !Array.isArray(categories) && Object.keys(categories).length <= 32
        && Object.entries(categories).every(([k, c]) => word(k) && CATEGORIES.includes(c))));
    if (!fine) throw new Error(`Invalid field${typeof f?.name === 'string' ? ` “${f.name.slice(0, 64)}”` : ''}`);
    return { name: f.name, kind: f.kind, values, required: !!f.required, default: f.default ?? null, toggle, categories };
  };
  return { name: validType(schema.name), alias: schema.alias || null, editor_skip: !!schema.editor_skip, fields: schema.fields.map(field) };
}
function validRenames(renames) {
  if (!Array.isArray(renames) || renames.length > 256 || !renames.every(r => r && word(r.field) && word(r.from) && word(r.to))) throw new Error('Invalid value changes');
  return renames.map(({ field, from, to }) => ({ field, from, to }));
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
      if (!existsSync(vault)) await run(binary('parc'), ['--vault', vault, 'init', '--yes']);
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
    // Vault settings, kept in the vault for everyone who uses it: which types are on, and what each type holds.
    vaultSettings: () => request('settings.get', {}),
    updateVaultSettings: async changes => request('settings.update', validVaultSettings(changes)),
    schemaUsage: async type => request('schema.usage', { type: validType(type) }),
    builtinSchema: async type => request('schema.builtin', { type: validType(type) }),
    // A new type (create) or a changed one; renames move fragments off renamed and removed values.
    saveSchema: async ({ schema, create = false, renames = [] } = {}) => request('schema.save', { schema: validSchema(schema), create: !!create, renames: validRenames(renames) }),
    deleteSchema: async type => request('schema.delete', { type: validType(type) }),
    links: () => request('vault.links', {}),
    list: () => request('fragment.search', { query: 'is:all' }),
    search(query) {
      if (typeof query !== 'string' || query.length > 2000) throw new Error('Invalid search query');
      return request('fragment.search', { query });
    },
    // Lines of fragment bodies containing the text (literal, smart case): Go to…'s / mode.
    async grep(pattern) {
      if (typeof pattern !== 'string' || pattern.length > 1000) throw new Error('Invalid search text');
      return request('fragment.grep', { pattern, limit: 300 });
    },
    // Fragments related to a saved fragment (id) or to unsaved text (text and tags, with the id of the fragment
    // it is a draft of, if any): what the Related panel shows as you write.
    async recall({ id = null, text = null, tags = [], limit = 5 } = {}) {
      if (id == null && text == null) throw new Error('Nothing to recall from');
      if (text != null && (typeof text !== 'string' || text.length > 1000000)) throw new Error('Invalid draft text');
      if (!Number.isInteger(limit) || limit < 1 || limit > 20) throw new Error('Invalid limit');
      const params = { limit };
      if (id != null) params.id = validId(id);
      if (text != null) { params.text = text; params.tags = validTags(tags ?? []); }
      return request('recall.context', params);
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

/**
 * The language to dictate in: the only preferred language; with several, the one chosen next to Dictate
 * (which may be 'auto'), else the first; 'auto' (whisper detects it) when none are set.
 */
function speechLanguage(settings = {}) {
  const preferred = (settings.speechLanguages || []).filter(code => LANGUAGES.includes(code));
  if (preferred.length === 1) return preferred[0];
  if (preferred.length && (settings.speechLanguage === 'auto' || preferred.includes(settings.speechLanguage))) return settings.speechLanguage;
  return preferred[0] || 'auto';
}

// Settings chosen in the app win over the environment variables, which still work for terminal launches.
async function speechConfig(env = process.env, settings = {}) {
  const model = settings.whisperModel || env.PARC_WHISPER_MODEL;
  if (!model || !path.isAbsolute(model) || !(await fs.stat(model).catch(() => null))?.isFile()) {
    throw new Error('Choose or download a speech model to enable dictation.');
  }
  const command = settings.whisperBin || env.PARC_WHISPER_BIN || 'whisper-cli';
  const options = path.isAbsolute(command) ? [command] : (env.PATH || '').split(path.delimiter).map(dir => path.join(dir, command));
  let found = false;
  for (const option of options) {
    try { await fs.access(option, constants.X_OK); found = true; break; } catch { /* next directory */ }
  }
  if (!found) throw new Error(`Can’t find ${path.basename(command)}: install whisper.cpp, or choose where whisper-cli is.`);
  const language = speechLanguage(settings);
  // An English-only model ignores the language option and writes English.
  const others = (settings.speechLanguages || []).filter(code => code !== 'en');
  const warning = englishOnly(model) && (others.length || !['en', 'auto'].includes(language))
    ? 'This model only understands English. Choose a multilingual model to dictate in other languages.' : null;
  return { model, command, language, warning };
}

const WAV_HEADER = 44, MAX_WAV = WAV_HEADER + 16000 * 2 * 120; // two minutes of 16 kHz mono 16-bit audio
/** Accept only the WAV the renderer writes: PCM, 16-bit, mono, 16 kHz, header and length consistent. */
function checkWav(audio) {
  if (!(audio instanceof Uint8Array) || audio.byteLength <= WAV_HEADER || audio.byteLength > MAX_WAV) {
    throw new Error('Recording is empty or too long. Record up to two minutes.');
  }
  const view = new DataView(audio.buffer, audio.byteOffset, audio.byteLength);
  const tag = offset => String.fromCharCode(...audio.subarray(offset, offset + 4));
  const valid = tag(0) === 'RIFF' && view.getUint32(4, true) === audio.byteLength - 8 && tag(8) === 'WAVE'
    && tag(12) === 'fmt ' && view.getUint32(16, true) === 16 && view.getUint16(20, true) === 1 && view.getUint16(22, true) === 1
    && view.getUint32(24, true) === 16000 && view.getUint16(34, true) === 16
    && tag(36) === 'data' && view.getUint32(40, true) === audio.byteLength - WAV_HEADER;
  if (!valid) throw new Error('The recording couldn’t be read. Try recording again.');
  return audio;
}

async function transcribe(audio, config, execute = run) {
  checkWav(audio);
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'parc-speech-'));
  const wav = path.join(dir, 'clip.wav'), output = path.join(dir, 'words');
  try {
    await fs.writeFile(wav, audio, { mode: 0o600 });
    await execute(config.command, ['-m', config.model, '-l', config.language || 'auto', '-f', wav, '-otxt', '-of', output], '', 180000);
    const text = (await fs.readFile(output + '.txt', 'utf8')).trim();
    if (!text) throw new Error('No speech detected. Try recording again.');
    return text;
  } finally { await fs.rm(dir, { recursive: true, force: true }); }
}

module.exports = { archive, binary, speechConfig, speechLanguage, checkWav, transcribe };
