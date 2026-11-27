// Keep partial successes visible: neither a skipped entry nor a failed vault is an empty inbox.
function sweepMessage(report) {
  const count = (n, one, many) => `${n} ${n === 1 ? one : many}`;
  const parts = [];
  if (report.created?.length) parts.push(`Imported ${count(report.created.length, 'document', 'documents')}.`);
  if (report.waiting?.length) parts.push(`${count(report.waiting.length, 'file is', 'files are')} still settling.`);
  if (report.skipped?.length) parts.push(`Skipped ${count(report.skipped.length, 'entry', 'entries')}.`);
  if (report.more) parts.push('More files queued for import.');
  if (report.pending) parts.push(`Preparing document text for ${count(report.pending, 'file', 'files')}.`);
  const problems = [...(report.failed || []).map(f => [f.vault, f.filename, f.error].filter(Boolean).join(': ')), ...(report.warnings || [])];
  if (problems.length) parts.push(`${count(problems.length, 'problem', 'problems')}: ${problems[0]}${problems.length > 1 ? ' (and ' + (problems.length - 1) + ' more)' : ''}`);
  return parts.join(' ') || 'Inbox folder is clear.';
}

async function sweepVaults(paths, sweep, name) {
  const reports = await Promise.all(paths.map(async path => {
    try {
      const r = await sweep(path);
      return { ...r, skipped: (r.skipped || []).map(file => name(path) + ': ' + file), failed: (r.failed || []).map(f => ({ ...f, vault: name(path) })),
        warnings: (r.warnings || []).map(w => name(path) + ': ' + w) };
    } catch (err) {
      return { failed: [{ vault: name(path), error: String(err.message ?? err) }] };
    }
  }));
  return { ...Object.fromEntries(['created', 'waiting', 'skipped', 'failed', 'warnings'].map(k => [k, reports.flatMap(r => r[k] || [])])),
    more: reports.some(r => r.more), pending: reports.reduce((n,r) => n + (r.pending || 0), 0) };
}

// Sweeping a vault's inbox/ in the main process: one sweep per vault at a time, a sweep asked for during one runs
// after it, and a vault with files still settling, queued, or waiting for their text is swept again shortly.
// sweep(vault) → the server's report; isOpen(vault); after(vault): once each sweep is done;
// tell(vault, event): an event for the workspace ({ kind: 'inbox' | 'attachments' | 'inbox-error', … }).
function inboxSweeper({ sweep, isOpen, after = () => {}, tell, retryMs = 1200 }) {
  const timers = new Map(), again = new Set(), running = new Map();
  function run(p) {
    if (!isOpen(p)) return Promise.reject(new Error('Choose an opened vault'));
    if (running.has(p)) return running.get(p);
    const job = (async () => {
      const res = await sweep(p);
      after(p);
      if (res?.created?.length) tell(p, { kind: 'inbox', created: res.created });
      if (res?.completed) tell(p, { kind: 'attachments' });
      if (res?.failed?.length || res?.warnings?.length) tell(p, { kind: 'inbox-error', message: sweepMessage(res) });
      const asked = again.delete(p);
      if (isOpen(p) && (res?.waiting?.length || res?.pending || res?.more || asked)) {
        clearTimeout(timers.get(p));
        timers.set(p, setTimeout(() => { timers.delete(p); background(p); }, res?.more ? 0 : retryMs));
      }
      return res;
    })();
    running.set(p, job);
    const done = () => running.delete(p);
    job.then(done, done);
    return job;
  }
  function background(p) {
    if (running.has(p)) { again.add(p); return; }
    run(p).catch(err => { if (isOpen(p)) tell(p, { kind: 'inbox-error', message: String(err.message ?? err) }); });
  }
  function close(p) { again.delete(p); clearTimeout(timers.get(p)); timers.delete(p); }
  return { run, background, close, closeAll() { for (const p of [...timers.keys()]) close(p); } };
}

module.exports = { sweepMessage, sweepVaults, inboxSweeper };
