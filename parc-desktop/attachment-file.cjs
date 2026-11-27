// Finding an attachment's file on disk for the main process (the image scheme, the PDF viewer, Open). A vault
// arrives by git, which carries symlinks, so neither attachments/ nor the fragment's folder nor the file may be one:
// otherwise a cloned vault could point parc at any file on this computer. parc-core refuses the same.
const fs = require('node:fs/promises');
const path = require('node:path');

const ULID_RE = /^[0-9A-HJKMNP-TV-Z]{26}$/;
const safeFile = name => typeof name === 'string' && name.length > 0 && name.length <= 255 && name !== '.' && name !== '..'
  && name !== '.text' && !/[/\\\0]/.test(name);

// { file, size } for a regular file at <vault>/attachments/<id>/<filename>, else null.
async function attachmentFile(vaultPath, id, filename) {
  if (typeof vaultPath !== 'string' || !path.isAbsolute(vaultPath) || !ULID_RE.test(id) || !safeFile(filename)) return null;
  const root = path.join(vaultPath, 'attachments'), dir = path.join(root, id), file = path.join(dir, filename);
  try {
    for (const folder of [root, dir]) {
      const stat = await fs.lstat(folder);
      if (stat.isSymbolicLink() || !stat.isDirectory()) return null;
    }
    const stat = await fs.lstat(file);
    return stat.isFile() ? { file, size: stat.size } : null;
  } catch { return null; }
}

module.exports = { attachmentFile, safeFile, ULID_RE };
