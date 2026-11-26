// Applies (or clears) Omarchy system-theme CSS custom properties on :root so both the
// workspace window and Quick Capture follow the active Omarchy theme live.
const applied = new Set();

export function applySystemTheme(theme) {
  const root = document.documentElement;
  const next = theme?.tokens && typeof theme.tokens === 'object' ? theme.tokens : null;
  for (const prop of [...applied]) {
    if (!next || !(prop in next)) {
      root.style.removeProperty(prop);
      applied.delete(prop);
    }
  }
  if (!next) {
    delete root.dataset.omarchy;
    delete root.dataset.omarchyGlass;
    delete root.dataset.omarchyOpacity;
    return;
  }
  for (const [prop, value] of Object.entries(next)) {
    root.style.setProperty(prop, value);
    applied.add(prop);
  }
  root.dataset.omarchy = theme.name || 'Omarchy';
  root.dataset.omarchyOpacity = theme.opacity || (theme.glass ? 'glassy' : 'opaque');
  root.dataset.omarchyGlass = theme.glass ? 'true' : 'false';
}

export function initSystemTheme(api) {
  if (!api?.systemTheme) return () => {};
  api.systemTheme().then(applySystemTheme, () => {});
  return api.onSystemTheme ? api.onSystemTheme(applySystemTheme) : () => {};
}
