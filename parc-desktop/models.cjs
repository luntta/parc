// Speech models parc Capture can download, and the download itself. Downloads happen only when the user asks for one,
// only from these pinned URLs, and a file is kept only if its SHA-256 matches. Main process only.
const crypto = require('node:crypto');
const fs = require('node:fs/promises');
const { createReadStream } = require('node:fs');
const path = require('node:path');

const SOURCE = 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/';
const MODELS = [
  { id: 'base.en', file: 'ggml-base.en.bin', size: 147964211, sha256: 'a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002',
    english: true, label: 'Base, English', note: 'Fast. English only.' },
  { id: 'small.en', file: 'ggml-small.en.bin', size: 487614201, sha256: 'c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d',
    english: true, label: 'Small, English', note: 'More accurate. English only.' },
  { id: 'base', file: 'ggml-base.bin', size: 147951465, sha256: '60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe',
    english: false, label: 'Base, multilingual', note: 'Fast, but rough outside English.' },
  { id: 'small', file: 'ggml-small.bin', size: 487601967, sha256: '1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b',
    english: false, label: 'Small, multilingual', note: 'A balance of speed and accuracy.' },
  { id: 'large-v3-turbo-q5_0', file: 'ggml-large-v3-turbo-q5_0.bin', size: 574041195, sha256: '394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2',
    english: false, label: 'Large v3 Turbo, multilingual', note: 'The most accurate. Slower on computers without a GPU.' }
];

/** The model to suggest for these language codes: English-only models are faster and better at English. */
function recommended(languages = []) {
  return languages.length && languages.some(l => l !== 'en') ? 'large-v3-turbo-q5_0' : 'small.en';
}

/** Whether a model file only understands English: a catalog English model, or named like ggml-base.en.bin. */
function englishOnly(file) {
  const name = path.basename(file || '');
  const known = MODELS.find(m => m.file === name);
  return known ? known.english : /\.en([.\-_]|$)/i.test(name);
}

async function installed(model, dir) {
  const stat = await fs.stat(path.join(dir, model.file)).catch(() => null);
  return !!stat?.isFile() && stat.size === model.size;
}

/**
 * Download `model` ({ file, size, sha256 }, and `url` when it isn't a whisper.cpp model) into `dir`, resuming a previous
 * partial download when the server allows it.
 * fetch: (url, init) => Response, e.g. Electron's net.fetch. onProgress(received, size). Resolves to the file's path.
 * An abort leaves the partial file for the next attempt; a file whose hash doesn't match is deleted.
 */
async function download(model, { dir, fetch, signal, onProgress = () => {} }) {
  const url = model.url || SOURCE + model.file;
  await fs.mkdir(dir, { recursive: true, mode: 0o700 });
  const target = path.join(dir, model.file), part = `${target}.part`;
  let hash = crypto.createHash('sha256'), offset = 0;
  const partial = await fs.stat(part).catch(() => null);
  if (partial?.isFile() && partial.size > 0 && partial.size < model.size) {
    for await (const chunk of createReadStream(part)) hash.update(chunk);
    offset = partial.size;
  } else if (partial) await fs.rm(part, { force: true });

  const response = await fetch(url, { signal, headers: offset ? { Range: `bytes=${offset}-` } : {} });
  if (offset && response.status !== 206) { hash = crypto.createHash('sha256'); offset = 0; } // no resume: start over
  if (!response.ok) throw new Error(`The download failed (HTTP ${response.status}). Try again later.`);

  const handle = await fs.open(part, offset ? 'a' : 'w', 0o600);
  let received = offset;
  try {
    for await (const chunk of response.body) {
      if (received + chunk.length > model.size) throw new Error('The download is larger than expected, so it was stopped.');
      hash.update(chunk);
      await handle.write(chunk);
      received += chunk.length;
      onProgress(received, model.size);
    }
  } finally { await handle.close(); }
  if (received !== model.size) throw new Error('The download stopped early. Download again to resume it.');
  if (hash.digest('hex') !== model.sha256) {
    await fs.rm(part, { force: true });
    throw new Error('The downloaded file didn’t match the expected model, so it was deleted. Try again.');
  }
  await fs.rename(part, target);
  return target;
}

async function remove(model, dir) {
  const target = path.join(dir, model.file);
  await fs.rm(target, { force: true });
  await fs.rm(`${target}.part`, { force: true });
  return target;
}

module.exports = { MODELS, SOURCE, recommended, englishOnly, installed, download, remove };
