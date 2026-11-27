// Saves from the CLI, the TUI, another window or a git pull land in fragments/, schemas/ or config.yml; this tells
// the workspace which vault changed so it can refresh its list and notice when an open fragment moved on under it.
// Writes made by the main workspace itself record the resulting file's SHA-256 hash, so the watcher's echo of them is
// suppressed without ever missing an outside edit (even one made in the same millisecond).
const { watch: fsWatch } = require('node:fs');
const fs = require('node:fs/promises');
const path = require('node:path');
const { createHash } = require('node:crypto');

const OWN_FOR = 5000;      // how long a recorded write may still explain an event
const SETTLE = 150;        // events closer together than this are one change
const INBOX_SETTLE = 350;  // a file being copied into inbox/ fires many events
const ignored = name => name && (name.startsWith('.') || name.endsWith('.tmp'));
const partial = name => name && (name.startsWith('.') || /\.(part|crdownload|tmp)$/i.test(name));

/**
 * changed(vault, kind): kind 'fragments' or 'schemas' (schemas/ or config.yml), not caused by recorded own writes.
 * inbox(vault): something landed in the vault's inbox/.
 */
function vaultWatcher({ changed, inbox, watch = fsWatch }) {
  const watchers = new Map();  // vault → { close(), ensureInbox() }
  const ownWrites = new Map(); // vault → Map<relPath, { hash: string | null, at: number }>

  async function fileHash(file) {
    try { return createHash('sha256').update(await fs.readFile(file)).digest('hex'); }
    catch (err) { return err.code === 'ENOENT' ? null : undefined; }
  }
  async function recordOwn(vault, rel) {
    const hash = await fileHash(path.join(vault, rel));
    if (hash === undefined) return;
    if (!ownWrites.has(vault)) ownWrites.set(vault, new Map());
    ownWrites.get(vault).set(rel, { hash, at: Date.now() });
  }
  // True when every file in the batch is as this app's own last write of it left it.
  async function isOnlyOwn(vault, batch) {
    if (!batch.size || batch.has(null)) return false;
    const known = ownWrites.get(vault);
    if (!known?.size) return false;
    const now = Date.now();
    for (const [k, v] of known) if (now - v.at > OWN_FOR) known.delete(k);
    for (const rel of batch) {
      const entry = known.get(rel);
      if (!entry) return false;
      if (await fileHash(path.join(vault, rel)) !== entry.hash) { known.delete(rel); return false; }
    }
    return true;
  }

  function start(p) {
    if (watchers.has(p)) return;
    const handles = [];
    let fragTimer, schemaTimer, inboxTimer, cw = null, iw = null, closed = false;
    const pendingFrags = new Set(), pendingSchemas = new Set();
    const notify = (kind, pending) => {
      const batch = new Set(pending);
      pending.clear();
      isOnlyOwn(p, batch).then(own => { if (!own && !closed) changed(p, kind); }, () => { if (!closed) changed(p, kind); });
    };
    const fragmentEvent = rel => { pendingFrags.add(rel); clearTimeout(fragTimer); fragTimer = setTimeout(() => notify('fragments', pendingFrags), SETTLE); };
    const schemaEvent = rel => { pendingSchemas.add(rel); clearTimeout(schemaTimer); schemaTimer = setTimeout(() => notify('schemas', pendingSchemas), SETTLE); };
    try {
      const fw = watch(path.join(p, 'fragments'), (_event, name) => { if (!ignored(name)) fragmentEvent(name ? `fragments/${name}` : null); });
      fw.on('error', () => close());
      handles.push(fw);
    } catch { return; } // no fragments/ yet: the next request to this vault tries again
    try {
      const sw = watch(path.join(p, 'schemas'), (_event, name) => { if (!ignored(name)) schemaEvent(name ? `schemas/${name}` : null); });
      sw.on('error', () => sw.close());
      handles.push(sw);
    } catch { /* optional */ }
    // config.yml is replaced by a rename on save, which ends a watch on the file itself: watch the new one.
    function watchConfig() {
      if (closed) return;
      try {
        cw = watch(path.join(p, 'config.yml'), event => {
          schemaEvent('config.yml');
          if (event === 'rename') { cw?.close(); cw = null; setTimeout(watchConfig, 50); }
        });
        cw.on('error', () => { cw?.close(); cw = null; });
      } catch { /* optional */ }
    }
    function ensureInbox() {
      if (closed || iw) return;
      try {
        iw = watch(path.join(p, 'inbox'), (_event, name) => {
          if (partial(name)) return;
          clearTimeout(inboxTimer);
          inboxTimer = setTimeout(() => { if (!closed) inbox(p); }, INBOX_SETTLE);
        });
        iw.on('error', () => { try { iw?.close(); } catch { /* ignore */ } iw = null; });
      } catch { /* inbox/ may not exist yet on older vaults */ }
    }
    function close() {
      closed = true;
      clearTimeout(fragTimer); clearTimeout(schemaTimer); clearTimeout(inboxTimer);
      for (const h of [...handles, cw, iw]) { try { h?.close(); } catch { /* ignore */ } }
      watchers.delete(p);
    }
    watchConfig();
    ensureInbox();
    watchers.set(p, { close, ensureInbox });
  }

  return {
    watch: start,
    watching: p => watchers.has(p),
    ensureInbox: p => watchers.get(p)?.ensureInbox(),
    recordOwn,
    close(p) { watchers.get(p)?.close(); ownWrites.delete(p); },
    closeAll() { for (const w of [...watchers.values()]) w.close(); }
  };
}

module.exports = { vaultWatcher, OWN_FOR };
