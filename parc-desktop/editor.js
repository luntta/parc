import { EditorState, Compartment, StateEffect } from '@codemirror/state';
import { EditorView, Decoration, ViewPlugin, WidgetType, keymap, placeholder } from '@codemirror/view';
import { history, historyKeymap, defaultKeymap } from '@codemirror/commands';
import { autocompletion, acceptCompletion } from '@codemirror/autocomplete';
import { markdown, markdownLanguage } from '@codemirror/lang-markdown';
import { syntaxTree, syntaxHighlighting, HighlightStyle } from '@codemirror/language';
import { tags } from '@lezer/highlight';
import { icon, typeIcon } from './icons.js';

// Decorations never rewrite the source. Moving the caret onto a line reveals its syntax.
class Glyph extends WidgetType {
  constructor(text, className) { super(); this.text = text; this.className = className; }
  eq(other) { return this.text === other.text && this.className === other.className; }
  toDOM() { const span = document.createElement('span'); span.className = this.className; span.textContent = this.text; return span; }
  ignoreEvent() { return false; }
}
// A task box toggles its own [ ] / [x] in the source, so clicking it is an edit like any other.
class TaskBox extends WidgetType {
  constructor(pos, checked) { super(); this.pos = pos; this.checked = checked; }
  eq(other) { return this.pos === other.pos && this.checked === other.checked; }
  toDOM(view) {
    const cell = document.createElement('span'), box = document.createElement('span');
    cell.className = 'md-taskcell'; cell.setAttribute('aria-hidden', 'true');
    box.className = 'md-task'; box.toggleAttribute('data-checked', this.checked); box.append(icon('check'));
    box.addEventListener('mousedown', e => {
      e.preventDefault();
      if (!view.state.facet(EditorView.editable)) return;
      view.dispatch({ changes: { from: this.pos + 1, to: this.pos + 2, insert: this.checked ? ' ' : 'x' } });
    });
    cell.append(box);
    return cell;
  }
  ignoreEvent() { return true; }
}
// [[id-prefix]] or [[id-prefix|label]] (optionally followed by `p. 12` or `slide 3`) shown as the fragment it points at.
class WikiLink extends WidgetType {
  constructor(prefix, label, target, open, part = null, partLabel = '') {
    super();
    Object.assign(this, { prefix, label, target, open, part, partLabel });
  }
  eq(o) {
    return o.prefix === this.prefix && o.label === this.label && o.part === this.part && o.partLabel === this.partLabel
      && o.target?.title === this.target?.title && o.target?.type === this.target?.type;
  }
  toDOM() {
    const chip = document.createElement('span');
    chip.className = 'md-wiki';
    if (this.target) {
      chip.append(typeIcon(this.target.type));
      chip.append(this.label || this.target.title || 'Untitled');
      if (this.partLabel) {
        const loc = document.createElement('span');
        loc.className = 'md-wiki-loc';
        loc.textContent = this.partLabel;
        chip.append(loc);
      }
      chip.title = `Open ${this.target.title || 'fragment'}${this.partLabel ? ` (${this.partLabel})` : ''}`;
      chip.addEventListener('mousedown', e => {
        e.preventDefault();
        this.open(this.prefix, e, this.part ? { part: this.part } : undefined);
      });
    } else {
      chip.dataset.missing = ''; chip.textContent = this.label || this.prefix;
      chip.title = `No fragment ${this.prefix} in this vault`;
    }
    return chip;
  }
  ignoreEvent() { return !!this.target; }
}
// Jump badge at the end of a document quote line (`> p. 12 · "…"`), clicking jumps the viewer to that passage.
class QuoteJump extends WidgetType {
  constructor(line, label, onOpen) { super(); Object.assign(this, { line, label, onOpen }); }
  eq(o) { return o.line === this.line && o.label === this.label; }
  toDOM() {
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'md-quote-jump';
    btn.textContent = `↗ ${this.label}`;
    btn.title = `Show ${this.label} in document`;
    btn.addEventListener('mousedown', e => { e.preventDefault(); e.stopPropagation(); this.onOpen?.(this.line); });
    return btn;
  }
  ignoreEvent() { return true; }
}
// On the line being edited, a link shows its source plus a faint note of what it resolves to.
class WikiHint extends WidgetType {
  constructor(target) { super(); this.target = target; }
  eq(o) { return o.target?.title === this.target?.title && o.target?.type === this.target?.type; }
  toDOM() {
    const hint = document.createElement('span');
    hint.className = 'md-wikihint'; hint.setAttribute('aria-hidden', 'true');
    if (this.target) hint.append(typeIcon(this.target.type), this.target.title || 'Untitled');
    else { hint.dataset.missing = ''; hint.textContent = 'No single fragment with this ID'; }
    return hint;
  }
}
// ![[attach:filename]] or ![[attach:filename|label]] shown as an inline image preview or clickable chip.
class AttachWidget extends WidgetType {
  constructor(filename, label, info, open) { super(); Object.assign(this, { filename, label, info, open }); }
  eq(o) { return o.filename === this.filename && o.label === this.label && o.info?.exists === this.info?.exists && o.info?.imageUrl === this.info?.imageUrl; }
  toDOM() {
    if (this.info?.imageUrl) {
      const wrap = document.createElement('span');
      wrap.className = 'md-attach-figure';
      const img = document.createElement('img');
      img.className = 'md-attach-img'; img.src = this.info.imageUrl; img.alt = this.label || this.filename;
      img.title = `Open ${this.filename}`;
      img.addEventListener('mousedown', e => { e.preventDefault(); this.open?.(this.filename); });
      wrap.append(img);
      if (this.label) {
        const cap = document.createElement('span');
        cap.className = 'md-attach-caption'; cap.textContent = this.label;
        wrap.append(cap);
      }
      return wrap;
    }
    const chip = document.createElement('span');
    chip.className = 'md-attach';
    if (this.info?.exists) {
      chip.append(icon('paperclip'), this.label || this.filename);
      chip.title = `Open ${this.filename}`;
      chip.addEventListener('mousedown', e => { e.preventDefault(); this.open?.(this.filename); });
    } else {
      chip.dataset.missing = ''; chip.append(icon('paperclip'), this.label || this.filename);
      chip.title = `Attachment “${this.filename}” is not attached to this fragment`;
    }
    return chip;
  }
  ignoreEvent() { return !!this.info?.exists; }
}
const refresh = StateEffect.define();
const CODE = new Set(['InlineCode', 'CodeText', 'FencedCode', 'CodeBlock', 'URL', 'Autolink', 'HTMLTag', 'Comment']);
const WIKI = /\[\[([0-9A-HJKMNP-TV-Za-hjkmnp-tv-z]{4,26})(?:\|([^\]\n]*))?\]\]/g;
const WIKI_LOC = /^[ \t]+(?:(p\.\s*(\d+))|(slide\s+(\d+)))(?=$|[\s.,;:!?)\]])/i;
const DOC_QUOTE = /^\s*>\s*(?:([^\s/\\]+?\.[A-Za-z0-9]{1,8})\s+)?(?:(p\.\s*\d+)|(slide\s+\d+)|(§\s*[^·\n]+?))\s*·\s*["“](.+?)["”]\s*$/;
const ATTACH = /!\[\[attach:([^\]\n|]+?)(?:\|([^\]\n]*))?\]\]/g;
const TAG = /(^|[\s(])(#[\p{L}\p{N}][\p{L}\p{N}_/-]*)/gu;

// ── [[ link completion ──
// Ranked in memory over the fragments the window already holds, so every keystroke answers at once.
export const fold = s => s.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase();
function wordStart(text, token) {
  for (let i = text.indexOf(token); i >= 0; i = text.indexOf(token, i + 1)) if (i === 0 || !/[\p{L}\p{N}]/u.test(text[i - 1])) return i;
  return -1;
}
function subsequence(text, token) { // ranges of a fuzzy, in-order match, or null
  const out = []; let at = 0;
  for (const ch of token) {
    const i = text.indexOf(ch, at); if (i < 0) return null;
    if (out.length && out[out.length - 1] === i) out[out.length - 1] = i + 1; else out.push(i, i + 1);
    at = i + 1;
  }
  // Only tight matches count ("brd" in "board", not "dep" across "dictate captures").
  return out.length <= 6 && out[out.length - 1] - out[0] <= token.length * 2 + 2 ? out : null;
}
export function rank(pool, query) {
  const tokens = fold(query).split(/\s+/).filter(Boolean);
  if (!tokens.length) return pool.slice(0, 30).map(r => ({ r, match: [] }));
  const hits = [];
  pool.forEach((r, order) => {
    const raw = r.title || '', title = fold(raw), mark = title.length === raw.length;
    let score = 0, match = [];
    for (const t of tokens) {
      const w = wordStart(title, t), i = title.indexOf(t);
      if (w === 0) { score += 120; match.push(0, t.length); }
      else if (w > 0) { score += 80; match.push(w, w + t.length); }
      else if (/^[0-9a-z]{2,}$/.test(t) && r.id.toLowerCase().startsWith(t)) score += 90;
      else if (i >= 0) { score += 40; match.push(i, i + t.length); }
      else if ((r.tags || []).some(g => fold(g).startsWith(t.replace(/^#/, '')))) score += 30;
      else if (fold(r.type || '').startsWith(t)) score += 20;
      else { const f = t.length > 2 && subsequence(title, t); if (!f) return; score += 10; match.push(...f); }
    }
    hits.push({ r, order, score, match: mark ? merge(match) : [] });
  });
  return hits.sort((a, b) => b.score - a.score || a.order - b.order).slice(0, 30);
}
function merge(ranges) {
  const pairs = []; for (let i = 0; i < ranges.length; i += 2) pairs.push([ranges[i], ranges[i + 1]]);
  pairs.sort((a, b) => a[0] - b[0]);
  const out = [];
  for (const [a, b] of pairs) { if (out.length && a <= out[out.length - 1]) out[out.length - 1] = Math.max(out[out.length - 1], b); else out.push(a, b); }
  return out;
}
function linkCompletion(links) {
  const source = context => {
    const m = context.matchBefore(/\[\[[^[\]\n|]*$/);
    if (!m) return null;
    for (let n = syntaxTree(context.state).resolveInner(m.from, 1); n; n = n.parent) if (CODE.has(n.name)) return null;
    const query = m.text.slice(2), hits = rank(links.pool(), query);
    if (!hits.length) return null;
    return {
      from: m.from + 2, filter: false,
      getMatch: c => c.match,
      options: hits.map(({ r, match }) => ({
        label: r.title || 'Untitled', fragment: r, match,
        section: query.trim() ? undefined : 'Recent',
        apply(view, _c, _from, to) {
          const end = view.state.sliceDoc(to, to + 2) === ']]' ? to + 2 : to, insert = `[[${links.shortId(r)}]]`;
          view.dispatch({ changes: { from: m.from, to: end, insert }, selection: { anchor: m.from + insert.length }, userEvent: 'input.complete' });
        }
      }))
    };
  };
  return [
    autocompletion({
      override: [source], activateOnTypingDelay: 0, icons: false, maxRenderedOptions: 30,
      tooltipClass: () => 'cm-linkmenu', optionClass: () => 'cm-linkoption',
      addToOptions: [
        { position: 10, render: c => { const g = typeIcon(c.fragment.type, 'icon cm-linkglyph'); return g; } },
        { position: 90, render: c => {
          const meta = document.createElement('span'), d = links.describe(c.fragment);
          meta.className = 'cm-linkmeta';
          for (const part of d) { const s = document.createElement('span'); s.textContent = part; meta.append(s); }
          return meta;
        } }
      ]
    }),
    keymap.of([{ key: 'Tab', run: acceptCompletion }])
  ];
}

function liveMarks(view, links) {
  const ranges = [], { doc, selection } = view.state, tree = syntaxTree(view.state);
  const activeLines = new Set();
  if (view.hasFocus) for (const r of selection.ranges) for (let n = doc.lineAt(r.from).number; n <= doc.lineAt(r.to).number; n++) activeLines.add(n);
  const active = n => activeLines.has(n);
  for (const { from, to } of view.visibleRanges) {
    tree.iterate({ from, to, enter(node) {
      const line = doc.lineAt(node.from), parent = node.node.parent?.name;
      if (node.name === 'FencedCode' || node.name === 'CodeBlock') {
        const first = doc.lineAt(node.from).number, last = doc.lineAt(node.to).number;
        for (let n = Math.max(first, doc.lineAt(from).number); n <= Math.min(last, doc.lineAt(to).number); n++) {
          const cls = ['md-code-line', n === first && 'md-code-first', n === last && 'md-code-last'].filter(Boolean).join(' ');
          ranges.push(Decoration.line({ class: cls }).range(doc.line(n).from));
        }
      }
      if (/^ATXHeading[1-6]$/.test(node.name)) ranges.push(Decoration.line({ class: `md-heading md-h${node.name.at(-1)}` }).range(line.from));
      if (node.name === 'QuoteMark') ranges.push(Decoration.line({ class: 'md-quote' }).range(line.from));
      if (node.name === 'HorizontalRule') ranges.push(Decoration.mark({ class: 'md-hr' }).range(node.from, node.to));
      if (node.name === 'Task' && /^\[[xX]\]/.test(doc.sliceString(node.from, node.from + 3))) ranges.push(Decoration.mark({ class: 'md-done' }).range(node.from + 3, node.to));
      const space = pos => doc.sliceString(pos, pos + 1) === ' ' && pos < line.to ? 1 : 0;
      if (node.name === 'ListMark' && parent === 'ListItem') {
        // Every list line hangs by its depth; the marker (raw or drawn) fills one fixed column.
        const item = node.node.parent, ordered = item.parent?.name === 'OrderedList';
        let depth = 0; for (let p = item; p; p = p.parent) if (p.name === 'BulletList' || p.name === 'OrderedList') depth++;
        ranges.push(Decoration.line({ class: `md-li md-li-${Math.min(depth, 4)}` }).range(line.from));
        if (node.from > line.from && /^[ \t]+$/.test(doc.sliceString(line.from, node.from))) ranges.push(Decoration.mark({ class: 'md-indent' }).range(line.from, node.from));
        const end = node.to + space(node.to);
        if (ordered || active(line.number)) ranges.push(Decoration.mark({ class: 'md-marker' }).range(node.from, end));
        else if (!item.getChild('Task')) ranges.push(Decoration.replace({ widget: new Glyph('•', 'md-bullet') }).range(node.from, end));
      }
      if (active(line.number)) return;
      if (node.name === 'TaskMarker') {
        const item = node.node.parent?.parent, mark = item?.getChild('ListMark');
        const start = mark && item.parent?.name === 'BulletList' ? mark.from : node.from;
        ranges.push(Decoration.replace({ widget: new TaskBox(node.from, /x/i.test(doc.sliceString(node.from, node.to))) }).range(start, node.to + space(node.to)));
      }
      if (['HeaderMark', 'EmphasisMark', 'StrikethroughMark', 'QuoteMark'].includes(node.name) || (node.name === 'CodeMark' && parent === 'InlineCode') || (parent === 'Link' && ['LinkMark', 'URL'].includes(node.name))) {
        const end = node.name === 'HeaderMark' || node.name === 'QuoteMark' ? Math.min(node.to + 1, line.to) : node.to;
        ranges.push(Decoration.replace({}).range(node.from, end));
      }
    } });
    // Wiki-links and #tags are parc syntax, not Markdown: find them in the text outside code.
    const text = doc.sliceString(from, to);
    const inCode = pos => { for (let n = tree.resolveInner(pos, 1); n; n = n.parent) if (CODE.has(n.name)) return true; return false; };
    for (const m of text.matchAll(WIKI)) {
      const start = from + m.index;
      let end = start + m[0].length;
      if (inCode(start)) continue;
      if (active(doc.lineAt(start).number)) {
        ranges.push(Decoration.mark({ class: 'md-wikisrc' }).range(start, end));
        ranges.push(Decoration.widget({ widget: new WikiHint(links.resolve(m[1].toUpperCase())), side: 1 }).range(end));
      } else {
        let part = null, partLabel = '';
        const tail = WIKI_LOC.exec(doc.sliceString(end, Math.min(doc.length, end + 32)));
        if (tail) {
          part = Number.parseInt(tail[2] || tail[4], 10) || null;
          partLabel = (tail[1] || tail[3] || '').replace(/\s+/g, ' ');
          if (part) end += tail[0].length;
        }
        ranges.push(Decoration.replace({ widget: new WikiLink(m[1].toUpperCase(), m[2]?.trim(), links.resolve(m[1].toUpperCase()), links.open, part, partLabel) }).range(start, end));
      }
    }
    if (links.openQuote) {
      const firstLine = doc.lineAt(from).number, lastLine = doc.lineAt(to).number;
      for (let ln = firstLine; ln <= lastLine; ln++) {
        const lineObj = doc.line(ln);
        if (inCode(lineObj.from)) continue;
        const qm = DOC_QUOTE.exec(lineObj.text);
        if (!qm) continue;
        const locLabel = (qm[2] || qm[3] || qm[4] || '').trim().replace(/\s+/g, ' ');
        ranges.push(Decoration.line({ class: 'md-doc-quote' }).range(lineObj.from));
        ranges.push(Decoration.widget({ widget: new QuoteJump(ln, locLabel, links.openQuote), side: 1 }).range(lineObj.to));
      }
    }
    for (const m of text.matchAll(ATTACH)) {
      const start = from + m.index, end = start + m[0].length;
      if (inCode(start)) continue;
      const filename = m[1].trim(), label = m[2]?.trim() || '';
      if (active(doc.lineAt(start).number)) {
        ranges.push(Decoration.mark({ class: 'md-wikisrc' }).range(start, end));
      } else {
        const info = links.attachment?.(filename) || null;
        ranges.push(Decoration.replace({ widget: new AttachWidget(filename, label, info, links.openAttachment) }).range(start, end));
      }
    }
    for (const m of text.matchAll(TAG)) {
      const start = from + m.index + m[1].length, end = start + m[2].length;
      if (!inCode(start) && !/^#\d+$/.test(m[2])) ranges.push(Decoration.mark({ class: 'md-tag' }).range(start, end));
    }
  }
  return Decoration.set(ranges, true);
}
const style = HighlightStyle.define([
  { tag: tags.strong, class: 'md-strong' }, { tag: tags.emphasis, class: 'md-em' },
  { tag: tags.strikethrough, class: 'md-strike' }, { tag: tags.link, class: 'md-link' },
  { tag: tags.monospace, class: 'md-code' }, { tag: tags.processingInstruction, class: 'md-mark' }
]);
const NO_LINKS = { resolve: () => null, open() {}, pool: () => [], shortId: r => r.id, describe: () => [] };

// ── Grep: literal text, smart case, as fragment.grep matches it ──
// Case-insensitive unless the text has an uppercase letter.
export function grepRegex(pattern) {
  return new RegExp(pattern.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), /\p{Uppercase}/u.test(pattern) ? 'gu' : 'giu');
}
// The match at a 1-based line and column in characters (fragment.grep's positions), or null.
function matchAt(doc, line, column, re) {
  if (!(line >= 1 && line <= doc.lines)) return null;
  const l = doc.line(line), offset = [...l.text].slice(0, Math.max(0, column - 1)).join('').length;
  re.lastIndex = offset;
  const m = re.exec(l.text);
  return m && m.index === offset ? { from: l.from + offset, to: l.from + offset + m[0].length } : null;
}
function hitMarks(doc, re, current) {
  const ranges = [], text = doc.toString();
  re.lastIndex = 0;
  for (const m of text.matchAll(re)) {
    if (!m[0] || ranges.length >= 5000) break;
    const here = current && m.index === current.from;
    ranges.push(Decoration.mark({ class: here ? 'md-hit md-hit-current' : 'md-hit' }).range(m.index, m.index + m[0].length));
  }
  return Decoration.set(ranges);
}
function liveView(getLinks) {
  return ViewPlugin.fromClass(class {
    constructor(view) { this.decorations = liveMarks(view, getLinks()); }
    update(u) {
      if (u.docChanged || u.selectionSet || u.viewportChanged || u.focusChanged || syntaxTree(u.startState) !== syntaxTree(u.state) || u.transactions.some(t => t.effects.some(e => e.is(refresh)))) this.decorations = liveMarks(u.view, getLinks());
    }
  }, { decorations: v => v.decorations });
}

// A fragment as it reads, not editable: Go to…'s preview and Agent answers. Updates incrementally (appends during
// streaming, relinks in place) instead of rebuilding EditorState from scratch on every frame. With a grep pattern,
// every match is marked and the one at (line, column) stands out; show() returns its range so the caller can scroll.
export function markdownPreview(parent) {
  let links = NO_LINKS, hasHits = false;
  const hits = new Compartment();
  const base = [EditorView.cspNonce.of('parc-editor'), markdown({ base: markdownLanguage }), liveView(() => links), syntaxHighlighting(style), EditorView.lineWrapping,
    EditorView.editable.of(false), EditorState.readOnly.of(true), EditorView.contentAttributes.of({ 'aria-label': 'Fragment preview' }), hits.of([])];
  const view = new EditorView({ state: EditorState.create({ extensions: base }), parent });
  return {
    show(doc, { links: next = NO_LINKS, pattern = '', line = 0, column = 0 } = {}) {
      links = next;
      const text = String(doc ?? ''), prev = view.state.doc.toString();
      const changes = text === prev ? undefined : text.startsWith(prev)
        ? { from: prev.length, insert: text.slice(prev.length) }
        : { from: 0, to: prev.length, insert: text };
      if (!pattern) {
        const effects = hasHits ? [hits.reconfigure([]), refresh.of(null)] : changes ? [] : [refresh.of(null)];
        hasHits = false;
        view.dispatch({ ...(changes ? { changes } : {}), ...(effects.length ? { effects } : {}) });
        return null;
      }
      if (changes) view.dispatch({ changes });
      const re = grepRegex(pattern), current = line ? matchAt(view.state.doc, line, column, re) : null;
      hasHits = true;
      view.dispatch({ effects: [hits.reconfigure(EditorView.decorations.of(hitMarks(view.state.doc, re, current))), refresh.of(null)] });
      return current;
    },
    scrollTo(pos) { view.dispatch({ effects: EditorView.scrollIntoView(pos, { y: 'center' }) }); },
    destroy() { view.destroy(); }
  };
}

export function markdownEditor(parent, onChange, links = NO_LINKS) {
  const lock = new Compartment();
  const live = liveView(() => links);
  const extensions = [EditorView.cspNonce.of('parc-editor'), history(), markdown({ base: markdownLanguage }), live, syntaxHighlighting(style), EditorView.lineWrapping, linkCompletion(links),
    keymap.of([...defaultKeymap, ...historyKeymap]), placeholder('Write in Markdown, or dictate a thought…'),
    EditorView.contentAttributes.of({ 'aria-label': 'Fragment body, Markdown', spellcheck: 'true' }), lock.of(EditorView.editable.of(true)),
    EditorView.updateListener.of(u => { if (u.docChanged) onChange(); })];
  const view = new EditorView({ state: EditorState.create({ extensions }), parent });
  return {
    text: () => view.state.doc.toString(), focus: () => view.focus(),
    load(doc) { view.setState(EditorState.create({ doc, extensions })); }, // reset undo history across fragments
    // Insert at the caret (replacing a selection), spaced from the words around it, and put the caret after it.
    insert(text) {
      const { from, to } = view.state.selection.main, doc = view.state.doc;
      const before = from > 0 && !/\s/.test(doc.sliceString(from - 1, from)) ? ' ' : '';
      const after = to < doc.length && !/[\s.,;:!?)]/.test(doc.sliceString(to, to + 1)) ? ' ' : '';
      const insert = before + text + after;
      view.dispatch({ changes: { from, to, insert }, selection: { anchor: from + insert.length }, scrollIntoView: true });
      view.focus();
    },
    append(text) { const end = view.state.doc.length; view.dispatch({ changes: { from: end, insert: (end ? '\n' : '') + text } }); view.focus(); },
    // Append a document quote with blank-line spacing and place the caret on the comment line below it.
    insertQuote(quoteLine) {
      const doc = view.state.doc, end = doc.length;
      const tail = end ? doc.sliceString(Math.max(0, end - 2), end) : '';
      const prefix = !end ? '' : tail === '\n\n' ? '' : tail.endsWith('\n') ? '\n' : '\n\n';
      const insert = `${prefix}${quoteLine}\n`;
      view.dispatch({
        changes: { from: end, insert },
        selection: { anchor: end + insert.length },
        effects: EditorView.scrollIntoView(end + insert.length, { y: 'center' })
      });
      view.focus();
    },
    goToLine(lineNum) {
      if (lineNum < 1 || lineNum > view.state.doc.lines) return false;
      const line = view.state.doc.line(lineNum);
      view.dispatch({
        selection: { anchor: line.from, head: line.to },
        effects: EditorView.scrollIntoView(line.from, { y: 'center' })
      });
      view.focus();
      return true;
    },
    lock(value) { view.dispatch({ effects: lock.reconfigure(EditorView.editable.of(!value)) }); },
    destroy() { view.destroy(); },
    refresh() { view.dispatch({ effects: refresh.of(null) }); },
    // Select a grep match ({ line, column, pattern }, as fragment.grep reports it) and bring it into view.
    reveal({ line, column, pattern }) {
      const m = matchAt(view.state.doc, line, column, grepRegex(pattern));
      if (!m) return false;
      view.dispatch({ selection: { anchor: m.from, head: m.to }, effects: EditorView.scrollIntoView(m.from, { y: 'center' }) });
      view.focus();
      return true;
    }
  };
}
