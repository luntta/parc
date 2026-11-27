// The smallest change from `prev` to `next`: what lies between their common start and common end, as a
// CodeMirror change ({ from, to, insert }), or null when they are the same.
export function textChange(prev, next) {
  if (prev === next) return null;
  let from = 0;
  const max = Math.min(prev.length, next.length);
  while (from < max && prev.charCodeAt(from) === next.charCodeAt(from)) from++;
  let end = 0;
  while (end < max - from && prev.charCodeAt(prev.length - 1 - end) === next.charCodeAt(next.length - 1 - end)) end++;
  return { from, to: prev.length - end, insert: next.slice(from, next.length - end) };
}
