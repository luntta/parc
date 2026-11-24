// The Related panel: fragments in the vault that bear on what is being written, found as you type or dictate.
// It asks again after each short pause (and at least every MAX_WAIT while typing goes on), at once when a
// dictation lands, and keeps what it shows until an answer arrives; an answer overtaken by a newer one is
// dropped. A source that stays keeps its place and newcomers join at the end, so the list doesn't jump while
// you write. It never takes focus. A source opens in place to be read without leaving the draft, and from
// there can be linked into it or opened.
import { typeIcon } from './icons.js';

const PAUSE = 150, MAX_WAIT = 600, LIMIT = 5;

function el(tag, className = '', text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
}
function smallButton(label, onClick) {
  const b = document.createElement('aihio-button');
  b.setAttribute('variant', 'ghost'); b.setAttribute('size', 'sm'); b.append(label); b.onclick = onClick;
  return b;
}
// What a request depends on: the words, not their spacing or punctuation.
const words = text => (text.toLowerCase().match(/[\p{L}\p{N}]+/gu) || []).join(' ');
const keyOf = s => JSON.stringify([s.vault, s.id]);

/** Why a source was returned, in a few words. */
export function reasons(r) {
  const out = [];
  if (r.linked) out.push('Linked');
  if (r.shared_tags?.length) out.push(`Tagged ${r.shared_tags.map(t => `#${t}`).join(' ')}`);
  if (r.shared_words?.length) out.push(`Shares ${r.shared_words.join(', ')}`);
  return out.join(' · ');
}

/** An excerpt as text and <mark>s. Offsets count characters (code points), as parc-core does. */
export function excerpt({ text, column, ranges }) {
  const chars = Array.from(text), out = column > 1 ? ['…'] : [];
  let at = 0;
  for (const [from, to] of ranges) {
    if (from < at || to > chars.length) continue;
    if (from > at) out.push(chars.slice(at, from).join(''));
    out.push(el('mark', '', chars.slice(from, to).join('')));
    at = to;
  }
  if (at < chars.length) out.push(chars.slice(at).join(''));
  return out;
}

/**
 * host: the panel, holding a .related-list and a .related-empty.
 * seed(): what to find related fragments for now, or null when nothing is being written:
 *   { vaults: [path], vault, id, text, tags } (vault and id: the saved fragment being edited, if any).
 * recall(vault, params) and note(vault, id): the backend. open(source), link(source): the two actions.
 * date(iso): how to show when a source was written.
 */
export function relatedPanel(host, { seed, recall, note, open, link, date }) {
  const list = host.querySelector('.related-list'), empty = host.querySelector('.related-empty');
  let timer = 0, since = 0, asked = 0, floor = 0, lastKey = null, items = [];

  function schedule(wait) { clearTimeout(timer); timer = setTimeout(run, wait); }
  async function run() {
    clearTimeout(timer); since = 0;
    const s = seed();
    if (!s) return;
    const w = words(s.text), key = JSON.stringify([s.vaults, s.vault, s.id, w, s.tags]);
    if (key === lastKey) return;
    lastKey = key;
    const mine = ++asked;
    if (!w && !s.tags.length && !s.id) return show(mine, [], 'blank');
    const replies = await Promise.all(s.vaults.map(vault => {
      const params = { text: s.text, tags: s.tags, limit: LIMIT };
      if (s.id && s.vault === vault) params.id = s.id;
      return recall(vault, params).then(r => r.sources.map(source => ({ ...source, vault })), () => null);
    }));
    // When every vault failed, keep what is shown and ask again next time.
    if (replies.every(r => r === null)) { if (lastKey === key) lastKey = null; return; }
    const sources = replies.flatMap(r => r || []).sort((a, b) => b.score - a.score).slice(0, LIMIT);
    show(mine, sources, 'none');
  }

  function show(mine, sources, state) {
    if (mine <= floor) return; // a newer answer is already on screen
    floor = mine;
    const next = new Map(sources.map(s => [keyOf(s), s]));
    items = items.filter(i => { if (next.has(i.key)) return true; i.node.remove(); return false; });
    for (const i of items) { i.source = next.get(i.key); fill(i); }
    const staying = new Set(items.map(i => i.key));
    for (const s of sources) {
      if (staying.has(keyOf(s))) continue;
      const i = { key: keyOf(s), source: s, open: false };
      create(i); fill(i); items.push(i); list.append(i.node);
    }
    empty.hidden = items.length > 0;
    empty.textContent = items.length ? '' : state === 'blank' ? 'Related notes appear here as you write.' : 'Nothing in the vault is clearly related.';
    host.dataset.count = items.length;
  }

  function create(i) {
    const node = el('li', 'rel-item rel-new'), head = el('button', 'rel-head'), mark = el('span', 'rel-icon');
    const title = el('span', 'rel-title'), when = el('span', 'rel-date');
    head.type = 'button'; head.setAttribute('aria-expanded', 'false'); head.onclick = () => toggle(i);
    head.append(mark, title, when);
    const why = el('p', 'rel-why'), quote = el('p', 'rel-quote'), more = el('div', 'rel-more');
    more.hidden = true;
    node.append(head, why, quote, more);
    Object.assign(i, { node, head, mark, title, when, why, quote, more });
  }
  // Updates in place, so a focused or expanded source stays as it is.
  function fill(i) {
    const s = i.source;
    if (i.type !== s.type) { i.mark.replaceChildren(typeIcon(s.type)); i.type = s.type; }
    i.title.textContent = s.title || 'Untitled';
    i.when.textContent = date(s.created_at);
    i.head.title = `${i.open ? 'Close' : 'Read'} ${s.title || 'this fragment'}${s.archived ? ' (archived)' : ''}`;
    i.node.toggleAttribute('data-archived', !!s.archived);
    i.why.textContent = reasons(s.reasons);
    const first = s.excerpts?.[0];
    i.quote.replaceChildren(...(first ? excerpt(first) : []));
    i.quote.hidden = !first || i.open;
  }
  async function toggle(i) {
    i.open = !i.open;
    i.head.setAttribute('aria-expanded', String(i.open)); i.node.toggleAttribute('data-open', i.open);
    i.more.hidden = !i.open; fill(i);
    if (!i.open || i.loaded) return;
    i.loaded = true;
    const body = el('div', 'rel-body', 'Reading…'), actions = el('div', 'rel-actions');
    actions.append(
      smallButton('Link', () => link(i.source)),
      smallButton('Open', () => open(i.source)));
    i.more.replaceChildren(body, actions);
    try {
      const full = await note(i.source.vault, i.source.id);
      body.textContent = (full.body || '').replace(/^\n/, '').trim() || 'No text, only a title.';
    } catch { body.textContent = 'Couldn’t read it. It may have been moved to the trash.'; i.loaded = false; }
  }

  return {
    /** The text changed: ask after a pause, or at least every MAX_WAIT while typing goes on. */
    changed() {
      const now = Date.now();
      if (!since) since = now;
      schedule(Math.max(0, Math.min(PAUSE, since + MAX_WAIT - now)));
    },
    /** Ask now (after the current task, so text set in the same task counts): a dictation landed, a fragment opened. */
    now() { schedule(0); },
    /** Ask again even if the text is the same: the vault changed. */
    refresh() { lastKey = null; schedule(0); },
    /** Another fragment or a new capture: what was shown is about something else. */
    reset() {
      clearTimeout(timer); since = 0; lastKey = null; floor = asked;
      for (const i of items) i.node.remove();
      items = []; empty.hidden = true; empty.textContent = ''; host.dataset.count = 0;
    }
  };
}
