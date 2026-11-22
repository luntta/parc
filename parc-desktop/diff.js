// Line diff for comparing fragment versions: a longest common subsequence over the lines between the
// shared head and tail. Notes are short; past ~4M cell pairs it shows a whole replacement instead.
export function diffLines(from, to) {
  const a = from.split('\n'), b = to.split('\n');
  let head = 0;
  while (head < a.length && head < b.length && a[head] === b[head]) head++;
  let endA = a.length, endB = b.length;
  while (endA > head && endB > head && a[endA - 1] === b[endB - 1]) { endA--; endB--; }
  const x = a.slice(head, endA), y = b.slice(head, endB), out = a.slice(0, head).map(text => ({ op: ' ', text }));
  if (x.length * y.length > 4e6) out.push(...x.map(text => ({ op: '-', text })), ...y.map(text => ({ op: '+', text })));
  else {
    const n = x.length, m = y.length, lcs = Array.from({ length: n + 1 }, () => new Uint32Array(m + 1));
    for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) lcs[i][j] = x[i] === y[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
    let i = 0, j = 0;
    while (i < n && j < m) {
      if (x[i] === y[j]) { out.push({ op: ' ', text: x[i] }); i++; j++; }
      else if (lcs[i + 1][j] >= lcs[i][j + 1]) out.push({ op: '-', text: x[i++] });
      else out.push({ op: '+', text: y[j++] });
    }
    while (i < n) out.push({ op: '-', text: x[i++] });
    while (j < m) out.push({ op: '+', text: y[j++] });
  }
  out.push(...a.slice(endA).map(text => ({ op: ' ', text })));
  return out;
}

// Keeps `context` unchanged lines around each change; longer unchanged runs become { op: '…', count }.
export function fold(lines, context = 2) {
  const keep = lines.map(() => false);
  lines.forEach((l, i) => { if (l.op !== ' ') for (let k = Math.max(0, i - context); k <= Math.min(lines.length - 1, i + context); k++) keep[k] = true; });
  const out = [];
  for (let i = 0; i < lines.length;) {
    if (keep[i]) { out.push(lines[i++]); continue; }
    let j = i; while (j < lines.length && !keep[j]) j++;
    out.push({ op: '…', count: j - i }); i = j;
  }
  return out;
}

export const stat = lines => ({ added: lines.filter(l => l.op === '+').length, removed: lines.filter(l => l.op === '-').length });
