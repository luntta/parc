const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const fsp = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const {
  contrast, parseColorsToml, parseShellAlpha, buildTokens, omarchyTheme
} = require('../omarchy.cjs');

const TOKYO_NIGHT = `
mode = "dark"
accent = "#7aa2f7"
selection = "#292e42"
muted = "#414868"
background = "#1a1b26"
dark_background = "#13141c"
darker_background = "#0e0e14"
lighter_background = "#24283b"
foreground = "#a9b1d6"
dark_foreground = "#565f89"
red = "#f7768e"
yellow = "#e0af68"
green = "#9ece6a"
`;

const CATPPUCCIN_LATTE = `
mode = "light"
accent = "#1e66f5"
selection = "#ccd0da"
muted = "#acb0be"
background = "#eff1f5"
dark_background = "#e3e4e8"
darker_background = "#d7d8dc"
lighter_background = "#dce0e8"
foreground = "#4c4f69"
dark_foreground = "#9ca0b0"
red = "#d20f39"
yellow = "#df8e1d"
green = "#40a02b"
`;

test('parseColorsToml parses quotes, strips comments, infers missing mode, and rejects incomplete themes', () => {
  const dark = parseColorsToml(`# comment\nbackground = "#101315" # inline\nforeground = '#cacccc'\naccent = "#798186"`);
  assert.equal(dark.mode, 'dark');
  assert.equal(dark.background, '#101315');
  assert.equal(dark.foreground, '#cacccc');
  assert.equal(dark.accent, '#798186');

  const light = parseColorsToml(`background = "#FFFCF0"\nforeground = "#100F0F"`);
  assert.equal(light.mode, 'light');
  assert.equal(light.background, '#fffcf0');

  assert.equal(parseColorsToml('accent = "#ff0000"'), null);
  assert.equal(parseColorsToml('background = "not-a-hex"\nforeground = "#ffffff"'), null);
  assert.equal(parseShellAlpha('[bar]\nbackground-alpha = 0.72\n[menu]\nbackground-alpha = 1.0'), 0.72);
  assert.equal(parseShellAlpha('[menu]\nbackground-alpha = 0.5'), null);
});

test('buildTokens preserves WCAG contrast across all stock Omarchy themes and extreme palettes', () => {
  const samples = [
    ['tokyo-night', TOKYO_NIGHT],
    ['catppuccin-latte', CATPPUCCIN_LATTE],
    ['vantablack', `mode = "dark"\naccent = "#8d8d8d"\nbackground = "#000000"\ndark_background = "#090909"\nlighter_background = "#1a1a1a"\nforeground = "#ffffff"\ndark_foreground = "#505050"\nmuted = "#7a7a7a"\nred = "#a4a4a4"\ngreen = "#b6b6b6"\nyellow = "#cecece"`],
    ['white', `mode = "light"\naccent = "#6e6e6e"\nselection = "#c0c0c0"\nmuted = "#808080"\nbackground = "#ffffff"\ndark_background = "#f5f5f5"\nforeground = "#000000"\ndark_foreground = "#c0c0c0"\nred = "#2a2a2a"\ngreen = "#3a3a3a"\nyellow = "#4a4a4a"`],
    ['solitude', `mode = "dark"\naccent = "#798186"\nmuted = "#4b4e55"\nbackground = "#101315"\ndark_background = "#0c0e10"\nlighter_background = "#101315"\nforeground = "#cacccc"\ndark_foreground = "#4b4e55"\nred = "#565d60"\ngreen = "#9fa5a9"\nyellow = "#d9dbdc"`]
  ];
  // Also test every real stock theme if Omarchy is installed on this host.
  const stockDir = '/usr/share/omarchy/themes';
  if (fs.existsSync(stockDir)) {
    for (const entry of fs.readdirSync(stockDir)) {
      const file = path.join(stockDir, entry, 'colors.toml');
      if (fs.existsSync(file)) samples.push([`stock:${entry}`, fs.readFileSync(file, 'utf8')]);
    }
  }

  for (const [name, text] of samples) {
    const palette = parseColorsToml(text);
    assert.ok(palette, `${name} should parse`);
    const { tokens } = buildTokens(palette);
    const panel = tokens['--panel'];
    assert.ok(contrast(tokens['--aihio-color-primary-action-fg'], tokens['--aihio-color-primary-action-bg']) >= 4.5, `${name}: primary button contrast`);
    assert.ok(contrast(tokens['--aihio-color-muted-fg'], panel) >= 4.45, `${name}: muted text contrast`);
    assert.ok(contrast(tokens['--aihio-color-destructive-text'], panel) >= 4.45, `${name}: destructive text contrast`);
    assert.ok(contrast(tokens['--aihio-color-success-text'], panel) >= 4.45, `${name}: success text contrast`);
    assert.ok(contrast(tokens['--aihio-color-warning-text'], panel) >= 4.45, `${name}: warning text contrast`);
    assert.ok(contrast(tokens['--aihio-color-focus-ring'], panel) >= 2.95, `${name}: focus ring contrast`);
    assert.notEqual(tokens['--desk'], tokens['--panel'], `${name}: desk and panel remain layered`);
    assert.equal(tokens['--aihio-radius-base'], undefined, `${name}: no radius override when rounding is null`);
  }
});

test('buildTokens applies Hyprland rounding and all three Omarchy opacity modes without fading text', () => {
  const palette = parseColorsToml(TOKYO_NIGHT);

  const sharp = buildTokens(palette, { rounding: 0, opacity: 'opaque' });
  assert.equal(sharp.tokens['--aihio-radius-base'], '0px');
  assert.equal(sharp.tokens['--aihio-radius-surface'], '0px');
  assert.equal(sharp.tokens['--aihio-radius-interactive'], '0px');
  assert.equal(sharp.tokens['--aihio-radius-xl'], '0px');
  assert.match(sharp.tokens['--desk'], /^#[0-9a-f]{6}$/);

  const subtle = buildTokens(palette, { rounding: 0, opacity: 'transparent' });
  assert.equal(subtle.tokens['--desk'], 'rgba(19, 20, 28, 0.9)');
  assert.equal(subtle.tokens['--panel'], 'rgba(26, 27, 38, 0.5)');
  assert.equal(subtle.tokens['--quick-bg'], 'rgba(26, 27, 38, 0.95)');

  const roundedGlass = buildTokens(palette, { rounding: 6, opacity: 'glassy', shellAlpha: 0.70 });
  assert.equal(roundedGlass.tokens['--aihio-radius-base'], '6px');
  assert.equal(roundedGlass.tokens['--aihio-radius-interactive'], '5px');
  assert.equal(roundedGlass.tokens['--aihio-radius-interactive-compact'], '3px');
  assert.equal(roundedGlass.tokens['--aihio-radius-xl'], '8px');
  assert.equal(roundedGlass.tokens['--desk'], 'rgba(19, 20, 28, 0.7)');
  // Panel over desk (0.7 + 0.433 * 0.3 = 0.83) reaches target opacity without double-darkening.
  assert.equal(roundedGlass.tokens['--panel'], 'rgba(26, 27, 38, 0.433)');
  assert.equal(roundedGlass.tokens['--quick-bg'], 'rgba(26, 27, 38, 0.83)');
  assert.match(roundedGlass.tokens['--aihio-color-page-fg'], /^#[0-9a-f]{6}$/, 'foreground text stays 100% opaque');
});

test('omarchyTheme follows atomic theme swaps, glass/corner toggles, and hyprctl rounding live', async t => {
  const dir = await fsp.mkdtemp(path.join(os.tmpdir(), 'parc-omarchy-test-'));
  t.after(() => fsp.rm(dir, { recursive: true, force: true }));

  const currentDir = path.join(dir, 'current');
  const themeDir = path.join(currentDir, 'theme');
  const togglesDir = path.join(dir, 'toggles', 'hypr');
  await fsp.mkdir(themeDir, { recursive: true });
  await fsp.mkdir(togglesDir, { recursive: true });
  await fsp.writeFile(path.join(themeDir, 'colors.toml'), TOKYO_NIGHT);
  await fsp.writeFile(path.join(currentDir, 'theme.name'), 'Tokyo Night\n');

  const events = [];
  const waitFor = async pred => {
    for (let i = 0; i < 60; i++) {
      if (pred(events.at(-1))) return events.at(-1);
      await new Promise(r => setTimeout(r, 50));
    }
    throw new Error(`Timeout waiting for theme event. Last: ${JSON.stringify(events.at(-1))}`);
  };

  const watcher = omarchyTheme({ stateDir: dir, hyprctl: null, onChange: s => events.push(s) });
  t.after(() => watcher.close());
  const initial = watcher.start();
  assert.equal(initial.name, 'Tokyo Night');
  assert.equal(initial.mode, 'dark');
  assert.equal(initial.opacity, 'transparent', 'default Omarchy mode without toggle files is transparent');
  assert.equal(initial.glass, false);
  assert.equal(initial.tokens['--aihio-color-primary-action-bg'], '#7aa2f7');

  // Simulate `omarchy theme set`: stage next/, rm -rf current/theme, mv next/ current/theme, write theme.name.
  const nextDir = path.join(currentDir, 'next-theme');
  await fsp.mkdir(nextDir, { recursive: true });
  await fsp.writeFile(path.join(nextDir, 'colors.toml'), CATPPUCCIN_LATTE);
  await fsp.rm(themeDir, { recursive: true, force: true });
  await fsp.rename(nextDir, themeDir);
  await fsp.writeFile(path.join(currentDir, 'theme.name'), 'Catppuccin Latte\n');

  const light = await waitFor(s => s?.name === 'Catppuccin Latte');
  assert.equal(light.mode, 'light');
  assert.equal(light.opacity, 'transparent');
  assert.equal(light.tokens['--aihio-color-primary-action-bg'], '#1e66f5');

  // Toggle Glassy + Rounded corners live.
  await fsp.writeFile(path.join(togglesDir, 'window-glassy.lua'), '-- glassy\n');
  await fsp.writeFile(path.join(togglesDir, 'corners-rounded.lua'), '-- rounded\n');
  const glassy = await waitFor(s => s?.opacity === 'glassy' && s?.rounding === 6);
  assert.equal(glassy.glass, true);
  assert.match(glassy.tokens['--desk'], /^rgba\(/);
  assert.equal(glassy.tokens['--aihio-radius-base'], '6px');

  // window-opaque overrides glass back to solid.
  await fsp.writeFile(path.join(togglesDir, 'window-opaque.lua'), '-- opaque\n');
  const opaque = await waitFor(s => s?.opacity === 'opaque');
  assert.equal(opaque.glass, false);
  assert.match(opaque.tokens['--desk'], /^#[0-9a-f]{6}$/);

  // Stub hyprctl reports custom live rounding (e.g. 10px).
  const fakeHyprctl = path.join(dir, 'hyprctl');
  await fsp.writeFile(fakeHyprctl, '#!/bin/sh\nprintf \'{"option":"decoration:rounding","int":10}\\n\'\n', { mode: 0o700 });
  const withHypr = omarchyTheme({ stateDir: dir, hyprctl: fakeHyprctl });
  t.after(() => withHypr.close());
  withHypr.loadSync();
  const polled = await withHypr.reload({ queryDesktop: true });
  assert.equal(polled.rounding, 10);
  assert.equal(polled.tokens['--aihio-radius-base'], '10px');
});
