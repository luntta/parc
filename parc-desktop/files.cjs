// Writing a file in one step: a temporary file beside it, then a rename over it, so a reader (or a crash) never
// sees half a file. Each write has a temporary file of its own, and writes to one file land in the order they were
// asked for, so two saves at once can neither collide on a shared temporary name nor leave the older one on disk.
const fs = require('node:fs/promises');
const fss = require('node:fs');
const { randomBytes } = require('node:crypto');

const TMP = /\.tmp$/;
const temp = file => `${file}.${process.pid}.${randomBytes(6).toString('hex')}.tmp`;
const queues = new Map(); // file → the last write asked for

function writeAtomic(file, data, { mode = 0o600 } = {}) {
  const write = async () => {
    const tmp = temp(file);
    try {
      await fs.writeFile(tmp, data, { mode });
      await fs.rename(tmp, file);
    } catch (err) {
      await fs.rm(tmp, { force: true }).catch(() => {});
      throw err;
    }
  };
  // A failed write doesn't stop the next one: each caller sees only its own outcome.
  const run = (queues.get(file) || Promise.resolve()).catch(() => {}).then(write);
  queues.set(file, run);
  const done = () => { if (queues.get(file) === run) queues.delete(file); };
  run.then(done, done);
  return run;
}

// The same, at once, for beforeunload, where nothing asynchronous gets to finish.
function writeAtomicSync(file, data, { mode = 0o600 } = {}) {
  const tmp = temp(file);
  try {
    fss.writeFileSync(tmp, data, { mode });
    fss.renameSync(tmp, file);
  } catch (err) {
    try { fss.rmSync(tmp, { force: true }); } catch { /* already gone */ }
    throw err;
  }
}

const writeJson = (file, value, { pretty = false } = {}) => writeAtomic(file, JSON.stringify(value, null, pretty ? 2 : undefined));

// A temporary file left by a write that was interrupted (a crash, a power cut): safe to remove once it is old
// enough that no write in progress can still own it.
async function isStaleTemp(file, { age = 60_000 } = {}) {
  if (!TMP.test(file)) return false;
  const stat = await fs.stat(file).catch(() => null);
  return !!stat && Date.now() - stat.mtimeMs > age;
}

module.exports = { writeAtomic, writeAtomicSync, writeJson, isStaleTemp };
