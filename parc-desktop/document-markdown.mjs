function el(tag, className = '', text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
}

// ── Safe Markdown-to-DOM renderer for extracted docx, pptx, HTML, md, txt ──
// Never uses innerHTML on document text; only http/https/mailto links become clickable anchors.
function renderInline(parent, text, onLink) {
  const re = /(\*\*[^*\n]+\*\*|__[^_\n]+__|`[^`\n]+`|\*[^*\n]+\*|_[^_\n]+_|\[([^\]\n]+)\]\(([^)\s]+)\))/g;
  let last = 0;
  for (const m of text.matchAll(re)) {
    if (m.index > last) parent.append(document.createTextNode(text.slice(last, m.index)));
    const tok = m[0];
    if ((tok.startsWith('**') && tok.endsWith('**')) || (tok.startsWith('__') && tok.endsWith('__'))) {
      parent.append(el('strong', '', tok.slice(2, -2)));
    } else if (tok.startsWith('`') && tok.endsWith('`')) {
      parent.append(el('code', '', tok.slice(1, -1)));
    } else if ((tok.startsWith('*') && tok.endsWith('*')) || (tok.startsWith('_') && tok.endsWith('_'))) {
      parent.append(el('em', '', tok.slice(1, -1)));
    } else if (tok.startsWith('[')) {
      const label = m[2], href = m[3];
      if (/^(?:https?:|mailto:)/i.test(href)) {
        const a = el('a', 'sv-ext-link', label);
        a.href = href;
        a.title = href;
        a.onclick = e => { e.preventDefault(); onLink(href); };
        parent.append(a);
      } else {
        parent.append(document.createTextNode(label));
      }
    }
    last = m.index + tok.length;
  }
  if (last < text.length) parent.append(document.createTextNode(text.slice(last)));
}

function isTableSep(line) {
  return /^\s*\|?(?:\s*:?-{3,}:?\s*\|)+\s*:?-{3,}:?\s*\|?\s*$/.test(line);
}

function splitTableRow(line) {
  const trimmed = line.trim().replace(/^\|/, '').replace(/\|$/, '');
  return trimmed.split('|').map(c => c.trim());
}

const FENCE = /^\s*(```|~~~)/;
const RULE = /^\s*(?:---+|\*\*\*+|___+)\s*$/;
const HEADING = /^\s*(#{1,6})\s+(.+?)(?:\s+#+)?\s*$/;
const BULLET = /^\s*[-*+]\s+(.*)$/;
const ORDERED = /^\s*\d+[.)]\s+(.*)$/;

function blockKind(lines, i) {
  const line = lines[i];
  if (!line.trim()) return 'blank';
  if (FENCE.test(line)) return 'fence';
  if (RULE.test(line)) return 'rule';
  if (HEADING.test(line)) return 'heading';
  if (line.includes('|') && i + 1 < lines.length && isTableSep(lines[i + 1])) return 'table';
  if (/^\s*>/.test(line)) return 'quote';
  if (BULLET.test(line)) return 'bullets';
  if (ORDERED.test(line)) return 'ordered';
  return 'paragraph';
}

export function renderMarkdownBlock(container, md, onLink) {
  const lines = String(md ?? '').replace(/\r\n?/g, '\n').split('\n');
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    const kind = blockKind(lines, i);
    if (kind === 'blank') { i++; continue; }

    // Fenced code block
    if (kind === 'fence') {
      const mark = FENCE.exec(line)[1];
      const codeLines = [];
      i++;
      while (i < lines.length && !lines[i].trim().startsWith(mark)) {
        codeLines.push(lines[i]);
        i++;
      }
      if (i < lines.length) i++;
      const pre = el('pre', 'sv-code');
      pre.append(el('code', '', codeLines.join('\n')));
      container.append(pre);
      continue;
    }

    // Horizontal rule
    if (kind === 'rule') {
      container.append(document.createElement('hr'));
      i++;
      continue;
    }

    // Heading
    if (kind === 'heading') {
      const hm = HEADING.exec(line);
      const level = Math.min(6, hm[1].length);
      const h = document.createElement(`h${level}`);
      renderInline(h, hm[2], onLink);
      container.append(h);
      i++;
      continue;
    }

    // Pipe table
    if (kind === 'table') {
      const table = el('table', 'sv-table');
      const thead = document.createElement('thead');
      const tr = document.createElement('tr');
      for (const cell of splitTableRow(line)) {
        const th = document.createElement('th');
        renderInline(th, cell, onLink);
        tr.append(th);
      }
      thead.append(tr);
      table.append(thead);
      i += 2;
      const tbody = document.createElement('tbody');
      while (i < lines.length && lines[i].includes('|') && lines[i].trim()) {
        const row = document.createElement('tr');
        for (const cell of splitTableRow(lines[i])) {
          const td = document.createElement('td');
          renderInline(td, cell, onLink);
          row.append(td);
        }
        tbody.append(row);
        i++;
      }
      table.append(tbody);
      container.append(table);
      continue;
    }

    // Blockquote
    if (kind === 'quote') {
      const bq = el('blockquote', 'sv-blockquote');
      const qLines = [];
      while (i < lines.length && /^\s*>/.test(lines[i])) {
        qLines.push(lines[i].replace(/^\s*>\s?/, ''));
        i++;
      }
      renderMarkdownBlock(bq, qLines.join('\n'), onLink);
      container.append(bq);
      continue;
    }

    // Unordered or ordered list
    if (kind === 'ordered' || kind === 'bullets') {
      const ordered = kind === 'ordered';
      const list = document.createElement(ordered ? 'ol' : 'ul');
      list.className = 'sv-list';
      const re = ordered ? ORDERED : BULLET;
      while (i < lines.length) {
        const m = re.exec(lines[i]);
        if (!m) break;
        const li = document.createElement('li');
        renderInline(li, m[1], onLink);
        list.append(li);
        i++;
      }
      container.append(list);
      continue;
    }

    // Paragraph (join contiguous non-structural lines)
    // Always consume the first line, even when the input resembles Markdown
    // without matching a supported block. Lookahead uses the same classifier.
    const para = [lines[i++]];
    while (i < lines.length && blockKind(lines, i) === 'paragraph') para.push(lines[i++]);
    const p = document.createElement('p');
    renderInline(p, para.join(' '), onLink);
    container.append(p);
  }
}
