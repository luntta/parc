const { app, BrowserWindow, ipcMain, session, systemPreferences, dialog, nativeTheme, clipboard, globalShortcut, net } = require('electron');
const path = require('node:path');
const os = require('node:os');
const fs = require('node:fs/promises');
const { watch } = require('node:fs');
const { pathToFileURL } = require('node:url');
const { archive, speechConfig, transcribe } = require('./backend.cjs');
const models = require('./models.cjs');
const { LANGUAGES } = require('./languages.cjs');

const page = pathToFileURL(path.join(__dirname, 'index.html')).href;
const capturePage = pathToFileURL(path.join(__dirname, 'capture.html')).href;
const QUICK = '--capture'; // `electron . --capture` opens the quick capture window, in the running app if there is one
const webPreferences = { preload: path.join(__dirname, 'preload.cjs'), nodeIntegration: false, contextIsolation: true, sandbox: true };
const canvas = () => nativeTheme.shouldUseDarkColors ? '#000000' : '#ffffff';
// Wayland compositors hand out global shortcuts through the desktop portal.
if (process.platform === 'linux') app.commandLine.appendSwitch('enable-features', 'GlobalShortcutsPortal');

let window, captureWindow;
function guard(win) {
  win.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
  win.webContents.on('will-navigate', event => event.preventDefault());
}
function openWindow() {
  // Shown on first paint, on the theme's canvas colour, so a dark desktop never flashes white.
  window = new BrowserWindow({ width: 1200, height: 800, minWidth: 640, minHeight: 480,
    title: 'parc Capture', autoHideMenuBar: true, show: false, backgroundColor: canvas(), webPreferences });
  guard(window);
  // Drafts survive a restart; only work that cannot be resumed holds the window open.
  window.webContents.on('will-prevent-unload', event => {
    const choice = dialog.showMessageBoxSync(window, { message: 'Leave this workspace?', detail: 'A recording, transcription or save is still in progress and will be lost.', buttons: ['Stay', 'Leave'], defaultId: 0, cancelId: 0 });
    if (choice === 1) event.preventDefault();
  });
  window.once('ready-to-show', () => window?.show());
  window.on('closed', () => { window = undefined; });
  window.loadURL(page);
}
// A small window over whatever you were doing: type, save, gone.
function openCapture() {
  if (captureWindow) { captureWindow.show(); captureWindow.focus(); return; }
  captureWindow = new BrowserWindow({ width: 620, height: 440, minWidth: 420, minHeight: 300, title: 'Quick capture',
    alwaysOnTop: true, autoHideMenuBar: true, show: false, backgroundColor: canvas(), webPreferences });
  guard(captureWindow);
  captureWindow.once('ready-to-show', () => { captureWindow?.show(); captureWindow?.focus(); });
  captureWindow.on('closed', () => { captureWindow = undefined; });
  captureWindow.loadURL(capturePage);
}

// One app per user data directory: a second launch passes its arguments to the first and exits.
if (!app.requestSingleInstanceLock()) app.quit();
else app.whenReady().then(() => {
  const vault = path.join(app.getPath('userData'), 'prototype', '.parc');
  const registry = path.join(app.getPath('userData'), 'vaults.json');
  const settingsFile = path.join(app.getPath('userData'), 'settings.json');
  const home = path.join(os.homedir(), '.parc'); // parc's global vault
  const stores = new Map([[vault, archive(vault)]]);
  let init, globalVault = null;
  const modelDir = path.join(app.getPath('userData'), 'models');
  let settings = { whisperModel: '', whisperBin: '', quickCaptureShortcut: '', speechLanguages: [], speechLanguage: '' }, shortcut = { active: false };
  async function initialize() {
    await stores.get(vault).init();
    globalVault = await fs.realpath(home).catch(() => null);
    let paths = [];
    try { paths = JSON.parse(await fs.readFile(registry, 'utf8')); }
    catch (err) { if (err.code !== 'ENOENT') throw err; }
    for (const p of paths) if (!stores.has(p)) stores.set(p, archive(p));
    for (const p of stores.keys()) watchVault(p);
    try { settings = clean(JSON.parse(await fs.readFile(settingsFile, 'utf8'))); }
    catch { /* none yet, or unreadable: defaults */ }
  }
  const ready = () => init ||= initialize().catch(err => { init = undefined; throw err; });
  const label = p => p === vault ? 'Prototype' : p === globalVault ? 'Global' : path.basename(path.dirname(p));
  function vaults() {
    const entries = [...stores.keys()].map(p => ({ path: p, name: label(p) }));
    return entries.map(v => entries.some(other => other !== v && other.name === v.name) ? { ...v, name: path.dirname(v.path) } : v);
  }
  // Vaults the app can offer by name. Only parc's own global vault: the machine is never scanned.
  async function suggestions() {
    if (!globalVault || stores.has(globalVault)) return [];
    const ok = await fs.stat(path.join(globalVault, 'config.yml')).then(s => s.isFile(), () => false);
    return ok ? [{ path: globalVault, name: 'Global', hint: '~/.parc' }] : [];
  }
  const store = p => {
    if (!stores.has(p)) throw new Error('Choose an opened vault');
    watchVault(p); // a no-op once watched; picks up a vault whose fragments/ appeared after launch
    return stores.get(p);
  };
  const trusted = wc => !!wc && ((wc === window?.webContents && wc.getURL() === page) || (wc === captureWindow?.webContents && wc.getURL() === capturePage));

  // ── Settings: dictation (model, whisper-cli, languages) and the quick capture shortcut, kept beside the vault registry ──
  function clean(input) {
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
    if (input?.quickCaptureShortcut !== undefined) {
      const v = String(input.quickCaptureShortcut).trim();
      if (v && !/^[A-Za-z0-9+ ]{1,64}$/.test(v)) throw new Error('Write the shortcut like CommandOrControl+Shift+Space.');
      next.quickCaptureShortcut = v;
    }
    return next;
  }
  function applyShortcut() {
    globalShortcut.unregisterAll();
    if (!settings.quickCaptureShortcut) return (shortcut = { active: false });
    try {
      return (shortcut = globalShortcut.register(settings.quickCaptureShortcut, openCapture)
        ? { active: true }
        : { active: false, error: 'Couldn’t register it: another app may use it, or this desktop doesn’t let apps register shortcuts.' });
    } catch { return (shortcut = { active: false, error: 'That isn’t a shortcut Electron understands, e.g. CommandOrControl+Shift+Space.' }); }
  }
  // Bind this in the desktop's own shortcut settings where apps can't register one (many Wayland compositors).
  const captureCommand = [process.execPath, ...(app.isPackaged ? [] : [app.getAppPath()]), QUICK].map(a => /\s/.test(a) ? `"${a}"` : a).join(' ');
  const settingsView = () => ({ ...settings, shortcut, captureCommand });

  // Saves from the CLI, the TUI, another window or a git pull land in fragments/; tell the workspace which vault
  // changed so it can refresh its list and notice when the open fragment moved on under it.
  const watchers = new Map();
  function watchVault(p) {
    if (watchers.has(p)) return;
    let timer;
    try {
      const watcher = watch(path.join(p, 'fragments'), () => {
        clearTimeout(timer);
        timer = setTimeout(() => window?.webContents.send('vault-changed', p), 150);
      });
      watcher.on('error', () => { watcher.close(); watchers.delete(p); });
      watchers.set(p, watcher);
    } catch { /* no fragments/ yet: the next request to this vault retries */ }
  }
  app.on('will-quit', () => { globalShortcut.unregisterAll(); for (const w of watchers.values()) w.close(); });

  session.defaultSession.setPermissionRequestHandler((wc, permission, callback, details) => {
    callback(trusted(wc) && permission === 'media' && details.mediaTypes?.join() === 'audio');
  });
  session.defaultSession.setPermissionCheckHandler((wc, permission, _origin, details) =>
    trusted(wc) && permission === 'media' && details.mediaType === 'audio');

  // Handlers get the window that asked, so dialogs open over it.
  function handle(name, handler) {
    ipcMain.handle(name, (event, ...args) => {
      if (!trusted(event.sender) || event.senderFrame !== event.sender.mainFrame) throw new Error('Untrusted frame');
      return handler.call(BrowserWindow.fromWebContents(event.sender), ...args);
    });
  }
  handle('vaults', async () => { await ready(); return vaults(); });
  handle('suggested-vaults', async () => { await ready(); return suggestions(); });
  // With no argument, asks for a folder. A suggested vault can be opened by path; nothing else can.
  handle('open-vault', async function (suggested) {
    await ready();
    let folder;
    if (suggested != null) {
      if (!(await suggestions()).some(s => s.path === suggested)) throw new Error('That vault is not available.');
      folder = suggested;
    } else {
      const choice = await dialog.showOpenDialog(this, { title: 'Open a trusted parc vault or its parent folder', properties: ['openDirectory', 'showHiddenFiles'] });
      if (choice.canceled) return null;
      folder = choice.filePaths[0];
    }
    const p = await fs.realpath(path.basename(folder) === '.parc' ? folder : path.join(folder, '.parc')).catch(() => { throw new Error('No .parc vault in that folder.'); });
    if (!(await fs.stat(path.join(p, 'config.yml'))).isFile()) throw new Error('Not a parc vault');
    if (!stores.has(p)) {
      const answer = await dialog.showMessageBox(this, { type: 'question', message: 'Open this trusted vault?', detail: `${p}\nSaving fragments may run this vault’s local hooks.`, buttons: ['Cancel', 'Open vault'], cancelId: 0, defaultId: 0 });
      if (!answer.response) return null;
      const next = archive(p);
      await fs.writeFile(registry + '.tmp', JSON.stringify([...stores.keys(), p].filter(x => x !== vault)), { mode: 0o600 });
      await fs.rename(registry + '.tmp', registry);
      stores.set(p, next);
      watchVault(p);
    }
    return { path: p, vaults: vaults() };
  });
  // Leaving unsaved work: 'save', 'discard' or 'cancel'. Save is offered only when the draft can be saved.
  handle('discard', async function ({ title = '', canSave = false } = {}) {
    const name = String(title).trim().slice(0, 80);
    const buttons = canSave ? ['Cancel', 'Don’t save', 'Save'] : ['Keep editing', 'Discard'];
    const { response } = await dialog.showMessageBox(this, {
      message: name ? `Save changes to “${name}”?` : 'Save this draft?',
      detail: canSave ? 'Your changes are lost if you don’t save them.' : 'Choose where to save it before leaving, or discard it.',
      buttons, cancelId: 0, defaultId: canSave ? 2 : 0
    });
    return ['cancel', 'discard', 'save'][response];
  });
  // The open fragment was saved elsewhere since it was loaded: 'overwrite', 'reload' or 'cancel'.
  handle('conflict', async function ({ title = '' } = {}) {
    const name = String(title).trim().slice(0, 80) || 'This fragment';
    const { response } = await dialog.showMessageBox(this, {
      type: 'warning', message: `“${name}” changed outside parc Capture`,
      detail: 'It was saved by another app since you opened it. Overwrite keeps your version (the other one stays in its history); Reload discards your changes.',
      buttons: ['Cancel', 'Reload theirs', 'Overwrite'], cancelId: 0, defaultId: 0
    });
    return ['cancel', 'reload', 'overwrite'][response];
  });
  handle('list', (p, query) => query ? store(p).search(query) : store(p).list());
  handle('note', (p, id) => store(p).get(id));
  handle('save', (p, data) => store(p).save(data));
  handle('update', (p, id, changes, expected) => store(p).update(id, changes, expected));
  handle('archive', (p, id, archived, expected) => store(p).archive(id, archived, expected));
  // Asks first; resolves { deleted } so the workspace can offer Undo.
  handle('delete', async function (p, id, title = '') {
    const name = String(title).trim().slice(0, 80) || 'this fragment';
    const { response } = await dialog.showMessageBox(this, {
      message: `Move “${name}” to the trash?`, detail: 'It leaves the vault’s lists and searches. Undo brings it back, as does parc trash --restore.',
      buttons: ['Cancel', 'Move to trash'], cancelId: 0, defaultId: 0
    });
    if (response !== 1) return { deleted: false };
    await store(p).remove(id);
    return { deleted: true };
  });
  handle('untrash', (p, id) => store(p).untrash(id));
  handle('history', (p, id) => store(p).history(id));
  handle('version', (p, id, timestamp) => store(p).version(id, timestamp));
  handle('restore-version', (p, id, timestamp) => store(p).restoreVersion(id, timestamp));
  handle('schemas', p => store(p).schemas());
  handle('links', p => store(p).links());
  handle('copy-text', text => { clipboard.writeText(String(text).slice(0, 10000)); });
  handle('open-capture', () => openCapture());
  handle('settings', async () => { await ready(); return settingsView(); });
  async function persist() {
    await fs.writeFile(settingsFile + '.tmp', JSON.stringify(settings, null, 2), { mode: 0o600 });
    await fs.rename(settingsFile + '.tmp', settingsFile);
  }
  handle('save-settings', async input => {
    await ready();
    settings = clean(input);
    await persist();
    applyShortcut();
    return settingsView();
  });
  // The dictation language picked next to Dictate: one setting, saved at once.
  handle('set-speech-language', async code => {
    await ready();
    settings = clean({ speechLanguage: code });
    await persist();
  });

  // ── Speech models: the catalog, with what is installed, in use and downloading ──
  // One download at a time. Only an explicit Download reaches the network, and only models.SOURCE.
  let download = null;
  async function modelView(languages = settings.speechLanguages) {
    const list = await Promise.all(models.MODELS.map(async m => {
      const file = path.join(modelDir, m.file);
      return {
        id: m.id, label: m.label, note: m.note, size: m.size, english: m.english, path: file,
        installed: await models.installed(m, modelDir),
        downloading: download?.id === m.id ? { received: download.received, size: m.size } : null
      };
    }));
    return { models: list, recommended: models.recommended(languages), dir: modelDir };
  }
  const catalogModel = id => {
    const model = models.MODELS.find(m => m.id === id);
    if (!model) throw new Error('Unknown model.');
    return model;
  };
  handle('speech-models', async languages => {
    await ready();
    return modelView(Array.isArray(languages) ? languages.filter(c => LANGUAGES.includes(c)) : undefined);
  });
  handle('download-model', async id => {
    await ready();
    const model = catalogModel(id);
    if (download) throw new Error('Another model is downloading. Wait for it, or cancel it.');
    const controller = new AbortController();
    download = { id, controller, received: 0 };
    const notify = data => window?.webContents.send('model-progress', { id, size: model.size, ...data });
    let last = 0;
    try {
      const file = await models.download(model, {
        dir: modelDir, signal: controller.signal, fetch: (url, init) => net.fetch(url, init),
        onProgress: received => {
          download.received = received;
          if (Date.now() - last > 200) { last = Date.now(); notify({ received }); }
        }
      });
      // The first model downloaded is the one dictation uses.
      if (!settings.whisperModel || !(await fs.stat(settings.whisperModel).catch(() => null))) {
        settings = clean({ whisperModel: file });
        await persist();
      }
      return { path: file, inUse: settings.whisperModel === file };
    } catch (err) {
      if (controller.signal.aborted) return { cancelled: true };
      throw err;
    } finally {
      download = null;
      notify({ done: true });
    }
  });
  handle('cancel-download', () => { download?.controller.abort(); });
  handle('delete-model', async function (id) {
    await ready();
    const model = catalogModel(id);
    if (download?.id === id) throw new Error('Cancel the download first.');
    const { response } = await dialog.showMessageBox(this, {
      message: `Delete the “${model.label}” speech model?`, detail: `This frees ${Math.round(model.size / 1e6)} MB. You can download it again later.`,
      buttons: ['Cancel', 'Delete'], cancelId: 0, defaultId: 0
    });
    if (response !== 1) return { deleted: false };
    const file = await models.remove(model, modelDir);
    if (settings.whisperModel === file) { settings = clean({ whisperModel: '' }); await persist(); }
    return { deleted: true, path: file };
  });
  handle('pick-file', async function (kind) {
    const model = kind === 'model';
    const choice = await dialog.showOpenDialog(this, {
      title: model ? 'Choose a whisper.cpp speech model' : 'Choose the whisper-cli program',
      properties: ['openFile', 'showHiddenFiles'],
      filters: model ? [{ name: 'whisper.cpp models', extensions: ['bin', 'gguf'] }, { name: 'All files', extensions: ['*'] }] : []
    });
    return choice.canceled ? null : choice.filePaths[0];
  });
  handle('speech-status', async () => {
    await ready();
    const languages = settings.speechLanguages;
    try {
      const { language, warning, model } = await speechConfig(process.env, settings);
      return { ready: true, language, languages, warning, model };
    } catch (err) { return { ready: false, problem: err.message, languages }; }
  });
  handle('speech-ready', async () => {
    await ready();
    await speechConfig(process.env, settings);
    if (process.platform === 'darwin' && !await systemPreferences.askForMediaAccess('microphone')) {
      throw new Error('Allow Electron microphone access in System Settings → Privacy & Security.');
    }
  });
  let busy = false;
  handle('transcribe', async audio => {
    if (busy) throw new Error('Transcription already running.');
    busy = true;
    try { return await transcribe(audio, await speechConfig(process.env, settings)); } finally { busy = false; }
  });

  ready().then(applyShortcut, () => {});
  if (process.argv.includes(QUICK)) openCapture(); else openWindow();
  app.on('second-instance', (_event, argv) => {
    if (argv.includes(QUICK)) return openCapture();
    if (!window) return openWindow();
    if (window.isMinimized()) window.restore();
    window.focus();
  });
  app.on('activate', () => { if (!window) openWindow(); });
});
app.on('window-all-closed', () => { if (process.platform !== 'darwin') app.quit(); });
