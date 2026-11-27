// Where you were reading each source document (page, slide or section): kept per computer, never inside a vault.
const fs = require('node:fs/promises');
const { writeJson } = require('./files.cjs');
const { ULID_RE, safeFile } = require('./attachment-file.cjs');

const UNITS = ['page', 'slide', 'section'];
const key = (vault, id) => JSON.stringify([vault, id]);

function positionStore(file) {
  let cache = null;
  async function all() {
    if (cache) return cache;
    try {
      const parsed = JSON.parse(await fs.readFile(file, 'utf8'));
      cache = parsed && typeof parsed === 'object' && !Array.isArray(parsed) ? parsed : {};
    } catch { cache = {}; }
    return cache;
  }
  return {
    // Positions in the vaults `open(vault)` says are open, keyed by JSON [vault, id].
    async list(open) {
      const out = {};
      for (const [k, v] of Object.entries(await all())) {
        try { const [vault] = JSON.parse(k); if (open(vault)) out[k] = v; } catch { /* not a key of ours */ }
      }
      return out;
    },
    async set(vault, id, pos) {
      if (typeof vault !== 'string' || !ULID_RE.test(id)) throw new Error('Invalid reading position target');
      if (!pos || typeof pos !== 'object') throw new Error('Invalid reading position');
      const part = Number(pos.part);
      const total = pos.total != null ? Number(pos.total) : null;
      if (!Number.isInteger(part) || part < 1 || part > 100000) throw new Error('Invalid page number');
      if (total != null && (!Number.isInteger(total) || total < 1 || total > 100000)) throw new Error('Invalid page count');
      const entry = {
        part, total, unit: UNITS.includes(pos.unit) ? pos.unit : 'page',
        filename: pos.filename && safeFile(pos.filename) ? pos.filename : null, updated: new Date().toISOString()
      };
      const positions = await all();
      positions[key(vault, id)] = entry;
      // Every write is of the whole map as it is now, in order, so a failed one is made good by the next.
      await writeJson(file, positions);
      return entry;
    }
  };
}

module.exports = { positionStore, key };
