// The app's own settings (userData/settings.json): the theme, dictation (model, whisper-cli, languages) and the quick
// capture shortcut. A change is checked here before it is kept; the result is the settings with it applied.
const path = require('node:path');
const { LANGUAGES } = require('./languages.cjs');

const THEMES = ['system', 'light', 'dark'];
const DEFAULTS = { whisperModel: '', whisperBin: '', quickCaptureShortcut: '', speechLanguages: [], speechLanguage: '', theme: 'system' };

function cleanSettings(settings, input) {
  const next = { ...settings };
  for (const k of ['whisperModel', 'whisperBin']) {
    if (input?.[k] === undefined) continue;
    const v = String(input[k]).trim();
    if (v && (!path.isAbsolute(v) || v.length > 4096)) throw new Error('Choose files by their full path.');
    next[k] = v;
  }
  if (input?.speechLanguages !== undefined) {
    const list = input.speechLanguages;
    if (!Array.isArray(list) || list.length > 12 || !list.every(c => LANGUAGES.includes(c)) || new Set(list).size !== list.length) {
      throw new Error('Choose languages from the list.');
    }
    next.speechLanguages = list;
  }
  if (input?.speechLanguage !== undefined) {
    const v = String(input.speechLanguage);
    if (v && v !== 'auto' && !LANGUAGES.includes(v)) throw new Error('Choose a language from the list.'); // '' = the first preferred
    next.speechLanguage = v;
  }
  if (input?.theme !== undefined) {
    if (!THEMES.includes(input.theme)) throw new Error('Choose System, Light or Dark.');
    next.theme = input.theme;
  }
  if (input?.quickCaptureShortcut !== undefined) {
    const v = String(input.quickCaptureShortcut).trim();
    if (v && !/^[A-Za-z0-9+ ]{1,64}$/.test(v)) throw new Error('Write the shortcut like CommandOrControl+Shift+Space.');
    next.quickCaptureShortcut = v;
  }
  return next;
}

module.exports = { cleanSettings, DEFAULTS, THEMES };
