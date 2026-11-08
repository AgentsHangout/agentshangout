// #830 UI: the bug report control only sends fixed codes, and no free-text note.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const { buildBugReport, sendBugReport, codeFor, SURFACES, ACTIONS, CODES } = require('../web/bug-report.js');

// The lists must match the server's lists in src/bug_reports.rs.
const rs = fs.readFileSync(__dirname + '/../src/bug_reports.rs', 'utf8');
const serverList = name => {
  const m = rs.match(new RegExp('pub const ' + name + ':[^=]*=\\s*&\\[([^\\]]*)\\]'));
  assert.ok(m, name + ' not found on server');
  return [...m[1].matchAll(/"([^"]+)"/g)].map(x => x[1]);
};
assert.deepEqual(SURFACES, serverList('SURFACES'));
assert.deepEqual(ACTIONS, serverList('ACTIONS'));
assert.deepEqual(CODES, serverList('CODES'));

// A report carries surface, action and code only. The server reads the code as `code`.
assert.deepEqual(buildBugReport('house', 'build', 'unknown'), { surface: 'house', action: 'build', code: 'unknown' });

// Anything outside the fixed lists is refused, so no free text can become the code.
assert.throws(() => buildBugReport('house', 'build', 'Error at /root/x: ah_secret'), /unknown code/);
assert.throws(() => buildBugReport('room', 'build', 'unknown'), /unknown surface/);
assert.throws(() => buildBugReport('house', 'drop table', 'unknown'), /unknown action/);

// The code from a failed request: the server's reason if listed, else the status, else unknown.
assert.equal(codeFor({ reason: 'limit', status: 429 }), 'limit');
assert.equal(codeFor({ reason: 'Error at /root/x: secret', status: 409 }), 'conflict');
assert.equal(codeFor({ status: 404 }), 'not_found');
assert.equal(codeFor({ status: 500, message: 'ah_secret' }), 'unknown');
assert.equal(codeFor(undefined), 'unknown');

// The sender posts the built body to the one endpoint.
(async () => {
  const calls = [];
  const api = async (path, options) => { calls.push([path, options]); return { ok: true }; };
  await sendBugReport(api, 'space', 'move', 'limit');
  assert.equal(calls.length, 1);
  assert.equal(calls[0][0], '/api/bugs/report');
  assert.equal(calls[0][1].method, 'POST');
  assert.deepEqual(calls[0][1].body, { surface: 'space', action: 'move', code: 'limit' });

  await assert.rejects(sendBugReport(api, 'space', 'move', 'nope'), /unknown code/);
  assert.equal(calls.length, 1, 'a refused report sends nothing');
  console.log('bug_report_ui ok');
})().catch(error => { console.error(error); process.exit(1); });
