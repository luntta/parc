const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'parc-ui-test-'));
try {
  const child = spawnSync(require('electron'), [path.join(__dirname, 'smoke.cjs'), dir], { stdio: 'inherit', timeout: 45000 });
  if (child.error) throw child.error;
  process.exitCode = child.status ?? 1;
} finally { fs.rmSync(dir, { recursive: true, force: true, maxRetries: 3 }); }
