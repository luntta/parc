// A failed (or still running) restore must never replace the last usable layout,
// including when beforeunload fires. Activate writes only after startup succeeds.
export function workspacePersistence({ snapshot, saveLayout, saveDrafts, report, delay = 300 }) {
  let ready = false, timer;
  function remember(now = false, { syncDisk = false } = {}) {
    clearTimeout(timer);
    if (!ready) return true;
    const write = () => {
      try {
        saveDrafts(syncDisk);
        saveLayout(snapshot());
        return true;
      } catch (err) { report(err); return false; }
    };
    if (now) return write();
    timer = setTimeout(write, delay);
    return true;
  }
  return { remember, activate(options) { ready = true; return remember(true, options); } };
}
