// #830 UI: the bug report control only sends fixed codes, and the note is capped.
const assert = require('node:assert/strict');
const { buildBugReport, sendBugReport, SURFACES, ACTIONS, CODES, NOTE_MAX } = require('../web/bug-report.js');

// A plain report carries surface, action and code only.
assert.deepEqual(buildBugReport('house', 'build', 'failed'), { surface: 'house', action: 'build', error: 'failed' });

// A note is trimmed and capped; an empty note is left out.
const long = ' ' + 'x'.repeat(400) + ' ';
assert.equal(buildBugReport('item', 'use', 'other', long).note.length, NOTE_MAX);
assert.equal(buildBugReport('item', 'use', 'other', '   ').note, undefined);

// Anything outside the fixed lists is refused, so no free text can become the error.
assert.throws(() => buildBugReport('house', 'build', 'Error at /root/x: ah_secret'), /unknown code/);
assert.throws(() => buildBugReport('room', 'build', 'failed'), /unknown surface/);
assert.throws(() => buildBugReport('house', 'drop table', 'failed'), /unknown action/);

// The sender posts the built body to the one endpoint.
(async () => {
  const calls = [];
  const api = async (path, options) => { calls.push([path, options]); return { ok: true }; };
  await sendBugReport(api, 'space', 'move', 'timeout', 'stuck on the stairs');
  assert.equal(calls.length, 1);
  assert.equal(calls[0][0], '/api/bugs/report');
  assert.equal(calls[0][1].method, 'POST');
  assert.deepEqual(calls[0][1].body, { surface: 'space', action: 'move', error: 'timeout', note: 'stuck on the stairs' });

  await assert.rejects(sendBugReport(api, 'space', 'move', 'nope'), /unknown code/);
  assert.equal(calls.length, 1, 'a refused report sends nothing');

  assert.ok(SURFACES.length && ACTIONS.length && CODES.length);
  console.log('bug_report_ui ok');
})().catch(error => { console.error(error); process.exit(1); });
