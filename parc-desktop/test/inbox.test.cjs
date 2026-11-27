const test = require('node:test');
const assert = require('node:assert/strict');
const { sweepMessage, sweepVaults } = require('../inbox-report.cjs');

test('all-vault sweep reports mixed outcomes and never calls a failed or skipped sweep clear', async () => {
  const report = await sweepVaults(['One', 'Two'], async p => {
    if (p === 'Two') throw new Error('Unavailable');
    return { created: [{}], waiting: ['copy'], skipped: ['.tmp'], failed: [{ filename: 'memo', error: 'hook veto' }] };
  }, p => p);
  assert.deepEqual(report.failed.map(f => f.vault), ['One', 'Two']);
  const text = sweepMessage(report);
  for (const part of ['Imported 1', 'still settling', 'Skipped 1', '2 problems']) assert.ok(text.includes(part), text);
  assert.doesNotMatch(sweepMessage({ skipped: ['file'] }), /clear/);
  assert.match(sweepMessage({ failed: [report.failed[1]] }), /Two: Unavailable/);
  assert.match(sweepMessage({}), /clear/);
});

test('unsupported Markdown stays readable and consumes input', async () => {
  const { renderMarkdownBlock } = await import('../document-markdown.mjs');
  let budget = 50;
  const node = tag => ({ tag, children: [], append(...n) {
    assert.ok(--budget > 0, 'renderer failed to advance'); this.children.push(...n);
  } });
  global.document = { createElement: node, createTextNode: text => text };
  try {
    const root = node('root');
    renderMarkdownBlock(root, 'Meeting notes\n--- agenda follows\n\n---\n## Decisions', () => {});
    assert.deepEqual(root.children.map(n => n.tag), ['p', 'hr', 'h2']);
    assert.deepEqual(root.children[0].children, ['Meeting notes --- agenda follows']);
  } finally { delete global.document; }
});

test('a late source title can merge with unsaved notes but competing edits still conflict', async () => {
  const { canAdoptSourceTitle } = await import('../source-revision.mjs');
  const f = { id: 'one', type: 'source', revision: 'before', title: 'sample', body: '' };
  const latest = { ...f, revision: 'after', title: 'Ferry Winter Plan' };
  assert.equal(canAdoptSourceTitle(f, 'before', 'sample', latest), true);
  assert.equal(canAdoptSourceTitle(f, 'before', 'My title', latest), false);
  assert.equal(canAdoptSourceTitle(f, 'before', 'sample', { ...latest, body: 'Other notes' }), false);
  assert.equal(canAdoptSourceTitle(f, 'stale-draft', 'sample', latest), false);
});
