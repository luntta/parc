const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'parc-ui-test-'));
try {
  // No meaning model, whatever this computer has downloaded: recall is by words, tags and links, as the test expects.
  const env = { ...process.env, PARC_MODELS_DIR: path.join(dir, 'models') };
  const child = spawnSync(require('electron'), [path.join(__dirname, 'smoke.cjs'), dir], { stdio: 'inherit', timeout: 45000, env });
  if (child.error) throw child.error;
  process.exitCode = child.status ?? 1;
} finally { fs.rmSync(dir, { recursive: true, force: true, maxRetries: 3 }); }
