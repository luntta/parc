// Follow Omarchy's active desktop theme live: colors.toml, shell.toml, Hyprland rounding,
// and Glassy transparency toggles. Pure Node (no Electron), so unit tests run directly against
// real stock themes and synthetic temp state directories.
const fs = require('node:fs');
const fsp = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const { execFile } = require('node:child_process');

const HEX_RE = /^#([0-9a-f]{6})$/i;
const COLOR_KEYS = [
  'accent', 'selection', 'muted',
  'background', 'dark_background', 'darker_background', 'lighter_background',
  'foreground', 'dark_foreground', 'light_foreground', 'bright_foreground',
  'red', 'yellow', 'orange', 'green', 'cyan', 'blue', 'magenta', 'brown',
  'bright_red', 'bright_yellow', 'bright_green', 'bright_cyan', 'bright_blue', 'bright_magenta'
];

// ── Color math (WCAG 2.1 luminance & contrast) ──
function parseHex(hex) {
  if (typeof hex !== 'string') return null;
  const m = HEX_RE.exec(hex.trim());
  if (!m) return null;
  const n = parseInt(m[1], 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}
function toHex([r, g, b]) {
  const clamp = v => Math.max(0, Math.min(255, Math.round(v))).toString(16).padStart(2, '0');
  return `#${clamp(r)}${clamp(g)}${clamp(b)}`;
}
function luminance(hex) {
  const rgb = typeof hex === 'string' ? parseHex(hex) : hex;
  if (!rgb) return 0;
  const lin = rgb.map(c => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2];
}
function contrast(a, b) {
  const la = luminance(a), lb = luminance(b);
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}
function mixHex(a, b, t) {
  const ca = parseHex(a), cb = parseHex(b);
  if (!ca) return b;
  if (!cb) return a;
  return toHex([ca[0] * t + cb[0] * (1 - t), ca[1] * t + cb[1] * (1 - t), ca[2] * t + cb[2] * (1 - t)]);
}
function withAlpha(hex, alpha) {
  if (alpha >= 0.999) return hex;
  const rgb = parseHex(hex);
  if (!rgb) return hex;
  const a = Math.max(0, Math.min(1, alpha));
  return `rgba(${rgb[0]}, ${rgb[1]}, ${rgb[2]}, ${Number(a.toFixed(3))})`;
}
function bestTextOn(bg, light = '#ffffff', dark = '#000000') {
  return contrast(light, bg) >= contrast(dark, bg) ? light : dark;
}
/** Preserve `fg`'s hue while nudging it toward `fallback` only if needed to meet `minRatio` on `bg`. */
function ensureContrast(fg, bg, minRatio, fallback) {
  if (!parseHex(fg)) return fallback;
  if (contrast(fg, bg) >= minRatio) return fg;
  const target = contrast(fallback, bg) >= contrast('#ffffff', bg) && contrast(fallback, bg) >= contrast('#000000', bg)
    ? fallback
    : contrast(fallback, bg) >= minRatio ? fallback : bestTextOn(bg, '#ffffff', '#000000');
  if (contrast(target, bg) < minRatio) return target;
  let lo = 0, hi = 1;
  for (let i = 0; i < 10; i++) {
    const mid = (lo + hi) / 2;
    if (contrast(mixHex(target, fg, mid), bg) >= minRatio) hi = mid; else lo = mid;
  }
  return mixHex(target, fg, hi);
}

// ── TOML parsers (narrow, zero-dependency, safe on arbitrary comments/whitespace) ──
function stripComment(line) {
  let quote = null;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (quote) { if (ch === quote && line[i - 1] !== '\\') quote = null; }
    else if (ch === '"' || ch === "'") quote = ch;
    else if (ch === '#') return line.slice(0, i);
  }
  return line;
}
function parseColorsToml(text) {
  if (typeof text !== 'string' || !text.trim()) return null;
  const raw = {};
  for (const line of text.split(/\r?\n/)) {
    const clean = stripComment(line).trim();
    if (!clean || clean.startsWith('[')) continue;
    const eq = clean.indexOf('=');
    if (eq < 1) continue;
    const key = clean.slice(0, eq).trim();
    let val = clean.slice(eq + 1).trim();
    if ((val.startsWith('"') && val.endsWith('"')) || (val.startsWith("'") && val.endsWith("'"))) {
      val = val.slice(1, -1).trim();
    }
    raw[key] = val;
  }
  const background = parseHex(raw.background) ? raw.background.toLowerCase() : null;
  const foreground = parseHex(raw.foreground) ? raw.foreground.toLowerCase() : null;
  if (!background || !foreground) return null;

  const palette = { background, foreground };
  for (const k of COLOR_KEYS) {
    if (k === 'background' || k === 'foreground') continue;
    if (parseHex(raw[k])) palette[k] = raw[k].toLowerCase();
  }
  const declared = raw.mode === 'light' || raw.mode === 'dark' ? raw.mode : null;
  palette.mode = declared || (luminance(background) > 0.35 ? 'light' : 'dark');
  return palette;
}
/** Extract `[bar] background-alpha` from Omarchy's `shell.toml` if a theme defines translucency. */
function parseShellAlpha(text) {
  if (typeof text !== 'string') return null;
  let section = '';
  for (const line of text.split(/\r?\n/)) {
    const clean = stripComment(line).trim();
    if (!clean) continue;
    const hdr = /^\[([^\]]+)\]$/.exec(clean);
    if (hdr) { section = hdr[1].trim(); continue; }
    if (section !== 'bar') continue;
    const m = /^background-alpha\s*=\s*([0-9]*\.?[0-9]+)$/.exec(clean);
    if (m) {
      const v = Number(m[1]);
      if (Number.isFinite(v) && v >= 0 && v <= 1) return v;
    }
  }
  return null;
}

// Exact Porter-Duff source-over incremental alpha so compositing `main` over `body` hits `target`
// instead of double-darkening (`a_body + a_layer * (1 - a_body) = target`).
function layerAlpha(target, base) {
  if (target >= 0.999) return 1;
  if (base >= 0.999) return 1;
  return Math.max(0.05, Math.min(1, (target - base) / (1 - base)));
}

// ── Semantic token synthesis for parc-desktop + @luntta/aihio ──
function buildTokens(palette, { rounding = null, opacity = 'opaque', glass = false, shellAlpha = null } = {}) {
  const opacityMode = opacity === 'glassy' || opacity === 'transparent' || opacity === 'opaque'
    ? opacity : (glass ? 'glassy' : 'opaque');
  const dark = palette.mode === 'dark';
  const bg = palette.background;
  const fg = palette.foreground;
  const darkBg = palette.dark_background || mixHex('#000000', bg, dark ? 0.25 : 0.05);
  const lighterBg = palette.lighter_background || mixHex('#ffffff', bg, dark ? 0.1 : 0.05);

  // Desk vs work panel: the panel is the lighter surface in both modes, matching parc's layered chrome.
  let deskBg, panelBg, raisedBg, overlayBg, overlayHighlightBg;
  if (dark) {
    deskBg = luminance(darkBg) < luminance(bg) ? darkBg : bg;
    panelBg = luminance(bg) > luminance(deskBg) + 0.002 ? bg
      : luminance(darkBg) > luminance(bg) ? darkBg : mixHex(fg, bg, 0.06);
    raisedBg = luminance(lighterBg) > luminance(panelBg) + 0.005 ? lighterBg : mixHex(fg, panelBg, 0.08);
    overlayBg = mixHex(raisedBg, panelBg, 0.72);
    overlayHighlightBg = palette.selection && contrast(palette.selection, overlayBg) > 1.12
      ? palette.selection
      : mixHex(fg, overlayBg, 0.14);
  } else {
    panelBg = bg;
    deskBg = luminance(darkBg) < luminance(bg) - 0.004 ? darkBg : mixHex('#000000', bg, 0.05);
    raisedBg = deskBg;
    overlayBg = panelBg;
    overlayHighlightBg = palette.selection
      ? mixHex(palette.selection, overlayBg, contrast(palette.selection, overlayBg) > 1.5 ? 0.45 : 0.85)
      : mixHex(fg, overlayBg, 0.07);
  }

  const accent = palette.accent || palette.blue || fg;
  const primaryFg = bestTextOn(accent, '#ffffff', '#000000');
  const focusRing = ensureContrast(accent, panelBg, 3.0, fg);

  const mutedSeed = palette.dark_foreground || palette.muted || mixHex(fg, panelBg, 0.65);
  const mutedFg = ensureContrast(mutedSeed, panelBg, 4.5, fg);
  const borderSubtle = mixHex(palette.muted || fg, panelBg, dark ? 0.38 : 0.30);
  const fieldBorder = ensureContrast(palette.muted || mixHex(fg, panelBg, 0.45), panelBg, 2.5, fg);
  const controlHover = mixHex(fg, panelBg, dark ? 0.10 : 0.07);
  const selectionBg = withAlpha(palette.selection || accent, dark ? 0.48 : 0.38);

  const red = palette.red || (dark ? '#f87171' : '#dc2626');
  const green = palette.green || (dark ? '#4ade80' : '#16a34a');
  const yellow = palette.yellow || palette.orange || (dark ? '#fbbf24' : '#d97706');
  const destructiveText = ensureContrast(palette.bright_red || red, panelBg, 4.5, dark ? '#ff8a8a' : '#b91c1c');
  const successText = ensureContrast(palette.bright_green || green, panelBg, 4.5, dark ? '#86efac' : '#15803d');
  const warningText = ensureContrast(palette.orange || yellow, panelBg, 4.5, dark ? '#fcd34d' : '#b45309');

  // Follow Omarchy's three Window Opacity modes (opaque / transparent / glassy) while keeping text 100% opaque.
  // Because `main` composites over `body`, its CSS alpha is the exact incremental lift to reach `panelTarget`.
  let deskAlpha = 1, panelTarget = 1, panelAlpha = 1, raisedLayerAlpha = 1, surfaceAlpha = 1, overlayAlpha = 1;
  if (opacityMode === 'glassy') {
    deskAlpha = shellAlpha != null && shellAlpha < 0.98 ? Math.max(0.58, Math.min(0.82, shellAlpha)) : 0.65;
    panelTarget = Math.min(0.86, +(deskAlpha + 0.13).toFixed(2));
    panelAlpha = layerAlpha(panelTarget, deskAlpha);
    raisedLayerAlpha = 0.42;
    surfaceAlpha = 0.94;
    overlayAlpha = 0.95;
  } else if (opacityMode === 'transparent') {
    deskAlpha = 0.90;
    panelTarget = 0.95;
    panelAlpha = layerAlpha(panelTarget, deskAlpha);
    raisedLayerAlpha = 0.65;
    surfaceAlpha = 0.97;
    overlayAlpha = 0.97;
  }

  const tokens = {
    '--desk': withAlpha(deskBg, deskAlpha),
    '--panel': withAlpha(panelBg, panelAlpha),
    '--panel-raised': withAlpha(raisedBg, raisedLayerAlpha),
    '--quick-bg': withAlpha(panelBg, panelTarget),
    '--panel-edge': withAlpha(palette.muted || fg, dark ? 0.24 : 0.18),
    '--panel-ring': dark ? 'rgba(0, 0, 0, 0.34)' : 'transparent',
    '--panel-shadow': dark ? '0 10px 30px -14px rgba(0, 0, 0, 0.65)' : '0 1px 2px rgba(0, 0, 0, 0.05), 0 3px 14px -6px rgba(0, 0, 0, 0.09)',
    '--hairline': withAlpha(fg, dark ? 0.09 : 0.08),
    '--omarchy-accent': accent,
    '--omarchy-selection': selectionBg,

    '--aihio-color-page-bg': withAlpha(panelBg, panelTarget),
    '--aihio-color-page-fg': fg,
    '--aihio-color-surface-bg': withAlpha(raisedBg, surfaceAlpha),
    '--aihio-color-surface-fg': fg,
    '--aihio-color-overlay-bg': withAlpha(overlayBg, overlayAlpha),
    '--aihio-color-overlay-fg': fg,
    '--aihio-color-overlay-highlight-bg': overlayHighlightBg,
    '--aihio-color-muted-bg': withAlpha(raisedBg, surfaceAlpha),
    '--aihio-color-muted-fg': mutedFg,
    '--aihio-color-placeholder-bg': raisedBg,
    '--aihio-color-placeholder-fg': mutedFg,
    '--aihio-color-primary-action-bg': accent,
    '--aihio-color-primary-action-fg': primaryFg,
    '--aihio-color-secondary-action-bg': raisedBg,
    '--aihio-color-secondary-action-fg': fg,
    '--aihio-color-control-highlight-bg': controlHover,
    '--aihio-color-control-highlight-fg': fg,
    '--aihio-color-destructive-bg': red,
    '--aihio-color-destructive-fg': bestTextOn(red, '#ffffff', '#000000'),
    '--aihio-color-destructive-text': destructiveText,
    '--aihio-color-success-bg': green,
    '--aihio-color-success-fg': bestTextOn(green, '#ffffff', '#000000'),
    '--aihio-color-success-text': successText,
    '--aihio-color-warning-bg': yellow,
    '--aihio-color-warning-fg': bestTextOn(yellow, '#ffffff', '#000000'),
    '--aihio-color-warning-text': warningText,
    '--aihio-color-border-subtle': borderSubtle,
    '--aihio-color-field-border': fieldBorder,
    '--aihio-color-focus-ring': focusRing
  };

  if (typeof rounding === 'number' && Number.isFinite(rounding) && rounding >= 0) {
    const r = Math.min(24, Math.round(rounding));
    const sm = r === 0 ? 0 : Math.max(2, Math.min(4, Math.round(r * 0.5)));
    const md = r === 0 ? 0 : Math.max(2, Math.min(r, Math.round(r * 0.75)));
    const xl = r === 0 ? 0 : Math.min(14, r + 2);
    Object.assign(tokens, {
      '--aihio-radius-base': `${r}px`,
      '--aihio-radius-sm': `${sm}px`,
      '--aihio-radius-md': `${md}px`,
      '--aihio-radius-lg': `${r}px`,
      '--aihio-radius-xl': `${xl}px`,
      '--aihio-radius-surface': `${r}px`,
      '--aihio-radius-interactive': `${md}px`,
      '--aihio-radius-interactive-compact': `${sm}px`
    });
  }

  return { tokens, canvas: deskBg, panelBg };
}

function defaultStateDir(env = process.env) {
  if (env.PARC_OMARCHY_STATE) return env.PARC_OMARCHY_STATE;
  const xdg = env.XDG_STATE_HOME || path.join(os.homedir(), '.local', 'state');
  return path.join(xdg, 'omarchy');
}

function queryHyprInt(hyprctl, option) {
  if (!hyprctl) return Promise.resolve(null);
  return new Promise(resolve => {
    execFile(hyprctl, ['-j', 'getoption', option], { timeout: 500, maxBuffer: 65536 }, (err, stdout) => {
      if (err) return resolve(null);
      try {
        const parsed = JSON.parse(stdout);
        resolve(typeof parsed?.int === 'number' && Number.isInteger(parsed.int) ? parsed.int : null);
      } catch { resolve(null); }
    });
  });
}

function readOptionalSync(filePath) {
  try { return fs.readFileSync(filePath, 'utf8'); } catch { return null; }
}
async function readOptional(filePath) {
  try { return await fsp.readFile(filePath, 'utf8'); } catch { return null; }
}
function existsSyncSafe(filePath) {
  try { return fs.existsSync(filePath); } catch { return false; }
}
async function existsSafe(filePath) {
  try { await fsp.access(filePath); return true; } catch { return false; }
}

/**
 * Watches Omarchy's state directory (`current/` + `toggles/hypr/`) and queries Hyprland's live
 * rounding when available. Calls `onChange(snapshot)` whenever the effective theme changes.
 */
function omarchyTheme({
  stateDir = defaultStateDir(),
  hyprctl = process.env.HYPRLAND_INSTANCE_SIGNATURE ? 'hyprctl' : null,
  onChange = () => {}
} = {}) {
  const currentDir = path.join(stateDir, 'current');
  const themeDir = path.join(currentDir, 'theme');
  const colorsFile = path.join(themeDir, 'colors.toml');
  const shellFile = path.join(themeDir, 'shell.toml');
  const nameFile = path.join(currentDir, 'theme.name');
  const togglesDir = path.join(stateDir, 'toggles', 'hypr');

  let snapshot = null;
  let lastSig = '';
  let liveRounding = null;
  let closed = false;
  let debounceTimer = 0;
  let watchers = [];

  function assemble(colorsText, shellText, nameText, { glassyToggle, opaqueToggle, roundedToggle, sharpToggle, rounding }) {
    const palette = parseColorsToml(colorsText);
    if (!palette) return null;
    const shellAlpha = parseShellAlpha(shellText);
    // Matches `omarchy-hyprland-window-transparency`: window-opaque.lua → opaque,
    // window-glassy.lua (or translucent shell.toml) → glassy, neither → transparent (Omarchy's default).
    const opacity = opaqueToggle ? 'opaque'
      : (glassyToggle || (shellAlpha != null && shellAlpha < 0.98)) ? 'glassy'
      : 'transparent';
    const glass = opacity === 'glassy';
    const effectiveRounding = rounding != null ? rounding
      : roundedToggle ? 6
      : sharpToggle ? 0
      : null;
    const { tokens, canvas } = buildTokens(palette, { rounding: effectiveRounding, opacity, shellAlpha });
    const name = (nameText || '').trim() || 'Omarchy';
    return { name, mode: palette.mode, opacity, glass, rounding: effectiveRounding, canvas, tokens };
  }

  function loadSync() {
    const next = assemble(
      readOptionalSync(colorsFile),
      readOptionalSync(shellFile),
      readOptionalSync(nameFile),
      {
        glassyToggle: existsSyncSafe(path.join(togglesDir, 'window-glassy.lua')),
        opaqueToggle: existsSyncSafe(path.join(togglesDir, 'window-opaque.lua')),
        roundedToggle: existsSyncSafe(path.join(togglesDir, 'corners-rounded.lua')),
        sharpToggle: existsSyncSafe(path.join(togglesDir, 'corners-sharp.lua')),
        rounding: liveRounding
      }
    );
    snapshot = next;
    lastSig = JSON.stringify(next);
    return next;
  }

  async function reload({ queryDesktop = true } = {}) {
    if (closed) return snapshot;
    const [colorsText, shellText, nameText, glassyToggle, opaqueToggle, roundedToggle, sharpToggle, queriedRounding] = await Promise.all([
      readOptional(colorsFile),
      readOptional(shellFile),
      readOptional(nameFile),
      existsSafe(path.join(togglesDir, 'window-glassy.lua')),
      existsSafe(path.join(togglesDir, 'window-opaque.lua')),
      existsSafe(path.join(togglesDir, 'corners-rounded.lua')),
      existsSafe(path.join(togglesDir, 'corners-sharp.lua')),
      queryDesktop ? queryHyprInt(hyprctl, 'decoration:rounding') : Promise.resolve(liveRounding)
    ]);
    if (closed) return snapshot;
    if (queriedRounding != null && queriedRounding >= 0) liveRounding = queriedRounding;
    const next = assemble(colorsText, shellText, nameText, {
      glassyToggle, opaqueToggle, roundedToggle, sharpToggle, rounding: liveRounding
    });
    const sig = JSON.stringify(next);
    snapshot = next;
    if (sig !== lastSig) {
      lastSig = sig;
      onChange(next);
    }
    return next;
  }

  function scheduleReload(rearm = false) {
    if (closed) return;
    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      if (closed) return;
      if (rearm) armWatchers();
      reload({ queryDesktop: true }).catch(() => {});
    }, 80);
  }

  function closeWatchers() {
    for (const w of watchers) { try { w.close(); } catch { /* ignore */ } }
    watchers = [];
  }

  function armWatchers() {
    closeWatchers();
    if (closed) return;
    // Watching current/ survives `rm -rf theme && mv next theme`; watching theme/ catches in-place glass edits;
    // watching stateDir/toggles catches toggles/hypr being created later.
    const togglesParent = path.join(stateDir, 'toggles');
    const targets = [
      [currentDir, true],
      [themeDir, false],
      [togglesDir, false],
      ...(!existsSyncSafe(togglesDir) && existsSyncSafe(togglesParent) ? [[togglesParent, true]] : []),
      ...(!existsSyncSafe(currentDir) && existsSyncSafe(stateDir) ? [[stateDir, true]] : [])
    ];
    for (const [dir, rearmOnChange] of targets) {
      if (!existsSyncSafe(dir)) continue;
      try {
        const w = fs.watch(dir, () => scheduleReload(rearmOnChange));
        w.on('error', () => {});
        watchers.push(w);
      } catch { /* optional */ }
    }
  }

  function start() {
    loadSync();
    armWatchers();
    // Pick up live Hyprland rounding asynchronously right after startup without blocking first paint.
    if (hyprctl) reload({ queryDesktop: true }).catch(() => {});
    return snapshot;
  }

  return {
    start,
    loadSync,
    reload,
    refreshDesktop: () => reload({ queryDesktop: true }),
    current: () => snapshot,
    available: () => snapshot !== null,
    close() { closed = true; clearTimeout(debounceTimer); closeWatchers(); }
  };
}

module.exports = {
  parseHex, luminance, contrast, mixHex, withAlpha, bestTextOn, ensureContrast,
  parseColorsToml, parseShellAlpha, buildTokens, defaultStateDir, omarchyTheme
};
