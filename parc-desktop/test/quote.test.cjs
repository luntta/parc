const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const quoteMod = import('../quote.mjs');

test('JS quote normalizer matches shared Rust fixture file identically', async () => {
  const { normalize } = await quoteMod;
  const fixturePath = path.resolve(__dirname, '../../parc-core/tests/fixtures/quote_normalize.json');
  const cases = JSON.parse(fs.readFileSync(fixturePath, 'utf8'));
  assert.ok(cases.length >= 7);
  for (const c of cases) {
    assert.equal(normalize(c.input), c.expected, `input: ${JSON.stringify(c.input)}`);
  }
});

test('parseQuotes and formatQuote handle page, slide, section, multi-file, and comments', async () => {
  const { parseQuotes, formatQuote } = await quoteMod;
  const body = [
    'Intro note.',
    '',
    '> p. 12 · "The ferry timetable changed in autumn."',
    'Check winter departures.',
    '',
    '> slide 3 · "Revenue grew 14%"',
    '',
    '> § Key Findings · "Latency dropped below 20 ms."',
    'Fast enough for offline sync.',
    '',
    '> report.pdf p. 4 · "Appendix table"',
    '',
    '```',
    '> p. 99 · "Ignored in fence"',
    '```'
  ].join('\n');

  const parsed = parseQuotes(body);
  assert.equal(parsed.length, 4);
  assert.deepEqual(parsed[0].anchor, { kind: 'page', number: 12 });
  assert.equal(parsed[0].filename, null);
  assert.equal(parsed[0].text, 'The ferry timetable changed in autumn.');
  assert.equal(parsed[0].comment, 'Check winter departures.');

  assert.deepEqual(parsed[1].anchor, { kind: 'slide', number: 3 });
  assert.equal(parsed[1].comment, '');

  assert.deepEqual(parsed[2].anchor, { kind: 'section', heading: 'Key Findings' });
  assert.equal(parsed[2].comment, 'Fast enough for offline sync.');

  assert.equal(parsed[3].filename, 'report.pdf');
  assert.deepEqual(parsed[3].anchor, { kind: 'page', number: 4 });

  assert.equal(
    formatQuote({ anchor: { kind: 'page', number: 12 }, text: 'Hello   "world"\nnow' }),
    '> p. 12 · "Hello \'world\' now"'
  );
  assert.equal(
    formatQuote({ filename: 'report.pdf', anchor: { kind: 'slide', number: 2 }, text: 'Deck note' }),
    '> report.pdf slide 2 · "Deck note"'
  );
});

test('locate finds quotes across ligatures, line-end hyphens, and shifted pages', async () => {
  const { locate } = await quoteMod;
  const parts = [
    'Page one intro.',
    'The “ﬁnancial” re-\nport — updated.',
    'The “financial” report - updated.'
  ];
  assert.equal(locate(parts, 'The "financial" report - updated.', 2).part, 2);
  assert.equal(locate(parts, 'The "financial" report - updated.', 1).part, 2);
  assert.equal(locate(parts, 'The "financial" report - updated.', 3).part, 3);
});
