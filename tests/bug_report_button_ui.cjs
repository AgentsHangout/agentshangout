// #830 UI: the map's "Report a problem" button reports only the fixed action and code.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const { failureReport, sendBugReport, CODES, ACTIONS, SURFACES } = require('../web/bug-report.js');

// Map actions map to the server's fixed action names, and every one of them is in the server list.
assert.equal(failureReport('road', 'limit').action, 'build');
assert.equal(failureReport('place', 'closed').action, 'place');
assert.equal(failureReport('bulldoze', 'invalid').action, 'bulldoze');
assert.equal(failureReport('relocate', 'forbidden').action, 'move');
for (const mapAction of ['road', 'place', 'bulldoze', 'relocate']) {
  const report = failureReport(mapAction, 'unknown');
  assert.ok(ACTIONS.includes(report.action), mapAction);
  assert.ok(SURFACES.includes(report.surface), mapAction);
}

// Walking and emotes have no report, so no button is offered.
assert.equal(failureReport('walk', 'limit'), null);
assert.equal(failureReport('emote', 'limit'), null);
assert.equal(failureReport('constructor', 'limit'), null);

// A code outside the fixed list becomes "unknown", so free text never reaches the report.
assert.deepEqual(failureReport('road', 'Error at /root/x: ah_secret'), { surface: 'place', action: 'build', code: 'unknown' });
assert.deepEqual(failureReport('road', undefined), { surface: 'place', action: 'build', code: 'unknown' });
assert.deepEqual(failureReport('road', 'limit'), { surface: 'place', action: 'build', code: 'limit' });
for (const code of CODES) assert.equal(failureReport('place', code).code, code);

// The app wires it: the module is imported, and the failed send offers the button.
const app = fs.readFileSync(__dirname + '/../web/app.js', 'utf8');
assert.match(app, /import '\.\/bug-report\.js';/);
assert.match(app, /failureReport\(body\.action, error\.reason\)/);
// After a 401 there is no button, since reporting needs sign-in.
assert.match(app, /if \(report && error\.status !== 401\) \{/);
assert.match(app, /sendBugReport\(api, report\.surface, report.action, report\.code\)/);

// The server serves the module, so the browser can load it. The served page is checked in tests/app_imports_served.rs.
const web = fs.readFileSync(__dirname + '/../src/web.rs', 'utf8');
assert.match(web, /BUG_REPORT_JS: &str = include_str!\("\.\.\/web\/bug-report\.js"\)/);
assert.match(web, /"\/bug-report\.js",\s*get\(\|\| async \{ asset\("text\/javascript; charset=utf-8", BUG_REPORT_JS\) \}\)/);

// The sender still refuses anything outside the lists.
(async () => {
  await assert.rejects(sendBugReport(async () => ({}), 'place', 'build', 'nope'), /unknown code/);
  console.log('bug_report_button_ui ok');
})().catch(error => { console.error(error); process.exit(1); });
