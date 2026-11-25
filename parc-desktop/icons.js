// One stroke family (24px grid, 1.75 stroke, round joins) for every glyph in the workspace.
const paths = {
  plus: '<path d="M12 5v14M5 12h14"/>',
  compose: '<path d="M11 4.5H6.5a2 2 0 0 0-2 2v11a2 2 0 0 0 2 2h11a2 2 0 0 0 2-2V13"/><path d="M17.4 3.9a2 2 0 0 1 2.8 2.8l-7.4 7.4-3.6.9.9-3.6Z"/>',
  ring: '<circle cx="12" cy="12" r="7.5"/>',
  hash: '<path d="M10 4 8 20M16 4l-2 16M5 9h15M4 15h15"/>',
  search: '<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>',
  graph: '<circle cx="6" cy="12" r="2.5"/><circle cx="18" cy="5.5" r="2.5"/><circle cx="18" cy="18.5" r="2.5"/><path d="m8.2 10.8 7.6-4.1M8.2 13.2l7.6 4.1"/>',
  agent: '<rect x="4" y="8" width="16" height="12" rx="3"/><path d="M12 8V4.5M9 13.5v1.5M15 13.5v1.5M2 14h2M20 14h2"/>',
  pulse: '<path d="M3 12h4l2.5-6 5 12 2.5-6h4"/>',
  mic: '<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21"/>',
  stop: '<rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor" stroke="none"/>',
  chevrons: '<path d="m8 9.5 4-4 4 4M8 14.5l4 4 4-4"/>',
  check: '<path d="m5 12.5 4.5 4.5L19 7.5"/>',
  folder: '<path d="M3.5 18.5V6.5a2 2 0 0 1 2-2h4l2 2.5h7a2 2 0 0 1 2 2v.5M3.5 18.5l2.6-7.2a2 2 0 0 1 1.9-1.3H20a1.5 1.5 0 0 1 1.4 2l-2.2 6.2a2 2 0 0 1-1.9 1.3H5.5a2 2 0 0 1-2-1.5Z"/>',
  layers: '<path d="m12 3.5 8.5 4.25L12 12 3.5 7.75Z"/><path d="m3.5 12 8.5 4.25L20.5 12"/><path d="m3.5 16.25 8.5 4.25 8.5-4.25"/>',
  back: '<path d="M9 14.5 4 9.5l5-5"/><path d="M4 9.5h10.5a5.5 5.5 0 0 1 0 11H11"/>',
  link: '<path d="M10 13.5a4.5 4.5 0 0 0 6.4.4l2.8-2.8a4.5 4.5 0 0 0-6.4-6.4l-1.3 1.3"/><path d="M14 10.5a4.5 4.5 0 0 0-6.4-.4l-2.8 2.8a4.5 4.5 0 0 0 6.4 6.4l1.3-1.3"/>',
  calendar: '<rect x="3.5" y="5" width="17" height="15.5" rx="2.5"/><path d="M3.5 10h17M8 3v4M16 3v4"/>',
  alert: '<circle cx="12" cy="12" r="8.5"/><path d="M12 7.5v5M12 16.2v.1"/>',
  x: '<path d="m7 7 10 10M17 7 7 17"/>',
  chevron: '<path d="m7.5 10 4.5 4.5 4.5-4.5"/>',
  flag: '<path d="M5.5 21V4.5M5.5 4.5h11l-2.5 4 2.5 4h-11"/>',
  tag: '<path d="M3.5 12.2V4.5a1 1 0 0 1 1-1h7.7a2 2 0 0 1 1.4.6l7.3 7.3a2 2 0 0 1 0 2.8l-6.6 6.6a2 2 0 0 1-2.8 0l-7.3-7.3a2 2 0 0 1-.7-1.3Z"/><circle cx="8.5" cy="8.5" r="1.5"/>',
  sidebar: '<rect x="3.5" y="4.5" width="17" height="15" rx="2.5"/><path d="M9.5 4.5v15"/>',
  more: '<circle cx="6" cy="12" r="1.2" fill="currentColor"/><circle cx="12" cy="12" r="1.2" fill="currentColor"/><circle cx="18" cy="12" r="1.2" fill="currentColor"/>',
  settings: '<path d="M4 7h9M17 7h3M4 17h3M11 17h9"/><circle cx="15" cy="7" r="2"/><circle cx="9" cy="17" r="2"/>',
  history: '<path d="M4 12a8 8 0 1 0 2.4-5.7L4 8.5"/><path d="M4 4v4.5h4.5M12 8v4.5l3 2"/>',
  archive: '<rect x="3.5" y="4.5" width="17" height="4.5" rx="1.5"/><path d="M5 9v9a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V9M10 13h4"/>',
  trash: '<path d="M4.5 7h15M9.5 7V5a1.5 1.5 0 0 1 1.5-1.5h2A1.5 1.5 0 0 1 14.5 5v2M6.5 7l.8 12a2 2 0 0 0 2 1.9h5.4a2 2 0 0 0 2-1.9l.8-12"/>',
  copy: '<rect x="8.5" y="8.5" width="11" height="11" rx="2"/><path d="M15.5 8.5V6a1.5 1.5 0 0 0-1.5-1.5H6A1.5 1.5 0 0 0 4.5 6v8A1.5 1.5 0 0 0 6 15.5h2.5"/>',
  bolt: '<path d="M13 3 5 13.5h6L10 21l8-10.5h-6Z"/>',
  command: '<path d="m7 9 3 3-3 3M12.5 15H17"/><rect x="3.5" y="4.5" width="17" height="15" rx="2.5"/>',
  sort: '<path d="M4 6.5h9M4 12h6.5M4 17.5h4M17.5 5v14M14 15.5l3.5 3.5 3.5-3.5"/>',
  board: '<rect x="3.5" y="4.5" width="17" height="15" rx="2.5"/><path d="M9.25 4.5v15M14.75 4.5v15M5.9 8h1M11.5 8h1M17.1 8h1M5.9 11h1M11.5 11h1"/>',
  monitor: '<rect x="3" y="4" width="18" height="12.5" rx="2"/><path d="M8.5 20h7M12 16.5V20"/>',
  sun: '<circle cx="12" cy="12" r="3.75"/><path d="M12 3v1.75M12 19.25V21M3 12h1.75M19.25 12H21M5.6 5.6l1.25 1.25M17.15 17.15l1.25 1.25M5.6 18.4l1.25-1.25M17.15 6.85l1.25-1.25"/>',
  moon: '<path d="M19.5 14.5A7.75 7.75 0 0 1 9.5 4.5a7.75 7.75 0 1 0 10 10Z"/>',
  matrix: '<rect x="3.5" y="3.5" width="17" height="17" rx="2.5"/><path d="M9.2 3.5v17M14.8 3.5v17M3.5 9.2h17M3.5 14.8h17"/>',
  fit: '<path d="M4 9V5.5A1.5 1.5 0 0 1 5.5 4H9M15 4h3.5A1.5 1.5 0 0 1 20 5.5V9M20 15v3.5a1.5 1.5 0 0 1-1.5 1.5H15M9 20H5.5A1.5 1.5 0 0 1 4 18.5V15"/>',
  split: '<rect x="3.5" y="4.5" width="17" height="15" rx="2.5"/><path d="M12 4.5v15"/>',
  paperclip: '<path d="m21 11.5-8.6 8.6a5 5 0 0 1-7.1-7.1l9.2-9.2a3.3 3.3 0 0 1 4.7 4.7l-9.2 9.2a1.7 1.7 0 0 1-2.4-2.4l8.5-8.5"/>',
  file: '<path d="M14 3.5H7a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8.5Z"/><path d="M14 3.5v5h5"/>',
  image: '<rect x="3.5" y="4.5" width="17" height="15" rx="2"/><circle cx="9" cy="10" r="1.5"/><path d="m20.5 15.5-4.5-4.5-6.5 6.5"/>',
  shield: '<path d="M12 3 5 6v5.5c0 4.6 3 8.4 7 9.5 4-1.1 7-4.9 7-9.5V6Z"/><path d="m9 12 2 2 4-4"/>',
  folderPlus: '<path d="M3.5 18.5V6.5a2 2 0 0 1 2-2h4l2 2.5h7a2 2 0 0 1 2 2v9.5a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2Z"/><path d="M12 11v5M9.5 13.5h5"/>',
  shuffle: '<path d="M16 4h4v4M4 19 20 4M20 16v4h-4M14 14l6 6M4 5l6 6"/>',
  text: '<path d="M4 6h16M4 12h11M4 18h7"/>',
  list: '<path d="M9 6h11M9 12h11M9 18h11M4.5 6h.01M4.5 12h.01M4.5 18h.01"/>',
  // Fragment types
  note: '<path d="M14 3.5H7a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8.5Z"/><path d="M14 3.5v5h5M9 13h6M9 16.5h4"/>',
  todo: '<rect x="4.5" y="4.5" width="15" height="15" rx="4"/><path d="m8.5 12.2 2.5 2.5 4.5-5"/>',
  decision: '<path d="M12 21v-8M12 3v2.5"/><path d="M5 5.5h11.5l3.5 3.5-3.5 3.5H5Z"/>',
  risk: '<path d="M10.3 4.6 3.4 16.8a2 2 0 0 0 1.7 3h13.8a2 2 0 0 0 1.7-3L13.7 4.6a2 2 0 0 0-3.4 0Z"/><path d="M12 9.5v4M12 16.6v.1"/>',
  idea: '<path d="M9 17.5h6M10 21h4"/><path d="M15 14.5c.3-1.4 1-2.2 1.8-3A6 6 0 1 0 6 8c0 1.4.4 2.4 1.2 3.4.8.9 1.5 1.7 1.8 3.1"/>',
  fragment: '<path d="M12 3.5 19.5 8v8L12 20.5 4.5 16V8Z"/>'
};
const SVG = 'http://www.w3.org/2000/svg';
export const types = ['note', 'todo', 'decision', 'risk', 'idea'];
export function icon(name, className = 'icon') {
  const svg = document.createElementNS(SVG, 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('aria-hidden', 'true');
  svg.setAttribute('class', className);
  svg.innerHTML = paths[name] || paths.fragment;
  return svg;
}
export const typeIcon = (type, className = 'icon') => icon(types.includes(type) ? type : 'fragment', className);
// A status category as a mark, told apart by shape as well as colour: a ring (open), a half-filled ring (active),
// a filled disc with a check (done) and a dashed ring (closed).
const marks = {
  open: '<circle cx="12" cy="12" r="7.5"/>',
  active: '<circle cx="12" cy="12" r="7.5"/><path d="M12 8a4 4 0 0 1 0 8Z" fill="currentColor" stroke="none"/>',
  done: '<circle cx="12" cy="12" r="8.75" fill="currentColor" stroke="none"/><path class="knock" d="m8.6 12.3 2.3 2.3 4.5-4.7"/>',
  closed: '<circle cx="12" cy="12" r="7.5" stroke-dasharray="3.1 2.8"/>'
};
export function statusIcon(category, className = 'icon') {
  const svg = icon('fragment', className);
  svg.innerHTML = marks[category] || marks.open;
  svg.dataset.tone = category || 'open';
  return svg;
}
// Replace [data-icon] placeholders in static markup.
export function hydrateIcons(root = document) {
  for (const el of root.querySelectorAll('[data-icon]')) {
    const svg = icon(el.dataset.icon, el.className || 'icon');
    if (el.id) svg.id = el.id;
    el.replaceWith(svg);
  }
}
