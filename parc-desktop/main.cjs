const { sweepMessage, sweepVaults } = require('./inbox-report.cjs');
const { app, BrowserWindow, ipcMain, session, systemPreferences, dialog, nativeTheme, clipboard, globalShortcut, net, shell, safeStorage, protocol } = require('electron');
const path = require('node:path');
const os = require('node:os');
const fs = require('node:fs/promises');
const { watch, readFileSync } = require('node:fs');
const { pathToFileURL } = require('node:url');
const { randomUUID, randomBytes, createHash } = require('node:crypto');
const { archive, binary, speechConfig, transcribe } = require('./backend.cjs');
const { draftStore } = require('./drafts.cjs');
const agent = require('./agent.cjs');
const copilot = require('./copilot.cjs');
const models = require('./models.cjs');
const { LANGUAGES } = require('./languages.cjs');
const { omarchyTheme } = require('./omarchy.cjs');

// Narrow scheme for raster image previews inside opened vaults' attachments/ directories (never arbitrary files).
protocol.registerSchemesAsPrivileged([
  { scheme: 'parc-attach', privileges: { standard: true, secure: true, supportFetchAPI: false } }
]);
const IMAGE_MIME = { '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.gif': 'image/gif', '.webp': 'image/webp', '.avif': 'image/avif' };
const ULID_RE = /^[0-9A-HJKMNP-TV-Z]{26}$/;
const safeFile = name => typeof name === 'string' && name.length > 0 && name.length <= 255 && name !== '.' && name !== '..' && !/[/\\\0]/.test(name);

const page = pathToFileURL(path.join(__dirname, 'index.html')).href;
const capturePage = pathToFileURL(path.join(__dirname, 'capture.html')).href;
const QUICK = '--capture'; // `electron . --capture` opens the quick capture window, in the running app if there is one
const webPreferences = { preload: path.join(__dirname, 'preload.cjs'), nodeIntegration: false, contextIsolation: true, sandbox: true };
const canvas = () => nativeTheme.shouldUseDarkColors ? '#000000' : '#ffffff';
const THEMES = ['system', 'light', 'dark'];
// Wayland compositors hand out global shortcuts through the desktop portal, and native Wayland enables real window transparency.
if (process.platform === 'linux') {
  app.commandLine.appendSwitch('enable-features', 'GlobalShortcutsPortal');
  if (process.env.WAYLAND_DISPLAY && !app.commandLine.hasSwitch('ozone-platform') && !app.commandLine.hasSwitch('ozone-platform-hint')) {
    app.commandLine.appendSwitch('ozone-platform-hint', 'auto');
  }
}
// Chromium picks the keyring by the desktop's name, and on one it doesn't know (Hyprland, sway, i3…) keeps secrets
// under a fixed key even when a Secret Service (gnome-keyring, KeePassXC) is running. Ask for libsecret there; with no
// Secret Service it still falls back. A --password-store given on the command line wins.
if (process.platform === 'linux' && !app.commandLine.hasSwitch('password-store')
  && !/gnome|cinnamon|deepin|pantheon|xfce|ukui|unity|kde/i.test(process.env.XDG_CURRENT_DESKTOP || '')) {
  app.commandLine.appendSwitch('password-store', 'gnome-libsecret');
}

let window, captureWindow;
let transparentWindows = false, activeCanvas = null, refreshDesktopTheme = () => {};
const windowBackground = () => transparentWindows ? '#00000000' : (activeCanvas || canvas());
function guard(win) {
  win.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
  win.webContents.on('will-navigate', event => event.preventDefault());
}
function openWindow() {
  // Shown on first paint, on the theme's canvas colour (or frameless + transparent on Wayland/Omarchy so
  // Chromium's NativeWindowViews allocates an ARGB Wayland surface for Transparent/Glassy modes).
  window = new BrowserWindow({ width: 1200, height: 800, minWidth: 640, minHeight: 480,
    title: 'parc Capture', autoHideMenuBar: true, show: false,
    frame: !transparentWindows, transparent: transparentWindows, backgroundColor: windowBackground(), webPreferences });
  guard(window);
  // Ctrl+W (⌘W) closes a tab, not the window: the menu's shortcut stands aside and the page handles the key.
  const contents = window.webContents;
  contents.on('before-input-event', (_event, input) => {
    contents.setIgnoreMenuShortcuts(input.type === 'keyDown' && (input.control || input.meta) && !input.alt && input.key.toLowerCase() === 'w');
  });
  // Drafts survive a restart; only work that cannot be resumed holds the window open.
  window.webContents.on('will-prevent-unload', event => {
    const choice = dialog.showMessageBoxSync(window, { message: 'Leave this workspace?', detail: 'A recording, transcription or save is still in progress and will be lost.', buttons: ['Stay', 'Leave'], defaultId: 0, cancelId: 0 });
    if (choice === 1) event.preventDefault();
  });
  window.on('focus', () => refreshDesktopTheme());
  window.once('ready-to-show', () => window?.show());
  window.on('closed', () => { window = undefined; });
  window.loadURL(page);
}
// A small window over whatever you were doing: type, save, gone.
function openCapture() {
  if (captureWindow) { captureWindow.show(); captureWindow.focus(); return; }
  captureWindow = new BrowserWindow({ width: 620, height: 440, minWidth: 420, minHeight: 300, title: 'Quick capture',
    alwaysOnTop: true, autoHideMenuBar: true, show: false,
    frame: !transparentWindows, transparent: transparentWindows, backgroundColor: windowBackground(), webPreferences });
  guard(captureWindow);
  captureWindow.on('focus', () => refreshDesktopTheme());
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
  let settings = { whisperModel: '', whisperBin: '', quickCaptureShortcut: '', speechLanguages: [], speechLanguage: '', theme: 'system' }, shortcut = { active: false };
  // Read before the first window, so a saved theme (and Omarchy palette) is the one it paints in.
  try { settings = clean(JSON.parse(readFileSync(settingsFile, 'utf8'))); }
  catch { /* none yet, or unreadable: defaults */ }
  let activeTheme = settings.theme;
  const omarchy = omarchyTheme({ onChange: () => applyTheme(activeTheme) });
  omarchy.start();
  transparentWindows = process.platform === 'linux' && !!process.env.WAYLAND_DISPLAY
    && omarchy.available() && process.env.PARC_NO_TRANSPARENT !== '1';
  refreshDesktopTheme = () => omarchy.refreshDesktop().catch(() => {});
  applyTheme();
  async function initialize() {
    await stores.get(vault).init();
    globalVault = await fs.realpath(home).catch(() => null);
    let paths = [];
    try { paths = JSON.parse(await fs.readFile(registry, 'utf8')); }
    catch (err) { if (err.code !== 'ENOENT') throw err; }
    for (const p of paths) if (!stores.has(p)) stores.set(p, archive(p));
    for (const p of stores.keys()) {
      watchVault(p);
      backgroundSweep(p);
    }
  }
  const ready = () => init ||= initialize().catch(err => { init = undefined; throw err; });
  const label = p => p === vault ? 'Prototype' : p === globalVault ? 'Global' : path.basename(path.dirname(p));
  // Opaque per-session tokens for opened vaults so parc-attach:// URLs never expose local paths.
  const tokenToVault = new Map(), vaultToToken = new Map();
  function tokenFor(p) {
    if (!vaultToToken.has(p)) { const t = randomBytes(8).toString('hex'); vaultToToken.set(p, t); tokenToVault.set(t, p); }
    return vaultToToken.get(p);
  }
  function dropToken(p) { const t = vaultToToken.get(p); if (t) { vaultToToken.delete(p); tokenToVault.delete(t); } }
  async function resolveAttachmentFile(vaultPath, id, filename) {
    if (!stores.has(vaultPath) || !ULID_RE.test(id) || !safeFile(filename)) return null;
    try {
      const dir = await fs.realpath(path.join(vaultPath, 'attachments', id));
      const file = await fs.realpath(path.join(dir, filename));
      if (!file.startsWith(dir + path.sep)) return null;
      const stat = await fs.stat(file);
      return stat.isFile() ? { file, size: stat.size } : null;
    } catch { return null; }
  }
  protocol.handle('parc-attach', async req => {
    try {
      const url = new URL(req.url);
      const vaultPath = tokenToVault.get(url.hostname);
      const parts = url.pathname.split('/').filter(Boolean);
      if (!vaultPath || parts.length !== 2) return new Response('Not found', { status: 404 });
      const id = parts[0], filename = decodeURIComponent(parts[1]);
      const mime = IMAGE_MIME[path.extname(filename).toLowerCase()];
      if (!mime) return new Response('Forbidden', { status: 403 });
      const found = await resolveAttachmentFile(vaultPath, id, filename);
      if (!found || found.size > 20 * 1024 * 1024) return new Response('Not found', { status: 404 });
      const bytes = await fs.readFile(found.file);
      return new Response(bytes, { headers: { 'content-type': mime, 'cache-control': 'private, max-age=60' } });
    } catch { return new Response('Not found', { status: 404 }); }
  });
  function vaults() {
    const entries = [...stores.keys()].map(p => ({ path: p, name: label(p), token: tokenFor(p), removable: p !== vault }));
    return entries.map(v => entries.some(other => other !== v && other.name === v.name) ? { ...v, name: path.dirname(v.path) } : v);
  }
  async function saveRegistry() {
    await fs.writeFile(registry + '.tmp', JSON.stringify([...stores.keys()].filter(x => x !== vault)), { mode: 0o600 });
    await fs.rename(registry + '.tmp', registry);
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

  // ── Settings: the theme, dictation (model, whisper-cli, languages) and the quick capture shortcut, kept beside the vault registry ──
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
  // System follows Omarchy when present (or the desktop's dark/light setting otherwise).
  // Light and Dark override it for every window, prefers-color-scheme included.
  function effectiveOmarchy(theme = activeTheme) {
    return theme === 'system' ? omarchy.current() : null;
  }
  function omarchyPayload(om) {
    return om ? { name: om.name, mode: om.mode, opacity: om.opacity, glass: om.glass, rounding: om.rounding, tokens: om.tokens } : null;
  }
  function pushSystemTheme(theme = activeTheme) {
    const payload = omarchyPayload(effectiveOmarchy(theme));
    for (const w of BrowserWindow.getAllWindows()) {
      if (!w.isDestroyed()) w.webContents.send('system-theme', payload);
    }
  }
  function applyTheme(theme = settings.theme) {
    activeTheme = theme;
    const om = effectiveOmarchy(theme);
    activeCanvas = om ? om.canvas : null;
    nativeTheme.themeSource = om ? om.mode : theme;
    if (!transparentWindows) {
      const bg = windowBackground();
      for (const w of BrowserWindow.getAllWindows()) if (!w.isDestroyed()) w.setBackgroundColor(bg);
    }
    pushSystemTheme(theme);
  }
  nativeTheme.on('updated', () => {
    if (transparentWindows) return;
    const bg = windowBackground();
    for (const w of BrowserWindow.getAllWindows()) if (!w.isDestroyed()) w.setBackgroundColor(bg);
  });
  // On Wayland, Electron asks the desktop portal for the shortcut and reports success whether or not the
  // portal agrees. Hyprland's refuses an app it can't match to an installed .desktop file, so all we know is
  // that we asked.
  const viaPortal = process.platform === 'linux' && !!process.env.WAYLAND_DISPLAY;
  function applyShortcut() {
    globalShortcut.unregisterAll();
    if (!settings.quickCaptureShortcut) return (shortcut = { active: false });
    try {
      return (shortcut = globalShortcut.register(settings.quickCaptureShortcut, openCapture)
        ? (viaPortal ? { active: false, asked: true } : { active: true })
        : { active: false, error: 'Couldn’t register it: another app may use it, or this desktop doesn’t let apps register shortcuts.' });
    } catch { return (shortcut = { active: false, error: 'That isn’t a shortcut Electron understands, e.g. CommandOrControl+Shift+Space.' }); }
  }
  // Bind this in the desktop's own shortcut settings where apps can't register one (many Wayland compositors).
  const captureCommand = [process.execPath, ...(app.isPackaged ? [] : [app.getAppPath()]), QUICK].map(a => /\s/.test(a) ? `"${a}"` : a).join(' ');
  const omarchySummary = () => {
    const om = omarchy.current();
    return om ? { name: om.name, mode: om.mode, opacity: om.opacity, glass: om.glass, rounding: om.rounding } : null;
  };
  const settingsView = () => ({ ...settings, shortcut, captureCommand, omarchy: omarchySummary() });

  // Saves from the CLI, the TUI, another window or a git pull land in fragments/, schemas/ or config.yml; tell the
  // workspace which vault changed so it can refresh its list and notice when an open fragment moved on under it.
  // Writes made by the main workspace itself record the resulting file's SHA-256 hash so its own watcher echo is
  // suppressed without ever missing an external edit (even one made in the same millisecond).
  const watchers = new Map(); // vault → { close(), ensureInbox() }
  const sweepTimers = new Map(); // vault → timeout for retrying unsettled inbox files
  const resweep = new Set();
  const sweeping = new Map(); // vault → Promise
  async function sweepVault(p) {
    if (!stores.has(p)) throw new Error('Choose an opened vault');
    if (sweeping.has(p)) return sweeping.get(p);
    const run = (async () => {
      const res = await store(p).inboxSweep();
      watchers.get(p)?.ensureInbox?.();
      if (res?.created?.length) {
        window?.webContents.send('vault-changed', { vault: p, kind: 'inbox', created: res.created });
      }
      if (res.completed) window?.webContents.send('vault-changed', { vault: p, kind: 'attachments' });
      if (res.failed?.length || res.warnings?.length) {
        window?.webContents.send('vault-changed', { vault: p, kind: 'inbox-error', message: sweepMessage(res) });
      }
      const scanAgain = resweep.delete(p);
      if (stores.has(p) && (res.waiting?.length || res.pending || res.more || scanAgain)) {
        clearTimeout(sweepTimers.get(p));
        const t = setTimeout(() => { sweepTimers.delete(p); backgroundSweep(p); }, res.more ? 0 : 1200);
        sweepTimers.set(p, t);
      }
      return res;
    })();
    sweeping.set(p, run);
    try { return await run; } finally { sweeping.delete(p); }
  }
  function backgroundSweep(p) {
    if (sweeping.has(p)) { resweep.add(p); return; }
    sweepVault(p).catch(err => {
      if (stores.has(p)) window?.webContents.send('vault-changed', { vault: p, kind: 'inbox-error', message: String(err.message ?? err) });
    });
  }
  const ownWrites = new Map(); // vault → Map<relPath, { hash: string | null, at: number }>
  async function fileHash(filePath) {
    try { return createHash('sha256').update(await fs.readFile(filePath)).digest('hex'); }
    catch (err) { if (err.code === 'ENOENT') return null; return undefined; }
  }
  async function recordOwn(vaultPath, relPath) {
    if (!ownWrites.has(vaultPath)) ownWrites.set(vaultPath, new Map());
    const hash = await fileHash(path.join(vaultPath, relPath));
    if (hash !== undefined) ownWrites.get(vaultPath).set(relPath, { hash, at: Date.now() });
  }
  async function isOnlyOwn(vaultPath, changed) {
    if (!changed.size || changed.has(null)) return false;
    const known = ownWrites.get(vaultPath);
    if (!known || !known.size) return false;
    const now = Date.now();
    for (const [k, v] of known) if (now - v.at > 5000) known.delete(k);
    for (const rel of changed) {
      const entry = known.get(rel);
      if (!entry) return false;
      const current = await fileHash(path.join(vaultPath, rel));
      if (current !== entry.hash) { known.delete(rel); return false; }
    }
    return true;
  }
  function watchVault(p) {
    if (watchers.has(p)) return;
    const handles = [];
    let fragTimer, schemaTimer, closed = false;
    const pendingFrags = new Set(), pendingSchemas = new Set();
    const notify = (kind, pending) => {
      const batch = new Set(pending);
      pending.clear();
      isOnlyOwn(p, batch).then(suppress => {
        if (!suppress && !closed) window?.webContents.send('vault-changed', { vault: p, kind });
      }, () => { if (!closed) window?.webContents.send('vault-changed', { vault: p, kind }); });
    };
    try {
      const fw = watch(path.join(p, 'fragments'), (_event, name) => {
        if (name && (name.startsWith('.') || name.endsWith('.tmp'))) return;
        pendingFrags.add(name ? `fragments/${name}` : null);
        clearTimeout(fragTimer);
        fragTimer = setTimeout(() => notify('fragments', pendingFrags), 150);
      });
      fw.on('error', () => { closeAll(); });
      handles.push(fw);
    } catch { /* no fragments/ yet: the next request to this vault retries */ return; }
    try {
      const sw = watch(path.join(p, 'schemas'), (_event, name) => {
        if (name && (name.startsWith('.') || name.endsWith('.tmp'))) return;
        pendingSchemas.add(name ? `schemas/${name}` : null);
        clearTimeout(schemaTimer);
        schemaTimer = setTimeout(() => notify('schemas', pendingSchemas), 150);
      });
      sw.on('error', () => { sw.close(); });
      handles.push(sw);
    } catch { /* optional */ }
    let cw = null;
    function watchConfig() {
      if (closed) return;
      try {
        cw = watch(path.join(p, 'config.yml'), event => {
          pendingSchemas.add('config.yml');
          clearTimeout(schemaTimer);
          schemaTimer = setTimeout(() => notify('schemas', pendingSchemas), 150);
          if (event === 'rename') { cw?.close(); cw = null; setTimeout(watchConfig, 50); }
        });
        cw.on('error', () => { cw?.close(); cw = null; });
      } catch { /* optional */ }
    }
    watchConfig();
    let iw = null, inboxTimer = null;
    function ensureInbox() {
      if (closed || iw) return;
      try {
        iw = watch(path.join(p, 'inbox'), (_event, name) => {
          if (name && (name.startsWith('.') || /\.(part|crdownload|tmp)$/i.test(name))) return;
          clearTimeout(inboxTimer);
          inboxTimer = setTimeout(() => { if (!closed) backgroundSweep(p); }, 350);
        });
        iw.on('error', () => { try { iw?.close(); } catch { /* ignore */ } iw = null; });
      } catch { /* inbox/ may not exist yet on older vaults */ }
    }
    ensureInbox();
    function closeAll() {
      closed = true;
      resweep.delete(p);
      clearTimeout(fragTimer); clearTimeout(schemaTimer); clearTimeout(inboxTimer);
      clearTimeout(sweepTimers.get(p)); sweepTimers.delete(p);
      for (const h of handles) { try { h.close(); } catch { /* ignore */ } }
      try { cw?.close(); } catch { /* ignore */ }
      try { iw?.close(); } catch { /* ignore */ }
      watchers.delete(p);
    }
    watchers.set(p, { close: closeAll, ensureInbox });
  }
  app.on('will-quit', () => {
    globalShortcut.unregisterAll();
    omarchy.close();
    for (const t of sweepTimers.values()) clearTimeout(t);
    sweepTimers.clear();
    for (const w of watchers.values()) w.close();
    for (const s of stores.values()) s.close();
    for (const s of toolServers.values()) s.close();
    copilotRuntime.stop();
  });

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
      stores.set(p, next);
      await saveRegistry();
      watchVault(p);
      backgroundSweep(p);
    }
    return { path: p, vaults: vaults() };
  });
  // Create a new .parc vault inside a user-picked folder (or open it if one already exists there).
  handle('create-vault', async function () {
    await ready();
    const choice = await dialog.showOpenDialog(this, {
      title: 'Choose a folder for the new parc vault',
      properties: ['openDirectory', 'createDirectory', 'showHiddenFiles']
    });
    if (choice.canceled) return null;
    const folder = await fs.realpath(choice.filePaths[0]);
    const target = path.basename(folder) === '.parc' ? folder : path.join(folder, '.parc');
    const exists = await fs.stat(path.join(target, 'config.yml')).then(s => s.isFile(), () => false);
    if (!stores.has(target)) {
      const next = archive(target);
      if (!exists) await next.init();
      stores.set(target, next);
      await saveRegistry();
      watchVault(target);
      backgroundSweep(target);
    }
    return { path: target, vaults: vaults(), existed: exists };
  });
  // Remove an opened vault from the app's registry without deleting any files on disk.
  handle('close-vault', async function (p) {
    await ready();
    if (!stores.has(p) || p === vault) throw new Error('This vault cannot be removed.');
    const { response } = await dialog.showMessageBox(this, {
      message: `Remove “${label(p)}” from parc Capture?`,
      detail: `${p}\nIts files on disk stay untouched. You can open it again any time.`,
      buttons: ['Cancel', 'Remove'], cancelId: 0, defaultId: 0
    });
    if (response !== 1) return { closed: false, vaults: vaults() };
    watchers.get(p)?.close();
    stores.get(p)?.close();
    stores.delete(p);
    toolServers.get(p)?.close();
    toolServers.delete(p);
    ownWrites.delete(p);
    dropToken(p);
    await saveRegistry();
    return { closed: true, vaults: vaults() };
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
  handle('grep', (p, pattern) => store(p).grep(pattern));
  handle('recall', (p, seed) => store(p).recall(seed));
  handle('save', async function (p, data) {
    const res = await store(p).save(data);
    if (this === window && !res?.conflict && res?.id) await recordOwn(p, `fragments/${res.id}.md`);
    return res;
  });
  handle('update', async function (p, id, changes, expected) {
    const res = await store(p).update(id, changes, expected);
    if (this === window && !res?.conflict && res?.id) await recordOwn(p, `fragments/${res.id}.md`);
    return res;
  });
  handle('promote', async function (p, id, type, expected, fields) {
    const res = await store(p).promote(id, type, expected, fields);
    if (this === window && !res?.conflict && res?.id) await recordOwn(p, `fragments/${res.id}.md`);
    return res;
  });
  handle('inbox-attachments', p => store(p).inboxAttachments());
  handle('attachments', (p, id) => store(p).attachments(id));
  handle('pick-attachment', async function (p, id) {
    const choice = await dialog.showOpenDialog(this, { title: 'Attach files to fragment', properties: ['openFile', 'multiSelections'] });
    if (choice.canceled || !choice.filePaths.length) return null;
    const attached = [], errors = [];
    let last = null;
    for (const filePath of choice.filePaths) {
      try {
        last = await store(p).attach(id, filePath);
        attached.push(last.filename);
      } catch (err) { errors.push(`${path.basename(filePath)}: ${err.message}`); }
    }
    if (this === window && last?.id) await recordOwn(p, `fragments/${last.id}.md`);
    return { attached, errors, attachments: last?.attachments ?? null, revision: last?.revision ?? null };
  });
  handle('attach-bytes', async function (p, id, { name = 'attachment.bin', bytes } = {}) {
    if (!(bytes instanceof Uint8Array) || !bytes.byteLength || bytes.byteLength > 25 * 1024 * 1024) {
      throw new Error('Attachments must be between 1 byte and 25 MB.');
    }
    const clean = path.basename(String(name || 'attachment.bin')).replace(/[/\\\0<>:"|?*\x00-\x1f]/g, '_').trim() || 'attachment.bin';
    const existing = new Set((await store(p).attachments(id)).map(a => a.filename));
    const ext = path.extname(clean), stem = clean.slice(0, clean.length - ext.length) || 'attachment';
    let finalName = clean;
    for (let n = 2; existing.has(finalName) && n < 1000; n++) finalName = `${stem}-${n}${ext}`;
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'parc-attach-'));
    const tmpFile = path.join(dir, finalName);
    try {
      await fs.writeFile(tmpFile, bytes, { mode: 0o600 });
      const res = await store(p).attach(id, tmpFile);
      if (this === window && res?.id) await recordOwn(p, `fragments/${res.id}.md`);
      return res;
    } finally { await fs.rm(dir, { recursive: true, force: true }); }
  });
  handle('detach', async function (p, id, filename) {
    if (!safeFile(filename)) throw new Error('Invalid attachment filename');
    const { response } = await dialog.showMessageBox(this, {
      message: `Remove attachment “${filename.slice(0, 80)}”?`,
      detail: 'The attached file is deleted from this fragment’s attachments folder.',
      buttons: ['Cancel', 'Remove'], cancelId: 0, defaultId: 0
    });
    if (response !== 1) return { detached: false };
    const res = await store(p).detach(id, filename);
    if (this === window && res?.id) await recordOwn(p, `fragments/${res.id}.md`);
    return res;
  });
  handle('open-attachment', async (p, id, filename) => {
    const found = await resolveAttachmentFile(p, id, filename);
    if (!found) throw new Error('Attachment not found.');
    const err = await shell.openPath(found.file);
    if (err) throw new Error(err);
  });
  handle('attachment-text', (p, id, filename, opts) => store(p).attachmentText(id, filename, opts));
  handle('attachment-locate', (p, id, filename, quote, partHint) => store(p).attachmentLocate(id, filename, quote, partHint));
  // Binary bytes for the built-in PDF viewer only (parc-attach:// stays raster images only).
  const MAX_VIEWER_BYTES = 50 * 1024 * 1024;
  handle('attachment-bytes', async (p, id, filename) => {
    const found = await resolveAttachmentFile(p, id, filename);
    if (!found) throw new Error('Attachment not found.');
    if (found.size > MAX_VIEWER_BYTES) throw new Error('This file is larger than 50 MB. Open it with the system viewer instead.');
    const buf = await fs.readFile(found.file);
    return new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength);
  });
  handle('move-attachment', async function (p, fromId, toId, filename) {
    const res = await store(p).moveAttachment(fromId, toId, filename);
    if (this === window) {
      await recordOwn(p, `fragments/${fromId}.md`);
      await recordOwn(p, `fragments/${toId}.md`);
    }
    return res;
  });
  handle('link-fragments', async function (p, idA, idB) {
    const res = await store(p).link(idA, idB);
    if (this === window) {
      await recordOwn(p, `fragments/${idA}.md`);
      await recordOwn(p, `fragments/${idB}.md`);
    }
    return res;
  });
  handle('inbox-sweep', async p => {
    await ready();
    if (p) return sweepVault(p);
    return sweepVaults([...stores.keys()], sweepVault, label);
  });
  handle('inbox-path', async p => {
    await ready();
    if (!stores.has(p)) throw new Error('Choose an opened vault');
    return path.join(p, 'inbox');
  });
  handle('open-inbox-folder', async p => {
    await ready();
    if (!stores.has(p)) throw new Error('Choose an opened vault');
    await sweepVault(p);
    const dir = path.join(p, 'inbox');
    const stat = await fs.lstat(dir).catch(() => null);
    if (!stat || stat.isSymbolicLink() || !stat.isDirectory()) throw new Error('Inbox folder is not available.');
    const err = await shell.openPath(dir);
    if (err) throw new Error(err);
  });
  // Reading positions: kept per computer in userData, never inside the vault.
  const positionsFile = path.join(app.getPath('userData'), 'reading-positions.json');
  let positionsCache = null, posSaveQueue = Promise.resolve();
  async function loadPositions() {
    if (positionsCache) return positionsCache;
    try {
      const parsed = JSON.parse(await fs.readFile(positionsFile, 'utf8'));
      positionsCache = parsed && typeof parsed === 'object' && !Array.isArray(parsed) ? parsed : {};
    } catch { positionsCache = {}; }
    return positionsCache;
  }
  handle('reading-positions', async () => {
    await ready();
    const all = await loadPositions();
    const out = {};
    for (const [k, v] of Object.entries(all)) {
      try { const [vp] = JSON.parse(k); if (stores.has(vp)) out[k] = v; } catch { /* skip */ }
    }
    return out;
  });
  handle('set-reading-position', async (p, id, pos) => {
    await ready();
    if (!stores.has(p) || !ULID_RE.test(id)) throw new Error('Invalid reading position target');
    if (!pos || typeof pos !== 'object') throw new Error('Invalid reading position');
    const part = Number(pos.part);
    const total = pos.total != null ? Number(pos.total) : null;
    const unit = ['page', 'slide', 'section'].includes(pos.unit) ? pos.unit : 'page';
    const filename = pos.filename && safeFile(pos.filename) ? pos.filename : null;
    if (!Number.isInteger(part) || part < 1 || part > 100000) throw new Error('Invalid page number');
    if (total != null && (!Number.isInteger(total) || total < 1 || total > 100000)) throw new Error('Invalid page count');
    const all = await loadPositions();
    all[JSON.stringify([p, id])] = { part, total, unit, filename, updated: new Date().toISOString() };
    posSaveQueue = posSaveQueue.then(async () => {
      await fs.writeFile(positionsFile + '.tmp', JSON.stringify(all), { mode: 0o600 });
      await fs.rename(positionsFile + '.tmp', positionsFile);
    }, () => {});
    await posSaveQueue;
  });
  // Narrow external link opener for PDF / Markdown viewer links: only http, https, mailto.
  handle('open-external', async raw => {
    let url;
    try { url = new URL(String(raw || '')); } catch { throw new Error('Invalid link address.'); }
    if (!['http:', 'https:', 'mailto:'].includes(url.protocol)) throw new Error('Only web and email links can be opened.');
    await shell.openExternal(url.toString());
  });
  handle('archive', async function (p, id, archived, expected) {
    const res = await store(p).archive(id, archived, expected);
    if (this === window && !res?.conflict && res?.id) await recordOwn(p, `fragments/${res.id}.md`);
    return res;
  });
  // Asks first; resolves { deleted } so the workspace can offer Undo.
  handle('delete', async function (p, id, title = '') {
    const name = String(title).trim().slice(0, 80) || 'this fragment';
    const { response } = await dialog.showMessageBox(this, {
      message: `Move “${name}” to the trash?`, detail: 'It leaves the vault’s lists and searches. Undo brings it back, or restore it from Trash.',
      buttons: ['Cancel', 'Move to trash'], cancelId: 0, defaultId: 0
    });
    if (response !== 1) return { deleted: false };
    await store(p).remove(id);
    if (this === window) await recordOwn(p, `fragments/${id}.md`);
    return { deleted: true };
  });
  // One-key discard in Inbox: skips the dialog ONLY for a `source` fragment with no body notes.
  handle('delete-quiet', async function (p, id, title = '', { onlyIfEmpty = false } = {}) {
    const frag = await store(p).get(id);
    if (onlyIfEmpty && (frag.type !== 'source' || frag.attachments?.length || (await store(p).attachments(id)).length)) {
      return { deleted: false };
    }
    const hasNotes = !!(frag.body || '').trim();
    if (frag.type !== 'source' || hasNotes) {
      const name = String(title || frag.title || '').trim().slice(0, 80) || 'this fragment';
      const { response } = await dialog.showMessageBox(this, {
        message: `Move “${name}” to the trash?`,
        detail: hasNotes
          ? 'This source has notes written on it. Undo brings it back, or restore it from Trash.'
          : 'It leaves the vault’s lists and searches. Undo brings it back, or restore it from Trash.',
        buttons: ['Cancel', 'Move to trash'], cancelId: 0, defaultId: 0
      });
      if (response !== 1) return { deleted: false };
    }
    await store(p).remove(id);
    if (this === window) await recordOwn(p, `fragments/${id}.md`);
    return { deleted: true };
  });
  handle('trash-list', p => store(p).trashList());
  handle('trash-purge', async function (p, id = null, title = '') {
    const single = id != null;
    const name = String(title).trim().slice(0, 80) || 'this fragment';
    const { response } = await dialog.showMessageBox(this, {
      type: 'warning',
      message: single ? `Delete “${name}” permanently?` : `Empty trash in ${label(p)}?`,
      detail: 'This cannot be undone.',
      buttons: ['Cancel', single ? 'Delete permanently' : 'Empty trash'], cancelId: 0, defaultId: 0
    });
    if (response !== 1) return { purged: false };
    const res = await store(p).trashPurge(id);
    return { purged: true, ...res };
  });
  handle('untrash', async function (p, id) {
    const res = await store(p).untrash(id);
    if (this === window && res?.id) await recordOwn(p, `fragments/${res.id}.md`);
    return res;
  });
  handle('doctor', p => store(p).doctor());
  handle('reindex', p => store(p).reindex());
  handle('history', (p, id) => store(p).history(id));
  handle('version', (p, id, timestamp) => store(p).version(id, timestamp));
  handle('restore-version', async function (p, id, timestamp) {
    const res = await store(p).restoreVersion(id, timestamp);
    if (this === window) await recordOwn(p, `fragments/${id}.md`);
    return res;
  });
  handle('schemas', p => store(p).schemas());
  handle('vault-settings', p => store(p).vaultSettings());
  handle('update-vault-settings', async function (p, changes) {
    const res = await store(p).updateVaultSettings(changes);
    if (this === window) await recordOwn(p, 'config.yml');
    return res;
  });
  handle('schema-usage', (p, type) => store(p).schemaUsage(type));
  handle('builtin-schema', (p, type) => store(p).builtinSchema(type));
  handle('save-schema', async function (p, data) {
    const res = await store(p).saveSchema(data);
    if (this === window && res?.schema?.name) await recordOwn(p, `schemas/${res.schema.name}.yml`);
    return res;
  });
  // Asks first; resolves { deleted }. The server refuses a type that still has fragments.
  handle('delete-schema', async function (p, type) {
    const name = String(type).slice(0, 64);
    const { response } = await dialog.showMessageBox(this, {
      message: `Delete the type “${name}”?`, detail: 'No fragment uses it. Its schema file is removed from the vault; a new type can take its name later.',
      buttons: ['Cancel', 'Delete type'], cancelId: 0, defaultId: 0
    });
    if (response !== 1) return { deleted: false };
    const res = await store(p).deleteSchema(type);
    if (this === window && res?.deleted) await recordOwn(p, `schemas/${type}.yml`);
    return res;
  });

  // ── Drafts: unsaved tab text kept on disk (userData/drafts/) instead of bloating localStorage ──
  const drafts = draftStore(path.join(app.getPath('userData'), 'drafts'));
  handle('save-draft', (id, draft) => drafts.save(id, draft));
  handle('load-draft', id => drafts.load(id));
  handle('delete-draft', id => drafts.remove(id));
  handle('prune-drafts', keepIds => drafts.prune(keepIds));
  // Synchronous flush on beforeunload so a reload or quit never loses keystrokes still in the debounce window.
  ipcMain.on('flush-drafts', (event, entries) => {
    if (!trusted(event.sender) || event.senderFrame !== event.sender.mainFrame) { event.returnValue = false; return; }
    try {
      if (Array.isArray(entries)) {
        for (const entry of entries.slice(0, 64)) {
          if (!entry || typeof entry.id !== 'string') continue;
          if (entry.draft == null) drafts.removeSync(entry.id);
          else drafts.saveSync(entry.id, entry.draft);
        }
      }
      event.returnValue = true;
    } catch { event.returnValue = false; }
  });
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
    applyTheme();
    applyShortcut();
    return settingsView();
  });
  // Settings shows a theme as soon as it is picked. Called with nothing, it puts the saved one back (closed without Save).
  handle('preview-theme', theme => applyTheme(theme === undefined ? settings.theme : clean({ theme }).theme));
  handle('system-theme', () => omarchyPayload(effectiveOmarchy()));
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
  // ── The meaning model recall uses. parc-server names its files (pinned URL, size, SHA-256) and the folder it reads
  // them from, so the CLI and every vault's server find what is downloaded here. Only an explicit Download reaches
  // the network, only huggingface.co, and a file is kept only if its hash matches. ──
  let meaningDownload = null;
  const HUGGING_FACE = 'https://huggingface.co/';
  async function meaningView() {
    const known = (await Promise.all([...stores.values()].map(s => s.meaningStatus().catch(() => null)))).filter(Boolean);
    if (!known.length) throw new Error('Recall isn’t available: parc-server didn’t answer.');
    const [{ model, dir, installed, loaded, error }] = known;
    const coverage = known.reduce((sum, s) => ({ embedded: sum.embedded + s.coverage.embedded, total: sum.total + s.coverage.total }),
      { embedded: 0, total: 0 });
    return {
      label: model.label, size: model.size, license: model.license, repo: model.repo, dir, installed, loaded, error, coverage,
      downloading: meaningDownload ? { received: meaningDownload.received, size: model.size } : null
    };
  }
  handle('meaning-model', async () => { await ready(); return meaningView(); });
  handle('download-meaning-model', async () => {
    await ready();
    if (meaningDownload) throw new Error('The meaning model is already downloading.');
    const [status] = (await Promise.all([...stores.values()].map(s => s.meaningStatus().catch(() => null)))).filter(Boolean);
    if (!status?.dir) throw new Error('This computer has no folder for models (no home folder).');
    const files = status.model.files;
    if (!files.every(f => typeof f.url === 'string' && f.url.startsWith(HUGGING_FACE) && /^[a-f0-9]{64}$/.test(f.sha256)
      && /^[\w.-]+$/.test(f.name) && Number.isSafeInteger(f.size) && f.size > 0)) throw new Error('Unexpected model files.');
    const controller = new AbortController(), total = status.model.size;
    meaningDownload = { controller, received: 0 };
    const notify = data => window?.webContents.send('meaning-progress', { size: total, ...data });
    let last = 0, before = 0;
    try {
      for (const f of files) {
        await models.download({ file: f.name, size: f.size, sha256: f.sha256, url: f.url }, {
          dir: status.dir, signal: controller.signal, fetch: (url, init) => net.fetch(url, init),
          onProgress: received => {
            meaningDownload.received = before + received;
            if (Date.now() - last > 200) { last = Date.now(); notify({ received: meaningDownload.received }); }
          }
        });
        before += f.size;
      }
      // Any request wakes a vault's server to read its notes with the new model.
      return { installed: true, view: await meaningView() };
    } catch (err) {
      if (controller.signal.aborted) return { cancelled: true };
      throw err;
    } finally {
      meaningDownload = null;
      notify({ done: true });
    }
  });
  handle('cancel-meaning-download', () => { meaningDownload?.controller.abort(); });
  handle('delete-meaning-model', async function () {
    await ready();
    if (meaningDownload) throw new Error('Cancel the download first.');
    const [status] = (await Promise.all([...stores.values()].map(s => s.meaningStatus().catch(() => null)))).filter(Boolean);
    if (!status?.dir) return { deleted: false };
    const { response } = await dialog.showMessageBox(this, {
      message: `Delete the “${status.model.label}” meaning model?`,
      detail: `This frees ${Math.round(status.model.size / 1e6)} MB. Recall goes back to shared words, tags and links once parc restarts. You can download it again later.`,
      buttons: ['Cancel', 'Delete'], cancelId: 0, defaultId: 0
    });
    if (response !== 1) return { deleted: false };
    for (const f of status.model.files) await models.remove({ file: f.name }, status.dir);
    return { deleted: true };
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

  // ── Agent: Sign in with ChatGPT and the model loop (agent.cjs) ──
  // Only Continue with ChatGPT and a question asked in the Agent view reach the network. The credential is sealed
  // with the system keyring; where there is none (safeStorage falls back to a fixed key), it isn't written at all.
  const agentFile = path.join(app.getPath('userData'), 'agent.json');      // this installation's device ID and model
  const credentialFile = path.join(app.getPath('userData'), 'chatgpt.bin'); // the sealed sign-in
  const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
  let agentPrefs = null;
  async function savePrefs() {
    await fs.writeFile(agentFile + '.tmp', JSON.stringify(agentPrefs, null, 2), { mode: 0o600 });
    await fs.rename(agentFile + '.tmp', agentFile);
  }
  async function prefs() {
    if (agentPrefs) return agentPrefs;
    try { agentPrefs = JSON.parse(await fs.readFile(agentFile, 'utf8')); } catch { agentPrefs = {}; }
    if (!agentPrefs || typeof agentPrefs !== 'object') agentPrefs = {};
    if (typeof agentPrefs.deviceId !== 'string' || !UUID.test(agentPrefs.deviceId)) { agentPrefs.deviceId = randomUUID(); await savePrefs(); }
    return agentPrefs;
  }
  const keyring = {
    available: async () => await safeStorage.isAsyncEncryptionAvailable()
      && !(process.platform === 'linux' && ['basic_text', 'unknown'].includes(safeStorage.getSelectedStorageBackend())),
    seal: text => safeStorage.encryptStringAsync(text),
    unseal: async data => (await safeStorage.decryptStringAsync(data)).result
  };
  const auth = agent.chatgpt({
    file: credentialFile,
    keyring,
    deviceId: () => agentPrefs.deviceId,
    open: url => {
      if (!url.startsWith(`${agent.ENDPOINTS.authorize}?`)) throw new Error('Unexpected sign-in address.');
      return shell.openExternal(url);
    },
    fetch: (url, init) => net.fetch(url, init)
  });
  const toolServers = new Map();
  const toolServer = p => {
    if (!toolServers.has(p)) toolServers.set(p, agent.mcpServer(binary('parc-server'), ['--vault', p, '--mcp', '--read-only']));
    return toolServers.get(p);
  };
  // GitHub Copilot: sign-in with GitHub (device flow), and Copilot's runtime with its data in parc's user data.
  // GitHub's code page: opened at sign-in, and again from the Agent view; only ever a page on GitHub's own host.
  let devicePage = null;
  const openDevicePage = url => {
    const u = new URL(url);
    const github = new URL(copilot.GITHUB.device);
    if (u.protocol !== github.protocol || u.host !== github.host) throw new Error('Unexpected sign-in address.');
    devicePage = u.toString();
    return shell.openExternal(devicePage);
  };
  const gitHub = copilot.github({
    file: path.join(app.getPath('userData'), 'github.bin'), keyring, fetch: (url, init) => net.fetch(url, init), open: openDevicePage
  });
  const copilotRuntime = copilot.copilotRuntime({
    auth: gitHub, sdk: () => require('@microsoft/copilot-sdk'),
    home: path.join(app.getPath('userData'), 'copilot'), work: path.join(app.getPath('userData'), 'copilot', 'work')
  });
  const PROVIDERS = ['chatgpt', 'copilot'];
  const bot = agent.harness({
    auth, server: toolServer, read: (p, id) => store(p).get(id), fetch: (url, init) => net.fetch(url, init),
    settings: { load: prefs, save: async changes => { Object.assign(await prefs(), changes); await savePrefs(); } },
    store: agent.chatStore(path.join(app.getPath('userData'), 'chats')), // saved chats, on this computer only
    copilot: copilotRuntime
  });
  const chatsChanged = () => window?.webContents.send('agent-chats-changed');
  // Errors the Agent view acts on (signed out, cancelled) come back as values: a thrown error loses its fields.
  const outcome = err => ({ error: err.message, signedOut: !!err.signedOut, cancelled: !!err.cancelled });
  const chatId = id => { if (typeof id !== 'string' || !/^[A-Za-z0-9_-]{1,64}$/.test(id)) throw new Error('Invalid chat'); return id; };
  const provider = p => { if (!PROVIDERS.includes(p)) throw new Error('Unknown provider'); return p; };
  // Both accounts, and the provider new chats start on.
  handle('agent-status', async () => {
    const [chatgpt, copilot] = await Promise.all([auth.status(), gitHub.status()]);
    return { chatgpt, copilot, provider: PROVIDERS.includes((await prefs()).provider) ? agentPrefs.provider : 'chatgpt' };
  });
  handle('agent-set-provider', async p => { (await prefs()).provider = provider(p); await savePrefs(); });
  // The code to type at GitHub goes to the window as soon as GitHub gives it; the call resolves once signed in.
  handle('copilot-sign-in', async function () {
    const sender = this?.webContents;
    try {
      const s = await gitHub.signIn(code => { if (!sender?.isDestroyed()) sender.send('copilot-code', code); });
      this?.focus();
      return s;
    } catch (err) { this?.focus(); return outcome(err); }
  });
  handle('copilot-cancel-sign-in', () => gitHub.cancel());
  handle('copilot-open', () => devicePage && shell.openExternal(devicePage));
  handle('copilot-sign-out', async () => { await copilotRuntime.signedOut(); return gitHub.signOut(); });
  handle('agent-sign-in', async function () {
    await prefs();
    try { const s = await auth.signIn(); this?.focus(); return s; } catch (err) { this?.focus(); return outcome(err); }
  });
  handle('agent-cancel-sign-in', () => auth.cancel());
  handle('agent-sign-out', async () => { bot.signedOut(); return auth.signOut(); });
  handle('agent-models', async (force, p = 'chatgpt') => {
    provider(p);
    try { return { models: await bot.models(force === true, p), model: await bot.model(p) }; } catch (err) { return outcome(err); }
  });
  handle('agent-set-model', (id, p = 'chatgpt') => bot.setModel(String(id), provider(p)));
  // One question. vaults: the opened vaults in view, for a new chat (a saved chat keeps its own); subject: { vault, id }
  // of the fragment the user has open, or null.
  handle('agent-ask', async function (chat, { text, vaults: paths, subject, provider: wanted = 'chatgpt' } = {}) {
    chatId(chat);
    const on = (await bot.providerOf(chat)) || provider(wanted);
    if (typeof text !== 'string' || !text.trim() || text.length > 20000) throw new Error('Ask in up to 20,000 characters.');
    paths = (await bot.vaultsOf(chat)) || paths;
    if (!Array.isArray(paths) || !paths.length || paths.length > 32) throw new Error('Choose the vaults to ask about.');
    const gone = paths.find(p => !stores.has(p));
    if (gone) return { error: `This chat searches ${gone}, which isn’t open in parc any more. Open that vault to go on.` };
    const named = vaults(), scope = paths.map(p => { store(p); return { path: p, name: named.find(v => v.path === p).name }; });
    if (subject != null) {
      if (!scope.some(v => v.path === subject.vault) || typeof subject.id !== 'string' || !/^[0-9A-HJKMNP-TV-Z]{26}$/.test(subject.id)) throw new Error('Invalid fragment');
      subject = { vault: subject.vault, id: subject.id };
    }
    const sender = this?.webContents;
    try {
      const summary = await bot.ask(chat, { text: text.trim(), vaults: scope, subject: subject || null, provider: on }, event => { if (!sender?.isDestroyed()) sender.send('agent-event', { chat, ...event }); });
      chatsChanged();
      return { ok: true, chat: summary };
    } catch (err) { return outcome(err); }
  });
  handle('agent-stop', chat => bot.stop(chatId(chat)));
  handle('agent-forget', chat => bot.forget(chatId(chat)));
  handle('agent-analyze', async function ({ vault: vp, id, filename = null } = {}) {
    await ready();
    if (!stores.has(vp) || !ULID_RE.test(id)) throw new Error('Invalid fragment');
    const [cg, gh, p] = await Promise.all([auth.status(), gitHub.status(), prefs()]);
    const pref = PROVIDERS.includes(p.provider) ? p.provider : 'chatgpt';
    const on = (pref === 'copilot' && gh.signedIn) ? 'copilot' : cg.signedIn ? 'chatgpt' : gh.signedIn ? 'copilot' : null;
    if (!on) return { error: 'Sign in with ChatGPT or GitHub Copilot in the Agent view first.', signedOut: true };
    const named = vaults().find(v => v.path === vp);
    try {
      return await bot.analyze({ vault: { path: vp, name: named?.name || label(vp) }, id, filename, provider: on });
    } catch (err) { return outcome(err); }
  });
  handle('agent-analyze-stop', key => bot.stopAnalyze(String(key || '')));
  handle('agent-chats', () => bot.chats());
  handle('agent-chat', id => bot.chat(chatId(id)));
  handle('agent-delete-chat', async id => { await bot.remove(chatId(id)); chatsChanged(); });
  handle('agent-restore-chat', async id => { await bot.restore(chatId(id)); chatsChanged(); });
  // The workspace starts (or reloads) with no chats: whatever main still holds belongs to a page that is gone.
  handle('agent-reset', () => bot.reset());

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
