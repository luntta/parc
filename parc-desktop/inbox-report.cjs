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

module.exports = { sweepMessage, sweepVaults };
