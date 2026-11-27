// Quote syntax and text normalization mirroring parc-core/src/quote.rs.
// Tested against the exact same JSON fixture (parc-core/tests/fixtures/quote_normalize.json)
// so pdf.js selections and xberg extracted text always match cleanly.

const DASHES = new Set(['-', '\u2010', '\u2011', '\u2012', '\u2013', '\u2014', '\u2015', '\u2212']);
const SINGLE_QUOTES = new Set(['\u2018', '\u2019', '\u201A', '\u201B']);
const DOUBLE_QUOTES = new Set(['\u201C', '\u201D', '\u201E', '\u201F']);
const ALNUM_RE = /[\p{L}\p{N}]/u;
const SPACE_RE = /\s/u;

function isDash(ch) {
  return DASHES.has(ch);
}

function foldChar(ch) {
  if (SINGLE_QUOTES.has(ch)) return "'";
  if (DOUBLE_QUOTES.has(ch)) return '"';
  if (isDash(ch)) return '-';
  return ch;
}

/**
 * Normalize `input` and return `{ text, map }` where `map[i]` is the 0-based
 * Unicode code-point index in `input` that produced `Array.from(text)[i]`.
 */
export function normalizeWithMap(input) {
  const orig = Array.from(String(input ?? ''));
  const n = orig.length;

  // Pass 1: rejoin line-end hyphens (`<alnum><dash>[spaces]\r?\n[spaces]<alnum>`)
  // and drop soft hyphens.
  const joined = [];
  let i = 0;
  while (i < n) {
    const c = orig[i];
    if (c === '\u00AD') {
      i += 1;
      continue;
    }
    if (isDash(c) && joined.length > 0 && ALNUM_RE.test(joined[joined.length - 1][0])) {
      let j = i + 1;
      while (j < n && (orig[j] === ' ' || orig[j] === '\t')) j += 1;
      if (j < n && (orig[j] === '\n' || orig[j] === '\r')) {
        if (orig[j] === '\r' && j + 1 < n && orig[j + 1] === '\n') j += 2;
        else j += 1;
        while (j < n && (orig[j] === ' ' || orig[j] === '\t')) j += 1;
        if (j < n && ALNUM_RE.test(orig[j])) {
          i = j;
          continue;
        }
      }
    }
    joined.push([c, i]);
    i += 1;
  }

  // Pass 2: NFKC + quote/dash folding + whitespace collapsing.
  const out = [];
  const map = [];
  let prevSpace = true;

  for (const [rawCh, origIdx] of joined) {
    for (const ch of rawCh.normalize('NFKC')) {
      if (ch === '\u00AD') continue;
      const folded = foldChar(ch);
      if (SPACE_RE.test(folded)) {
        if (!prevSpace) {
          out.push(' ');
          map.push(origIdx);
          prevSpace = true;
        }
      } else {
        out.push(folded);
        map.push(origIdx);
        prevSpace = false;
      }
    }
  }

  while (out.length > 0 && out[out.length - 1] === ' ') {
    out.pop();
    map.pop();
  }

  return { text: out.join(''), map };
}

export function normalize(input) {
  return normalizeWithMap(input).text;
}

const QUOTE_RE = /^\s*>\s*(?:([^\s/\\]+?\.[A-Za-z0-9]{1,8})\s+)?(?:(p\.\s*(\d+))|(slide\s+(\d+))|(§\s*([^·\n]+?)))\s*·\s*["“](.+?)["”]\s*$/;

/**
 * Parse all document quotes in a fragment Markdown body, ignoring fenced code blocks.
 * Each quote carries `{ line, filename, anchor, text, comment }` where `comment` is the
 * contiguous non-quote text block immediately below the quote (for Promote).
 */
export function parseQuotes(body) {
  const quotes = [];
  let inFenced = false;
  const clean = String(body ?? '').replace(/^\n/, '');
  const lines = clean.split('\n');

  for (let idx = 0; idx < lines.length; idx++) {
    const line = lines[idx].replace(/\r$/, '');
    const trimmed = line.trim();
    if (trimmed.startsWith('```') || trimmed.startsWith('~~~')) {
      inFenced = !inFenced;
      continue;
    }
    if (inFenced) continue;
    const m = QUOTE_RE.exec(line);
    if (!m) continue;
    const filename = m[1] || null;
    let anchor = null;
    if (m[3]) {
      const number = Number.parseInt(m[3], 10);
      if (!number) continue;
      anchor = { kind: 'page', number };
    } else if (m[5]) {
      const number = Number.parseInt(m[5], 10);
      if (!number) continue;
      anchor = { kind: 'slide', number };
    } else if (m[7]) {
      const heading = m[7].trim();
      if (!heading) continue;
      anchor = { kind: 'section', heading };
    } else {
      continue;
    }
    const text = m[8];
    // Collect any comment paragraph right beneath the quote up to the next quote/heading/blank separator.
    const commentLines = [];
    for (let j = idx + 1; j < lines.length; j++) {
      const next = lines[j].replace(/\r$/, '');
      if (!next.trim()) {
        if (commentLines.length) break;
        continue;
      }
      if (QUOTE_RE.test(next) || /^\s*#/.test(next)) break;
      commentLines.push(next);
    }
    quotes.push({
      line: idx + 1,
      filename,
      anchor,
      text,
      comment: commentLines.join('\n').trim()
    });
  }
  return quotes;
}

/** Format a quote line for insertion into Notes. */
export function formatQuote({ filename = null, anchor, text }) {
  const prefix = filename ? `${filename} ` : '';
  let loc = '';
  if (anchor.kind === 'page') loc = `p. ${anchor.number}`;
  else if (anchor.kind === 'slide') loc = `slide ${anchor.number}`;
  else if (anchor.kind === 'section') loc = `§ ${anchor.heading}`;
  const clean = String(text ?? '')
    .trim()
    .replace(/\s+/g, ' ')
    .replace(/"/g, "'");
  return `> ${prefix}${loc} · "${clean}"`;
}

function searchOrder(len, hint1Based) {
  const order = [];
  const seen = new Array(len).fill(false);
  if (Number.isInteger(hint1Based) && hint1Based >= 1 && hint1Based <= len) {
    const hint = hint1Based - 1;
    order.push(hint);
    seen[hint] = true;
    for (let delta = 1; delta < len; delta++) {
      if (hint - delta >= 0 && !seen[hint - delta]) {
        order.push(hint - delta);
        seen[hint - delta] = true;
      }
      if (hint + delta < len && !seen[hint + delta]) {
        order.push(hint + delta);
        seen[hint + delta] = true;
      }
    }
  }
  for (let i = 0; i < len; i++) {
    if (!seen[i]) order.push(i);
  }
  return order;
}

/** Find a quote inside a single text string; returns `{ offset, length }` in code-point indices or null. */
export function findInText(part, quote) {
  const needle = normalize(quote);
  if (!needle) return null;
  const { text: norm, map } = normalizeWithMap(part);
  if (!norm) return null;

  const exactIdx = norm.indexOf(needle);
  if (exactIdx >= 0) {
    const charStart = Array.from(norm.slice(0, exactIdx)).length;
    const charLen = Array.from(needle).length;
    const origStart = map[charStart];
    const origEnd = map[charStart + charLen - 1] + 1;
    return { offset: origStart, length: Math.max(0, origEnd - origStart) };
  }

  const needleLower = needle.toLowerCase();
  const normChars = Array.from(norm);
  let lower = '';
  const lowerToOrig = [];
  for (let i = 0; i < normChars.length; i++) {
    const lc = normChars[i].toLowerCase();
    for (const ch of Array.from(lc)) {
      lower += ch;
      lowerToOrig.push(map[i]);
    }
  }
  const ciIdx = lower.indexOf(needleLower);
  if (ciIdx < 0) return null;
  const charStart = Array.from(lower.slice(0, ciIdx)).length;
  const charLen = Array.from(needleLower).length;
  const origStart = lowerToOrig[charStart];
  const origEnd = lowerToOrig[charStart + charLen - 1] + 1;
  return { offset: origStart, length: Math.max(0, origEnd - origStart) };
}

/** Find `quote` across an array of part strings, preferring `partHint` (1-based) first. */
export function locate(parts, quote, partHint = null) {
  if (!Array.isArray(parts) || !parts.length) return null;
  const needle = normalize(quote);
  if (!needle) return null;
  const order = searchOrder(parts.length, partHint);
  for (const idx of order) {
    const hit = findInText(parts[idx], needle);
    if (hit) return { part: idx + 1, ...hit };
  }
  return null;
}
