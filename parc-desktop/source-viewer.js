// Document viewer for `source` fragments:
// - PDF via pdfjs-dist (`PDFViewer`, `EventBus`, `PDFLinkService`, `PDFFindController`),
//   loaded from IPC bytes with `isEvalSupported: false`, `enableScripting: false`, `useWasm: false`,
//   theme-recolouring via `pageColors` (with toggle back to paper), selectable text layer,
//   and external links routed through the narrow `open-external` IPC.
// - DOCX, PPTX, HTML, Markdown and plain text rendered strictly from core-extracted Markdown/text
//   via safe DOM construction (never raw HTML, zero network/script execution).
// - Bidirectional quotes & highlights using the shared normalizer (`quote.mjs`), Promote a comment
//   to todo/idea/decision, Related following the visible page, Ask about this document, and Analyze.
import { renderMarkdownBlock } from './document-markdown.mjs';
import * as pdfjsLib from 'pdfjs-dist/build/pdf.mjs';
import { EventBus, PDFViewer, PDFLinkService, PDFFindController } from 'pdfjs-dist/web/pdf_viewer.mjs';
import { icon, typeIcon } from './icons.js';
import { parseQuotes, formatQuote, findInText } from './quote.mjs';

pdfjsLib.GlobalWorkerOptions.workerSrc = new URL('dist/pdf.worker.mjs', document.baseURI).href;

const PDF_COLORS_KEY = 'parc.pdfColors';
const IMAGE_EXT = /\.(png|jpe?g|gif|webp|avif)$/i;
const PDF_EXT = /\.pdf$/i;

function el(tag, className = '', text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
}

const bytesLabel = n => typeof n !== 'number' ? '' : n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1048576).toFixed(1)} MB`;

// Resolve any CSS color (including oklch / CSS custom properties) to `rgb(r, g, b)` for pdf.js pageColors.
let colorCtx = null;
function resolveRgb(cssColor, fallback) {
  try {
    if (!colorCtx) {
      const c = document.createElement('canvas');
      c.width = 1; c.height = 1;
      colorCtx = c.getContext('2d', { willReadFrequently: true });
    }
    colorCtx.clearRect(0, 0, 1, 1);
    colorCtx.fillStyle = fallback;
    colorCtx.fillStyle = cssColor;
    colorCtx.fillRect(0, 0, 1, 1);
    const [r, g, b] = colorCtx.getImageData(0, 0, 1, 1).data;
    return `rgb(${r}, ${g}, ${b})`;
  } catch { return fallback; }
}

function themePageColors() {
  const cs = getComputedStyle(document.body);
  const bg = resolveRgb(cs.getPropertyValue('--aihio-color-page-bg').trim() || cs.backgroundColor, 'rgb(255, 255, 255)');
  const fg = resolveRgb(cs.getPropertyValue('--aihio-color-page-fg').trim() || cs.color, 'rgb(20, 20, 20)');
  return { background: bg, foreground: fg };
}

function getPdfColorPref() {
  try { return localStorage.getItem(PDF_COLORS_KEY) === 'paper' ? 'paper' : 'theme'; }
  catch { return 'theme'; }
}

function setPdfColorPref(v) {
  try { localStorage.setItem(PDF_COLORS_KEY, v); } catch { /* ignore */ }
}

// First Markdown heading of a section (used for `> § Heading · "…"` quotes).
function headingOfPart(text, index) {
  const m = String(text || '').match(/^\s*#{1,6}\s+(.+?)(?:\s+#+)?\s*$/m);
  return m ? m[1].trim().slice(0, 80) : `Section ${index}`;
}

// Highlight matching ranges in a DOM subtree's text nodes without breaking layout.
// Returns the created `<mark>` elements.
function highlightTextNodes(root, quoteText, { className, lineNum = null, onClick = null }) {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const nodes = [];
  let full = '';
  const codePointStarts = [];
  let cpOffset = 0;
  let n;
  while ((n = walker.nextNode())) {
    if (!n.nodeValue) continue;
    if (n.parentElement?.closest('mark.sv-quote-hl, mark.sv-find-hl')) continue;
    const cps = Array.from(n.nodeValue);
    nodes.push({ node: n, cpStart: cpOffset, cpEnd: cpOffset + cps.length, cps });
    codePointStarts.push(cpOffset);
    full += n.nodeValue;
    cpOffset += cps.length;
  }
  if (!full) return [];
  const hit = findInText(full, quoteText);
  if (!hit || !hit.length) return [];
  const start = hit.offset, end = hit.offset + hit.length;
  const created = [];

  // Process backwards so DOM splits don't invalidate later nodes.
  for (let i = nodes.length - 1; i >= 0; i--) {
    const entry = nodes[i];
    if (entry.cpEnd <= start || entry.cpStart >= end) continue;
    const localStart = Math.max(0, start - entry.cpStart);
    const localEnd = Math.min(entry.cps.length, end - entry.cpStart);
    if (localStart >= localEnd) continue;

    const before = entry.cps.slice(0, localStart).join('');
    const mid = entry.cps.slice(localStart, localEnd).join('');
    const after = entry.cps.slice(localEnd).join('');
    const frag = document.createDocumentFragment();
    if (before) frag.append(document.createTextNode(before));
    const mark = el('mark', className, mid);
    if (lineNum != null) {
      mark.dataset.quoteLine = String(lineNum);
      mark.title = 'Jump to note';
      if (onClick) mark.addEventListener('click', e => { e.preventDefault(); e.stopPropagation(); onClick(lineNum); });
    }
    frag.append(mark);
    created.push(mark);
    if (after) frag.append(document.createTextNode(after));
    entry.node.replaceWith(frag);
  }
  return created.reverse();
}

function clearHighlights(root, selector) {
  for (const mark of root.querySelectorAll(selector)) {
    const parent = mark.parentNode;
    if (!parent) continue;
    parent.replaceChild(document.createTextNode(mark.textContent || ''), mark);
    parent.normalize();
  }
}

/**
 * Controller attached to each `doc` tab (`d.sourceViewer`).
 * Activates only when `d.opened?.type === 'source' && d.attachments?.length > 0`.
 */
export function sourceViewer(d, deps) {
  const {
    api, status, vaultToken, rowsIn, onPageChange, onAsk, onPromote, onApplyAnalyze, onOpenAgent
  } = deps;

  const pane = d.ui.sourcePane;
  const railBar = d.ui.sourceRailBar;
  const railNotes = d.ui.railNotes;
  const railRelated = d.ui.railRelated;
  const railRelatedCount = d.ui.railRelatedCount;
  const quoteActionsBox = d.ui.quoteActions;

  let active = false;
  let currentFile = null;
  let mode = null; // 'pdf' | 'markdown' | 'image' | 'fallback'
  let unit = 'page';
  let currentPart = 1;
  let totalParts = 0;
  let extractedParts = null; // Array<{ part, text }> cached for Related & Markdown viewer
  let pdfDoc = null;
  let pdfTask = null;
  let pdfViewer = null;
  let pdfEventBus = null;
  let pdfLinkService = null;
  let pdfFindController = null;
  let pdfColorMode = getPdfColorPref();
  let observer = null;
  let posTimer = 0;
  let pageChangeTimer = 0;
  let hlTimer = 0;
  let loadSeq = 0;
  let findMatches = [];
  let findIndex = -1;
  let pendingSelection = null; // { text, part, heading }
  let railMode = 'notes';
  let floatBtn = null;
  let relatedObserver = null;

  // Rail switcher (Notes | Related)
  function setRail(next) {
    railMode = next === 'related' ? 'related' : 'notes';
    d.ui.scroller.dataset.rail = railMode;
    railNotes.setAttribute('aria-pressed', String(railMode === 'notes'));
    railRelated.setAttribute('aria-pressed', String(railMode === 'related'));
    if (railMode === 'related') d.related.now();
  }
  railNotes.onclick = () => setRail('notes');
  railRelated.onclick = () => setRail('related');

  // Keep Related count badge in sync with `d.ui.related.dataset.count`
  relatedObserver = new MutationObserver(() => {
    const n = Number(d.ui.related.dataset.count || 0);
    railRelatedCount.hidden = !n;
    railRelatedCount.textContent = n ? String(n) : '';
  });
  relatedObserver.observe(d.ui.related, { attributes: true, attributeFilter: ['data-count'] });

  function teardownViewer() {
    clearTimeout(posTimer);
    clearTimeout(pageChangeTimer);
    clearTimeout(hlTimer);
    if (observer) { observer.disconnect(); observer = null; }
    if (pdfTask) { pdfTask.destroy().catch(() => {}); pdfTask = null; }
    if (pdfDoc) { pdfDoc.destroy().catch(() => {}); pdfDoc = null; }
    pdfViewer = null;
    pdfEventBus = null;
    pdfLinkService = null;
    pdfFindController = null;
    extractedParts = null;
    mode = null;
    totalParts = 0;
    findMatches = [];
    findIndex = -1;
    pendingSelection = null;
    hideFloatQuote();
    pane.replaceChildren();
  }

  function hideFloatQuote() {
    if (floatBtn) { floatBtn.remove(); floatBtn = null; }
  }

  function deactivate() {
    if (!active) return;
    active = false;
    currentFile = null;
    teardownViewer();
    delete d.ui.scroller.dataset.sourceMode;
    delete d.ui.scroller.dataset.rail;
    pane.hidden = true;
    railBar.hidden = true;
    quoteActionsBox.hidden = true;
    quoteActionsBox.replaceChildren();
  }

  function savePositionDebounced() {
    clearTimeout(posTimer);
    const f = d.opened;
    if (!f || !currentFile || !currentPart) return;
    posTimer = setTimeout(() => {
      if (!active || d.opened !== f) return;
      api.setReadingPosition(f.vault, f.id, {
        part: currentPart,
        total: totalParts || null,
        unit,
        filename: currentFile
      }).catch(() => {});
    }, 350);
  }

  function notifyPageChanged() {
    clearTimeout(pageChangeTimer);
    pageChangeTimer = setTimeout(() => {
      if (active) onPageChange();
    }, 350);
  }

  function partLabel(p, tot = totalParts) {
    if (unit === 'slide') return tot ? `Slide ${p} / ${tot}` : `Slide ${p}`;
    if (unit === 'section') return tot ? `§ ${p} / ${tot}` : `§ ${p}`;
    return tot ? `p. ${p} / ${tot}` : `p. ${p}`;
  }

  function setCurrentPart(p, { persist = true } = {}) {
    const clamped = Math.max(1, Math.min(totalParts || p, p));
    if (clamped !== currentPart) {
      currentPart = clamped;
      notifyPageChanged();
    }
    const pageInput = pane.querySelector('.sv-page-input');
    const pageTotal = pane.querySelector('.sv-page-total');
    if (pageInput && document.activeElement !== pageInput) pageInput.value = String(currentPart);
    if (pageTotal) pageTotal.textContent = totalParts ? `/ ${totalParts}` : '';
    const unitBadge = pane.querySelector('.sv-unit-label');
    if (unitBadge) unitBadge.textContent = unit === 'slide' ? 'Slide' : unit === 'section' ? '§' : 'p.';
    if (persist) savePositionDebounced();
  }

  // ── Quotes & Promote strip in Notes rail ──
  function refreshQuoteActions() {
    if (!active || !d.opened) {
      quoteActionsBox.hidden = true;
      quoteActionsBox.replaceChildren();
      return;
    }
    const quotes = parseQuotes(d.editor.text());
    if (!quotes.length) {
      quoteActionsBox.hidden = true;
      quoteActionsBox.replaceChildren();
      return;
    }
    quoteActionsBox.hidden = false;
    const head = el('div', 'quote-actions-head');
    head.append(el('span', 'quote-actions-label', `${quotes.length} ${quotes.length === 1 ? 'quote' : 'quotes'} in notes`));
    const list = el('ul', 'quote-actions-list');
    for (const q of quotes) {
      const li = el('li', 'quote-action-row');
      const loc = q.anchor.kind === 'page' ? `p. ${q.anchor.number}`
        : q.anchor.kind === 'slide' ? `slide ${q.anchor.number}`
        : `§ ${q.anchor.heading}`;
      const jump = el('button', 'quote-action-jump');
      jump.type = 'button';
      jump.title = `Show ${loc} in document`;
      jump.append(el('span', 'quote-action-loc', loc), el('span', 'quote-action-snip', q.comment || q.text));
      jump.onclick = () => jumpToQuoteLine(q.line);

      const dd = document.createElement('aihio-dropdown');
      dd.className = 'quote-promote-menu';
      dd.setAttribute('align', 'end');
      const trig = document.createElement('aihio-button');
      trig.slot = 'trigger'; trig.setAttribute('variant', 'ghost'); trig.setAttribute('size', 'sm');
      trig.append(icon('arrowUp'), 'Promote');
      dd.append(trig);
      for (const targetType of ['todo', 'idea', 'decision']) {
        const it = document.createElement('aihio-dropdown-item');
        it.setAttribute('value', targetType);
        it.append(typeIcon(targetType), el('span', 'item-name', targetType[0].toUpperCase() + targetType.slice(1)));
        dd.append(it);
      }
      dd.addEventListener('aihio-select', e => {
        e.stopPropagation();
        onPromote(d, q, e.detail.value);
      });
      li.append(jump, dd);
      list.append(li);
    }
    quoteActionsBox.replaceChildren(head, list);
  }

  // ── Apply quote highlights to currently rendered PDF pages or Markdown sections ──
  function quotesForCurrentFile() {
    const all = parseQuotes(d.editor.text());
    const multi = (d.attachments?.length || 0) > 1;
    return all.filter(q => !q.filename || q.filename === currentFile || !multi);
  }

  function applyPdfHighlightsForPage(pageNum) {
    if (mode !== 'pdf') return;
    const pageEl = pane.querySelector(`.page[data-page-number="${pageNum}"] .textLayer`);
    if (!pageEl) return;
    clearHighlights(pageEl, 'mark.sv-quote-hl');
    const quotes = quotesForCurrentFile().filter(q => q.anchor.kind === 'page' && q.anchor.number === pageNum);
    for (const q of quotes) {
      highlightTextNodes(pageEl, q.text, {
        className: 'sv-quote-hl',
        lineNum: q.line,
        onClick: line => { setRail('notes'); d.editor.goToLine(line); }
      });
    }
  }

  function applyAllHighlights() {
    if (!active) return;
    refreshQuoteActions();
    if (mode === 'pdf') {
      for (const elPage of pane.querySelectorAll('.page[data-page-number]')) {
        const num = Number(elPage.dataset.pageNumber);
        if (num) applyPdfHighlightsForPage(num);
      }
      return;
    }
    if (mode === 'markdown') {
      const docBox = pane.querySelector('.sv-md-doc');
      if (!docBox) return;
      clearHighlights(docBox, 'mark.sv-quote-hl');
      const quotes = quotesForCurrentFile();
      const parts = [...docBox.querySelectorAll('.sv-part')];
      for (const q of quotes) {
        let targetEl = null;
        if (q.anchor.kind === 'page' || q.anchor.kind === 'slide') {
          targetEl = parts.find(p => Number(p.dataset.part) === q.anchor.number);
        } else if (q.anchor.kind === 'section') {
          const want = q.anchor.heading.toLowerCase();
          targetEl = parts.find(p => (p.dataset.heading || '').toLowerCase() === want)
            || parts.find(p => (p.dataset.heading || '').toLowerCase().includes(want));
        }
        let marks = targetEl ? highlightTextNodes(targetEl, q.text, {
          className: 'sv-quote-hl',
          lineNum: q.line,
          onClick: line => { setRail('notes'); d.editor.goToLine(line); }
        }) : [];
        if (!marks.length) {
          for (const p of parts) {
            if (p === targetEl) continue;
            marks = highlightTextNodes(p, q.text, {
              className: 'sv-quote-hl',
              lineNum: q.line,
              onClick: line => { setRail('notes'); d.editor.goToLine(line); }
            });
            if (marks.length) break;
          }
        }
      }
    }
  }

  function scheduleHighlights() {
    clearTimeout(hlTimer);
    hlTimer = setTimeout(applyAllHighlights, 220);
  }

  function flashHighlight(lineNum) {
    const marks = pane.querySelectorAll(`mark.sv-quote-hl[data-quote-line="${lineNum}"]`);
    if (!marks.length) return false;
    marks[0].scrollIntoView({ behavior: 'smooth', block: 'center' });
    for (const m of marks) {
      m.dataset.flash = '';
      setTimeout(() => { delete m.dataset.flash; }, 1400);
    }
    return true;
  }

  // ── Capture current text selection inside the document viewer ──
  function readViewerSelection() {
    const sel = window.getSelection();
    if (!sel || sel.isCollapsed || !sel.rangeCount) return null;
    const text = sel.toString().trim();
    if (!text) return null;
    const range = sel.getRangeAt(0);
    if (!pane.contains(range.commonAncestorContainer)) return null;
    const node = range.startContainer.nodeType === 1 ? range.startContainer : range.startContainer.parentElement;
    if (mode === 'pdf') {
      const pageEl = node?.closest?.('.page[data-page-number]');
      const part = Number(pageEl?.dataset.pageNumber) || currentPart || 1;
      return { text, part, anchor: { kind: 'page', number: part }, rect: range.getBoundingClientRect() };
    }
    if (mode === 'markdown') {
      const partEl = node?.closest?.('.sv-part');
      const part = Number(partEl?.dataset.part) || currentPart || 1;
      const heading = partEl?.dataset.heading || `Section ${part}`;
      const anchor = unit === 'slide' ? { kind: 'slide', number: part }
        : unit === 'section' ? { kind: 'section', heading }
        : { kind: 'page', number: part };
      return { text, part, anchor, rect: range.getBoundingClientRect() };
    }
    return null;
  }

  function quoteCurrentSelection() {
    const picked = readViewerSelection() || pendingSelection;
    if (!picked?.text) {
      status('Select a passage in the document first, then press Q or click Quote.');
      return false;
    }
    const multi = (d.attachments?.length || 0) > 1;
    const line = formatQuote({
      filename: multi ? currentFile : null,
      anchor: picked.anchor,
      text: picked.text
    });
    hideFloatQuote();
    pendingSelection = null;
    window.getSelection()?.removeAllRanges();
    setRail('notes');
    d.editor.insertQuote(line);
    scheduleHighlights();
    return true;
  }

  function showFloatQuote(picked) {
    hideFloatQuote();
    if (!picked?.rect || (!picked.rect.width && !picked.rect.height)) return;
    const btn = el('button', 'sv-float-quote');
    btn.type = 'button';
    btn.append(icon('quote'), 'Quote', el('kbd', '', 'Q'));
    const paneRect = pane.getBoundingClientRect();
    const top = Math.max(48, picked.rect.top - paneRect.top - 36);
    const left = Math.max(12, Math.min(paneRect.width - 120, picked.rect.left - paneRect.left + picked.rect.width / 2 - 48));
    btn.style.top = `${Math.round(top)}px`;
    btn.style.left = `${Math.round(left)}px`;
    btn.onmousedown = e => { e.preventDefault(); e.stopPropagation(); quoteCurrentSelection(); };
    pane.append(btn);
    floatBtn = btn;
  }

  // ── Jump to a page/part or to a quote line ──
  async function goToPart(partNum, { filename = null } = {}) {
    if (!active) return;
    if (filename && filename !== currentFile && d.attachments?.some(a => a.filename === filename)) {
      await openAttachment(filename, { initialPart: partNum });
      return;
    }
    const p = Math.max(1, Math.min(totalParts || partNum, Number(partNum) || 1));
    setCurrentPart(p);
    if (mode === 'pdf' && pdfViewer) {
      pdfViewer.currentPageNumber = p;
    } else if (mode === 'markdown') {
      const target = pane.querySelector(`.sv-part[data-part="${p}"]`);
      target?.scrollIntoView({ behavior: 'smooth', block: 'start' });
    }
  }

  async function jumpToQuoteLine(lineNum) {
    const quotes = parseQuotes(d.editor.text());
    const q = quotes.find(x => x.line === lineNum);
    if (!q || !d.opened) return;
    const targetFile = q.filename && d.attachments?.some(a => a.filename === q.filename)
      ? q.filename : currentFile;
    if (targetFile !== currentFile) {
      await openAttachment(targetFile);
    }
    let targetPart = q.anchor.kind === 'page' || q.anchor.kind === 'slide' ? q.anchor.number : null;
    if (q.anchor.kind === 'section' && mode === 'markdown') {
      const want = q.anchor.heading.toLowerCase();
      const hitEl = [...pane.querySelectorAll('.sv-part')].find(p => (p.dataset.heading || '').toLowerCase() === want)
        || [...pane.querySelectorAll('.sv-part')].find(p => (p.dataset.heading || '').toLowerCase().includes(want));
      if (hitEl) targetPart = Number(hitEl.dataset.part);
    }
    // Check if the quote is still at the hinted location, else ask core `attachment.locate` to find it!
    try {
      const loc = await api.attachmentLocate(d.opened.vault, d.opened.id, currentFile, q.text, targetPart);
      if (loc?.part) targetPart = loc.part;
    } catch { /* fallback to anchor part */ }

    if (targetPart) {
      await goToPart(targetPart);
      setTimeout(() => {
        applyAllHighlights();
        flashHighlight(lineNum);
      }, mode === 'pdf' ? 250 : 60);
    }
  }

  // ── Analyze modal (Step 4): read-only Agent proposals, user ticks what to apply via Writer ──
  async function runAnalyze() {
    const f = d.opened;
    if (!f) return;
    const key = `${f.vault}\0${f.id}`;
    const backdrop = el('div', 'analyze-modal');
    backdrop.setAttribute('role', 'dialog');
    backdrop.setAttribute('aria-modal', 'true');
    backdrop.setAttribute('aria-label', 'Analyze document');

    const box = el('div', 'analyze-box');
    const head = el('div', 'analyze-head');
    head.append(icon('sparkles'), el('h2', '', 'Analyze document'));
    const closeBtn = el('button', 'inbox-picker-close');
    closeBtn.type = 'button'; closeBtn.setAttribute('aria-label', 'Close');
    closeBtn.append(icon('x'));
    head.append(closeBtn);

    const body = el('div', 'analyze-body');
    const foot = el('div', 'analyze-foot');
    let closed = false;
    const close = () => {
      if (closed) return;
      closed = true;
      api.agentAnalyzeStop(key).catch(() => {});
      backdrop.remove();
    };
    closeBtn.onclick = close;
    backdrop.onclick = e => { if (e.target === backdrop) close(); };
    const onKey = e => { if (e.key === 'Escape') { e.preventDefault(); close(); } };
    document.addEventListener('keydown', onKey);
    const origClose = close;
    const finishClose = () => { document.removeEventListener('keydown', onKey); origClose(); };
    closeBtn.onclick = finishClose;
    backdrop.onclick = e => { if (e.target === backdrop) finishClose(); };

    body.append(el('p', 'analyze-loading', `Reading “${f.title || currentFile}” and checking related notes in your vault…`));
    const cancelBtn = document.createElement('aihio-button');
    cancelBtn.setAttribute('variant', 'outline'); cancelBtn.setAttribute('size', 'sm');
    cancelBtn.append('Cancel');
    cancelBtn.onclick = finishClose;
    foot.append(cancelBtn);

    box.append(head, body, foot);
    backdrop.append(box);
    document.body.append(backdrop);

    let reply;
    try {
      reply = await api.agentAnalyze({ vault: f.vault, id: f.id, filename: currentFile });
    } catch (err) {
      reply = { error: String(err?.message ?? err) };
    }
    if (closed) return;

    if (reply?.error) {
      body.replaceChildren(el('p', 'agent-error', reply.error));
      foot.replaceChildren();
      if (reply.signedOut && onOpenAgent) {
        const signInBtn = document.createElement('aihio-button');
        signInBtn.setAttribute('size', 'sm');
        signInBtn.append('Open Agent to sign in');
        signInBtn.onclick = () => { finishClose(); onOpenAgent(); };
        foot.append(signInBtn);
      }
      foot.append(cancelBtn);
      return;
    }

    const p = reply.proposal || { summary: '', tags: [], links: [], todos: [] };
    const vaultRows = rowsIn(f.vault);
    const resolvedLinks = (p.links || []).map(l => {
      const target = vaultRows.find(r => r.id === l.id);
      return target ? { ...l, title: target.title || 'Untitled', type: target.type } : null;
    }).filter(Boolean);

    const hasAnything = !!p.summary || (p.tags?.length > 0) || resolvedLinks.length > 0 || (p.todos?.length > 0);
    body.replaceChildren();
    if (!hasAnything) {
      body.append(el('p', 'quiet', 'The agent didn’t propose any new summary, tags, links or todos for this document.'));
      foot.replaceChildren(cancelBtn);
      return;
    }

    const checks = { summary: null, tags: [], links: [], todos: [] };
    body.append(el('p', 'analyze-note', 'Tick what you want to keep. Nothing is written until you click Apply selected.'));

    if (p.summary) {
      const sec = el('section', 'analyze-sec');
      const lab = el('label', 'analyze-check');
      const cb = el('input'); cb.type = 'checkbox'; cb.checked = true;
      checks.summary = { cb, text: p.summary };
      lab.append(cb, el('strong', '', 'Add summary to Notes'));
      sec.append(lab, el('p', 'analyze-summary', p.summary));
      body.append(sec);
    }

    if (p.tags?.length) {
      const sec = el('section', 'analyze-sec');
      sec.append(el('h3', '', 'Tags'));
      const row = el('div', 'analyze-tags');
      for (const tag of p.tags) {
        const lab = el('label', 'analyze-tag-check');
        const cb = el('input'); cb.type = 'checkbox'; cb.checked = true;
        checks.tags.push({ cb, tag });
        lab.append(cb, el('span', '', `#${tag}`));
        row.append(lab);
      }
      sec.append(row);
      body.append(sec);
    }

    if (resolvedLinks.length) {
      const sec = el('section', 'analyze-sec');
      sec.append(el('h3', '', 'Links'));
      const ul = el('ul', 'analyze-list');
      for (const l of resolvedLinks) {
        const li = el('li');
        const lab = el('label', 'analyze-check');
        const cb = el('input'); cb.type = 'checkbox'; cb.checked = true;
        checks.links.push({ cb, link: l });
        const txt = el('span', 'analyze-item-text');
        txt.append(el('strong', '', l.title));
        if (l.reason) txt.append(el('span', 'analyze-reason', l.reason));
        lab.append(cb, typeIcon(l.type), txt);
        li.append(lab);
        ul.append(li);
      }
      sec.append(ul);
      body.append(sec);
    }

    if (p.todos?.length) {
      const sec = el('section', 'analyze-sec');
      sec.append(el('h3', '', 'Todos found in document'));
      const ul = el('ul', 'analyze-list');
      for (const t of p.todos) {
        const li = el('li');
        const lab = el('label', 'analyze-check');
        const cb = el('input'); cb.type = 'checkbox'; cb.checked = true;
        checks.todos.push({ cb, todo: t });
        const txt = el('span', 'analyze-item-text');
        txt.append(el('strong', '', t.title));
        if (t.body) txt.append(el('span', 'analyze-reason', t.body));
        lab.append(cb, typeIcon('todo'), txt);
        li.append(lab);
        ul.append(li);
      }
      sec.append(ul);
      body.append(sec);
    }

    const applyBtn = document.createElement('aihio-button');
    applyBtn.setAttribute('size', 'sm');
    applyBtn.append(icon('check'), 'Apply selected');
    applyBtn.onclick = async () => {
      const selected = {
        summary: checks.summary?.cb.checked ? checks.summary.text : '',
        tags: checks.tags.filter(x => x.cb.checked).map(x => x.tag),
        links: checks.links.filter(x => x.cb.checked).map(x => x.link),
        todos: checks.todos.filter(x => x.cb.checked).map(x => x.todo)
      };
      applyBtn.setAttribute('loading', '');
      try {
        await onApplyAnalyze(d, selected);
        finishClose();
      } catch (err) {
        status(err);
        applyBtn.removeAttribute('loading');
      }
    };
    foot.replaceChildren(cancelBtn, applyBtn);
  }

  // ── Toolbar shared across PDF and Markdown viewers ──
  function buildToolbar({ isPdf, findBar }) {
    const bar = el('div', 'sv-toolbar');

    // Attachment selector when a source has >1 file
    if ((d.attachments?.length || 0) > 1) {
      const pick = el('select', 'sv-file-select');
      pick.setAttribute('aria-label', 'Attached file');
      for (const a of d.attachments) {
        pick.append(new Option(a.filename, a.filename, false, a.filename === currentFile));
      }
      pick.onchange = () => openAttachment(pick.value);
      bar.append(pick);
    } else {
      const nameSpan = el('span', 'sv-file-name', currentFile || '');
      nameSpan.title = currentFile || '';
      bar.append(nameSpan);
    }

    // Page / Slide / Section navigation
    if (isPdf || mode === 'markdown') {
      const nav = el('div', 'sv-nav');
      const prev = el('button', 'sv-btn'); prev.type = 'button'; prev.title = 'Previous'; prev.setAttribute('aria-label', 'Previous');
      prev.textContent = '‹';
      prev.onclick = () => goToPart(currentPart - 1);
      const unitLbl = el('span', 'sv-unit-label', unit === 'slide' ? 'Slide' : unit === 'section' ? '§' : 'p.');
      const pageIn = el('input', 'sv-page-input');
      Object.assign(pageIn, { type: 'text', inputMode: 'numeric', value: String(currentPart), autocomplete: 'off' });
      pageIn.setAttribute('aria-label', 'Current page');
      pageIn.onkeydown = e => {
        if (e.key === 'Enter') {
          e.preventDefault();
          const n = Number.parseInt(pageIn.value, 10);
          if (n >= 1) goToPart(n);
          pageIn.blur();
        }
      };
      const pageTot = el('span', 'sv-page-total', totalParts ? `/ ${totalParts}` : '');
      const next = el('button', 'sv-btn'); next.type = 'button'; next.title = 'Next'; next.setAttribute('aria-label', 'Next');
      next.textContent = '›';
      next.onclick = () => goToPart(currentPart + 1);
      nav.append(prev, unitLbl, pageIn, pageTot, next);
      bar.append(nav);
    }

    // Zoom & Paper/Theme toggle for PDF
    if (isPdf) {
      const zoom = el('div', 'sv-zoom');
      const out = el('button', 'sv-btn'); out.type = 'button'; out.title = 'Zoom out'; out.setAttribute('aria-label', 'Zoom out');
      out.append(icon('zoomOut'));
      out.onclick = () => { if (pdfViewer) pdfViewer.currentScale = Math.max(0.4, (pdfViewer.currentScale || 1) / 1.15); };
      const fit = el('button', 'sv-btn sv-btn-text', 'Fit'); fit.type = 'button'; fit.title = 'Fit width';
      fit.onclick = () => { if (pdfViewer) pdfViewer.currentScaleValue = 'page-width'; };
      const inn = el('button', 'sv-btn'); inn.type = 'button'; inn.title = 'Zoom in'; inn.setAttribute('aria-label', 'Zoom in');
      inn.append(icon('zoomIn'));
      inn.onclick = () => { if (pdfViewer) pdfViewer.currentScale = Math.min(4, (pdfViewer.currentScale || 1) * 1.15); };
      zoom.append(out, fit, inn);
      bar.append(zoom);

      const colorBtn = el('button', 'sv-btn sv-btn-text');
      colorBtn.type = 'button';
      const paintColorBtn = () => {
        colorBtn.textContent = pdfColorMode === 'theme' ? 'Paper' : 'Theme';
        colorBtn.title = pdfColorMode === 'theme' ? 'Switch PDF back to paper colors' : 'Recolour PDF pages to match theme';
      };
      paintColorBtn();
      colorBtn.onclick = () => {
        pdfColorMode = pdfColorMode === 'theme' ? 'paper' : 'theme';
        setPdfColorPref(pdfColorMode);
        paintColorBtn();
        openAttachment(currentFile, { initialPart: currentPart });
      };
      bar.append(colorBtn);
    }

    const spacer = el('span', 'sv-spacer');
    bar.append(spacer);

    if (findBar) {
      const findBtn = el('button', 'sv-btn'); findBtn.type = 'button'; findBtn.title = 'Find in document'; findBtn.setAttribute('aria-label', 'Find in document');
      findBtn.append(icon('search'));
      findBtn.onclick = () => {
        findBar.hidden = !findBar.hidden;
        if (!findBar.hidden) findBar.querySelector('input')?.focus();
      };
      bar.append(findBtn);
    }

    if (isPdf || mode === 'markdown') {
      const quoteBtn = el('button', 'sv-btn sv-btn-text'); quoteBtn.type = 'button';
      quoteBtn.title = 'Quote selected passage into Notes (Q)';
      quoteBtn.append(icon('quote'), 'Quote');
      quoteBtn.onmousedown = e => { e.preventDefault(); quoteCurrentSelection(); };
      bar.append(quoteBtn);
    }

    const openExt = el('button', 'sv-btn sv-btn-text'); openExt.type = 'button';
    openExt.title = 'Open original file in system viewer';
    openExt.append(icon('external'), 'Original');
    openExt.onclick = () => {
      if (d.opened && currentFile) api.openAttachment(d.opened.vault, d.opened.id, currentFile).catch(status);
    };
    bar.append(openExt);

    const askBtn = el('button', 'sv-btn sv-btn-text'); askBtn.type = 'button';
    askBtn.title = 'Ask the Agent about this document';
    askBtn.append(icon('agent'), 'Ask');
    askBtn.onclick = () => onAsk(d);
    bar.append(askBtn);

    const analyzeBtn = el('button', 'sv-btn sv-btn-text sv-btn-accent'); analyzeBtn.type = 'button';
    analyzeBtn.title = 'Ask the Agent to propose summary, tags, links and todos';
    analyzeBtn.append(icon('sparkles'), 'Analyze');
    analyzeBtn.onclick = () => runAnalyze();
    bar.append(analyzeBtn);

    return bar;
  }

  // ── In-document Find bar ──
  function buildFindBar({ onSearch, onStep, onClear }) {
    const bar = el('div', 'sv-findbar');
    bar.hidden = true;
    const input = el('input', 'sv-find-input');
    Object.assign(input, { type: 'search', placeholder: 'Find in document…', autocomplete: 'off' });
    input.setAttribute('aria-label', 'Find in document');
    const count = el('span', 'sv-find-count', '');
    const prev = el('button', 'sv-btn', '‹'); prev.type = 'button'; prev.setAttribute('aria-label', 'Previous match');
    const next = el('button', 'sv-btn', '›'); next.type = 'button'; next.setAttribute('aria-label', 'Next match');
    const close = el('button', 'sv-btn'); close.type = 'button'; close.setAttribute('aria-label', 'Close find');
    close.append(icon('x'));

    let timer = 0;
    input.oninput = () => {
      clearTimeout(timer);
      timer = setTimeout(() => onSearch(input.value, count), 120);
    };
    input.onkeydown = e => {
      if (e.key === 'Enter') { e.preventDefault(); onStep(e.shiftKey ? -1 : 1, input.value, count); }
      else if (e.key === 'Escape') { e.preventDefault(); bar.hidden = true; onClear(count); }
    };
    prev.onclick = () => onStep(-1, input.value, count);
    next.onclick = () => onStep(1, input.value, count);
    close.onclick = () => { bar.hidden = true; onClear(count); };
    bar.append(input, count, prev, next, close);
    return bar;
  }

  // ── Open PDF attachment ──
  async function openPdf(filename, initialPart, seq) {
    const f = d.opened;
    mode = 'pdf';
    unit = 'page';
    currentPart = initialPart || 1;
    totalParts = 0;

    let findBar;
    findBar = buildFindBar({
      onSearch(query, countEl) {
        if (!pdfEventBus) return;
        if (!query) { countEl.textContent = ''; pdfEventBus.dispatch('find', { type: '', query: '' }); return; }
        pdfEventBus.dispatch('find', { type: '', query, caseSensitive: false, highlightAll: true, findPrevious: false, phraseSearch: true });
      },
      onStep(dir, query, countEl) {
        if (!pdfEventBus || !query) return;
        pdfEventBus.dispatch('find', { type: 'again', query, caseSensitive: false, highlightAll: true, findPrevious: dir < 0, phraseSearch: true });
        void countEl;
      },
      onClear(countEl) {
        countEl.textContent = '';
        pdfEventBus?.dispatch('find', { type: '', query: '' });
      }
    });

    const toolbar = buildToolbar({ isPdf: true, findBar });
    const host = el('div', 'sv-pdf-host');
    const container = el('div', 'sv-pdf-container');
    const viewerDiv = el('div', 'pdfViewer');
    container.append(viewerDiv);
    const loadingMsg = el('p', 'sv-status', 'Loading PDF…');
    host.append(loadingMsg, container);
    pane.replaceChildren(toolbar, findBar, host);

    let bytes;
    try {
      bytes = await api.attachmentBytes(f.vault, f.id, filename);
    } catch (err) {
      if (seq !== loadSeq) return;
      loadingMsg.textContent = String(err?.message ?? err);
      loadingMsg.dataset.tone = 'error';
      return;
    }
    if (seq !== loadSeq || !active || d.opened !== f) return;

    // Also fetch core-extracted parts asynchronously in the background so Related gets clean text!
    api.attachmentText(f.vault, f.id, filename, { readyOnly: true }).then(res => {
      if (seq === loadSeq && active && currentFile === filename && res?.status === 'ready') {
        extractedParts = res.parts;
        onPageChange();
      }
    }).catch(() => {});

    const eventBus = new EventBus();
    const linkService = new PDFLinkService({ eventBus });
    const findController = new PDFFindController({ eventBus, linkService });
    const pageColors = pdfColorMode === 'theme' ? themePageColors() : null;

    const viewer = new PDFViewer({
      container,
      viewer: viewerDiv,
      eventBus,
      linkService,
      findController,
      removePageBorders: true,
      annotationMode: pdfjsLib.AnnotationMode.ENABLE,
      annotationEditorMode: 0,
      pageColors
    });
    linkService.setViewer(viewer);

    pdfEventBus = eventBus;
    pdfLinkService = linkService;
    pdfFindController = findController;
    pdfViewer = viewer;

    eventBus.on('pagesinit', () => {
      if (seq !== loadSeq) return;
      viewer.currentScaleValue = 'page-width';
      if (initialPart && initialPart > 1 && initialPart <= viewer.pagesCount) {
        viewer.currentPageNumber = initialPart;
      }
      setCurrentPart(viewer.currentPageNumber, { persist: false });
      notifyPageChanged();
    });
    eventBus.on('pagechanging', ({ pageNumber }) => {
      if (seq !== loadSeq) return;
      setCurrentPart(pageNumber, { persist: true });
    });
    eventBus.on('textlayerrendered', ({ pageNumber }) => {
      if (seq !== loadSeq) return;
      applyPdfHighlightsForPage(pageNumber);
    });
    eventBus.on('updatefindmatchescount', ({ matchesCount }) => {
      const c = findBar.querySelector('.sv-find-count');
      if (c) c.textContent = matchesCount?.total ? `${matchesCount.current} / ${matchesCount.total}` : '';
    });
    eventBus.on('updatefindcontrolstate', ({ matchesCount }) => {
      const c = findBar.querySelector('.sv-find-count');
      if (c) c.textContent = matchesCount?.total ? `${matchesCount.current} / ${matchesCount.total}` : '';
    });

    // External PDF links go through the narrow `open-external` IPC (http/https/mailto only).
    container.addEventListener('click', e => {
      const a = e.target?.closest?.('a[href]');
      if (!a) return;
      const href = a.getAttribute('href') || '';
      if (!href || href.startsWith('#')) return;
      e.preventDefault();
      e.stopPropagation();
      api.openExternal(href).catch(status);
    });

    container.addEventListener('mouseup', () => {
      setTimeout(() => {
        const picked = readViewerSelection();
        pendingSelection = picked;
        if (picked) showFloatQuote(picked); else hideFloatQuote();
      }, 10);
    });

    try {
      const task = pdfjsLib.getDocument({
        data: bytes,
        isEvalSupported: false,
        enableScripting: false,
        useWasm: false,
        useWorkerFetch: false,
        disableWorkerRendering: true,
        useSystemFonts: true
      });
      pdfTask = task;
      const doc = await task.promise;
      if (seq !== loadSeq || !active || d.opened !== f) { doc.destroy().catch(() => {}); return; }
      pdfDoc = doc;
      totalParts = doc.numPages;
      loadingMsg.remove();
      viewer.setDocument(doc);
      linkService.setDocument(doc, null);
      setCurrentPart(Math.min(currentPart, totalParts), { persist: false });
    } catch (err) {
      if (seq !== loadSeq) return;
      loadingMsg.textContent = `Could not display PDF: ${String(err?.message ?? err)}`;
      loadingMsg.dataset.tone = 'error';
    }
  }

  // ── Open Markdown-rendered document (docx, pptx, HTML, md, txt) ──
  async function openMarkdownDoc(filename, initialPart, seq) {
    const f = d.opened;
    mode = 'markdown';
    currentPart = initialPart || 1;
    totalParts = 0;

    let docBox;
    const findBar = buildFindBar({
      onSearch(query, countEl) {
        if (!docBox) return;
        clearHighlights(docBox, 'mark.sv-find-hl');
        findMatches = []; findIndex = -1;
        if (!query.trim()) { countEl.textContent = ''; return; }
        const re = new RegExp(query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'gi');
        const walker = document.createTreeWalker(docBox, NodeFilter.SHOW_TEXT);
        const textNodes = [];
        let node;
        while ((node = walker.nextNode())) {
          if (node.nodeValue && !node.parentElement?.closest('mark.sv-find-hl')) textNodes.push(node);
        }
        for (const tn of textNodes) {
          const val = tn.nodeValue;
          const matches = [...val.matchAll(re)];
          if (!matches.length) continue;
          const frag = document.createDocumentFragment();
          let last = 0;
          for (const m of matches) {
            if (m.index > last) frag.append(document.createTextNode(val.slice(last, m.index)));
            const mk = el('mark', 'sv-find-hl', m[0]);
            frag.append(mk);
            findMatches.push(mk);
            last = m.index + m[0].length;
          }
          if (last < val.length) frag.append(document.createTextNode(val.slice(last)));
          tn.replaceWith(frag);
        }
        if (findMatches.length) {
          findIndex = 0;
          findMatches[0].dataset.current = '';
          findMatches[0].scrollIntoView({ behavior: 'smooth', block: 'center' });
          countEl.textContent = `1 / ${findMatches.length}`;
        } else {
          countEl.textContent = '0 / 0';
        }
      },
      onStep(dir, _query, countEl) {
        if (!findMatches.length) return;
        if (findIndex >= 0 && findMatches[findIndex]) delete findMatches[findIndex].dataset.current;
        findIndex = (findIndex + dir + findMatches.length) % findMatches.length;
        findMatches[findIndex].dataset.current = '';
        findMatches[findIndex].scrollIntoView({ behavior: 'smooth', block: 'center' });
        countEl.textContent = `${findIndex + 1} / ${findMatches.length}`;
      },
      onClear(countEl) {
        if (docBox) clearHighlights(docBox, 'mark.sv-find-hl');
        findMatches = []; findIndex = -1;
        countEl.textContent = '';
      }
    });

    const toolbar = buildToolbar({ isPdf: false, findBar });
    const scroller = el('div', 'sv-md-scroll');
    docBox = el('div', 'sv-md-doc');
    const loadingMsg = el('p', 'sv-status', 'Loading document text…');
    scroller.append(loadingMsg, docBox);
    pane.replaceChildren(toolbar, findBar, scroller);

    let res;
    try {
      res = await api.attachmentText(f.vault, f.id, filename, { readyOnly: true });
    } catch (err) {
      if (seq !== loadSeq) return;
      loadingMsg.textContent = String(err?.message ?? err);
      loadingMsg.dataset.tone = 'error';
      return;
    }
    if (seq !== loadSeq || !active || d.opened !== f) return;

    if (res.status !== 'ready' || !res.parts?.length) {
      const att = d.attachments?.find(a => a.filename === filename);
      openFallback(filename, att, res.status === 'pending'
        ? 'Extracting text in the background… Click Refresh in a moment or open the original file.'
        : res.error ? `Could not extract text: ${res.error}` : 'No extractable text for this file format.', res.status === 'pending');
      return;
    }

    unit = res.unit || 'section';
    extractedParts = res.parts;
    totalParts = res.parts.length;
    loadingMsg.remove();

    const onLink = href => api.openExternal(href).catch(status);
    for (const part of res.parts) {
      const sec = el('section', unit === 'slide' ? 'sv-part sv-slide-card' : 'sv-part sv-section');
      sec.dataset.part = String(part.part);
      if (unit === 'section') sec.dataset.heading = headingOfPart(part.text, part.part);
      if (unit === 'slide') {
        sec.append(el('header', 'sv-slide-head', `Slide ${part.part}`));
      }
      const bodyEl = el('div', 'sv-part-body');
      renderMarkdownBlock(bodyEl, part.text, onLink);
      sec.append(bodyEl);
      docBox.append(sec);
    }

    // Track visible part with IntersectionObserver so reading position & Related follow scroll.
    const visible = new Map();
    observer = new IntersectionObserver(entries => {
      for (const e of entries) {
        const num = Number(e.target.dataset.part);
        if (e.isIntersecting) visible.set(num, e.intersectionRatio);
        else visible.delete(num);
      }
      if (visible.size) {
        const topPart = [...visible.entries()].sort((a, b) => a[0] - b[0])[0][0];
        setCurrentPart(topPart, { persist: true });
      }
    }, { root: scroller, threshold: [0.1, 0.5] });
    for (const sec of docBox.querySelectorAll('.sv-part')) observer.observe(sec);

    scroller.addEventListener('mouseup', () => {
      setTimeout(() => {
        const picked = readViewerSelection();
        pendingSelection = picked;
        if (picked) showFloatQuote(picked); else hideFloatQuote();
      }, 10);
    });

    setCurrentPart(Math.min(currentPart, totalParts), { persist: false });
    if (currentPart > 1) {
      docBox.querySelector(`.sv-part[data-part="${currentPart}"]`)?.scrollIntoView({ block: 'start' });
    }
    applyAllHighlights();
    notifyPageChanged();
  }

  // ── Fallback / Image viewer ──
  function openFallback(filename, att, message, allowRefresh = false) {
    mode = IMAGE_EXT.test(filename) ? 'image' : 'fallback';
    const toolbar = buildToolbar({ isPdf: false, findBar: null });
    const wrap = el('div', 'sv-fallback');
    if (mode === 'image' && d.opened) {
      const token = vaultToken(d.opened.vault);
      if (token) {
        const img = el('img', 'sv-image');
        img.src = `parc-attach://${token}/${d.opened.id}/${encodeURIComponent(filename)}`;
        img.alt = filename;
        wrap.append(img);
      }
    } else {
      wrap.append(
        icon('file', 'icon sv-fallback-icon'),
        el('strong', '', filename),
        el('p', 'quiet', `${att ? bytesLabel(att.size) + ' · ' : ''}${message}`)
      );
      const btns = el('div', 'sv-fallback-actions');
      const openBtn = document.createElement('aihio-button');
      openBtn.setAttribute('variant', 'outline'); openBtn.setAttribute('size', 'sm');
      openBtn.append(icon('external'), 'Open original');
      openBtn.onclick = () => api.openAttachment(d.opened.vault, d.opened.id, filename).catch(status);
      btns.append(openBtn);
      if (allowRefresh) {
        const refBtn = document.createElement('aihio-button');
        refBtn.setAttribute('variant', 'ghost'); refBtn.setAttribute('size', 'sm');
        refBtn.append('Refresh');
        refBtn.onclick = () => openAttachment(filename, { initialPart: currentPart });
        btns.append(refBtn);
      }
      wrap.append(btns);
    }
    pane.replaceChildren(toolbar, wrap);
  }

  async function openAttachment(filename, { initialPart = null } = {}) {
    const seq = ++loadSeq;
    teardownViewer();
    currentFile = filename;
    const f = d.opened;
    if (!f) return;

    let part = initialPart;
    if (!part) {
      const positions = await api.readingPositions().catch(() => ({}));
      const saved = positions[JSON.stringify([f.vault, f.id])];
      if (saved && (!saved.filename || saved.filename === filename)) part = saved.part;
    }
    if (seq !== loadSeq) return;

    const att = d.attachments?.find(a => a.filename === filename);
    if (PDF_EXT.test(filename)) {
      await openPdf(filename, part || 1, seq);
    } else if (IMAGE_EXT.test(filename)) {
      openFallback(filename, att, '');
    } else {
      await openMarkdownDoc(filename, part || 1, seq);
    }
    refreshQuoteActions();
  }

  // Keyboard shortcut inside viewer pane: `q` quotes current selection, Ctrl+F opens in-doc find.
  pane.addEventListener('keydown', e => {
    if (!active) return;
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'f') {
      const fb = pane.querySelector('.sv-findbar');
      if (fb) {
        e.preventDefault(); e.stopPropagation();
        fb.hidden = false;
        fb.querySelector('input')?.focus();
      }
      return;
    }
    if (!e.ctrlKey && !e.metaKey && !e.altKey && e.key.toLowerCase() === 'q'
      && !e.target.closest('input, textarea, select')) {
      if (readViewerSelection() || pendingSelection) {
        e.preventDefault(); e.stopPropagation();
        quoteCurrentSelection();
      }
    }
  });

  // Also allow pressing `q` anywhere in the window when a selection lives inside this tab's viewer!
  const onDocKey = e => {
    if (!active || !pane.isConnected) return;
    if (e.ctrlKey || e.metaKey || e.altKey || e.isComposing) return;
    if (e.key.toLowerCase() !== 'q') return;
    if (e.target.closest('input, textarea, select, [contenteditable=true]')) return;
    const picked = readViewerSelection();
    if (picked) {
      e.preventDefault(); e.stopPropagation();
      pendingSelection = picked;
      quoteCurrentSelection();
    }
  };
  document.addEventListener('keydown', onDocKey);

  return {
    isActive: () => active,
    currentFilename: () => currentFile,
    /** Extracted text of the currently visible page/slide/section, for Related recall seeding. */
    visibleText() {
      if (!active || !extractedParts?.length) return '';
      const entry = extractedParts.find(p => p.part === currentPart) || extractedParts[currentPart - 1];
      return entry?.text ? String(entry.text).slice(0, 6000) : '';
    },
    /** Called when `d.opened` or `d.attachments` changes. */
    async sync({ initialPart = null, filename = null, textUpdated = false } = {}) {
      const f = d.opened;
      const atts = d.attachments || [];
      if (!f || f.type !== 'source' || !atts.length) {
        deactivate();
        return;
      }
      const wanted = (filename && atts.some(a => a.filename === filename))
        ? filename
        : (currentFile && atts.some(a => a.filename === currentFile))
          ? currentFile
          : atts[0].filename;

      const justActivated = !active;
      active = true;
      d.ui.scroller.dataset.sourceMode = 'true';
      pane.hidden = false;
      railBar.hidden = false;
      if (justActivated) setRail('notes');

      if (justActivated || wanted !== currentFile) {
        await openAttachment(wanted, { initialPart });
      } else if (textUpdated && mode === 'fallback') {
        await openAttachment(wanted, { initialPart: currentPart });
      } else if (textUpdated && mode === 'pdf' && !extractedParts) {
        const seq = loadSeq;
        const res = await api.attachmentText(f.vault, f.id, wanted, { readyOnly: true }).catch(() => null);
        if (seq === loadSeq && res?.status === 'ready') { extractedParts = res.parts; onPageChange(); }
      } else if (initialPart) {
        await goToPart(initialPart);
      } else {
        applyAllHighlights();
      }
    },
    onNotesChanged: scheduleHighlights,
    goToPart,
    jumpToQuoteLine,
    quoteSelection: quoteCurrentSelection,
    analyze: runAnalyze,
    destroy() {
      document.removeEventListener('keydown', onDocKey);
      relatedObserver?.disconnect();
      deactivate();
    }
  };
}
