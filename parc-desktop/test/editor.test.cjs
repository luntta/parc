const test = require('node:test');
const assert = require('node:assert/strict');

test('a saved text catches up with the smallest change, leaving the rest (and the caret in it) alone', async () => {
  const { textChange } = await import('../text-change.mjs');
  const apply = (prev, c) => c ? prev.slice(0, c.from) + c.insert + prev.slice(c.to) : prev;
  assert.equal(textChange('same', 'same'), null);
  for (const [prev, next] of [['abc', 'abXc'], ['hello world', 'hello'], ['', 'new'], ['old', ''], ['aaa', 'aaaa'], ['line\n', 'line'], ['x y z', 'x Y z']]) {
    const c = textChange(prev, next);
    assert.equal(apply(prev, c), next, `${prev} → ${next}`);
  }
  assert.deepEqual(textChange('Title\n\nBody text  ', 'Title\n\nBody text'), { from: 16, to: 18, insert: '' });
});
