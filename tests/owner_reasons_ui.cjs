// node tests/owner_reasons_ui.cjs: #999 (web line). The reasons under the since line show up to three
// lines from /api/owner/reasons, and nothing when there are none. Runs the shipped loadReasons.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
const start = app.indexOf('  async function loadReasons() {');
assert.ok(start >= 0, 'loadReasons');
const source = app.slice(start, app.indexOf('\n  }\n', start) + '\n  }\n'.length);

function run(response) {
  const made = [];
  const context = {
    reasonsBox: { hidden: true, children: [], replaceChildren(...kids) { this.children = kids; } },
    h: (tag, attrs, text) => ({ tag, attrs, text }),
    api: async (path) => {
      assert.equal(path, '/api/owner/reasons');
      if (response instanceof Error) throw response;
      return response;
    },
  };
  vm.createContext(context);
  vm.runInContext(`${source}\nthis.loadReasons = loadReasons;`, context);
  return context.loadReasons().then(() => context.reasonsBox);
}

(async () => {
  const none = await run({ since: 1, until: 2, reasons: [] });
  assert.equal(none.hidden, true, 'zero reasons shows nothing');
  assert.equal(none.children.length, 0);

  const four = await run({
    reasons: [
      { kind: 'plan', line: 'Plan: a new answer on Friday', bots: [] },
      { kind: 'vote', line: 'Vote closes tonight', bots: [] },
      { kind: 'lab', line: 'Lab PR merged', bots: [] },
      { kind: 'extra', line: 'Fourth line', bots: [] },
    ],
  });
  assert.equal(four.hidden, false);
  assert.deepEqual(four.children.map((c) => c.text), ['Plan: a new answer on Friday', 'Vote closes tonight', 'Lab PR merged']);

  const failed = await run(new Error('down'));
  assert.equal(failed.hidden, true, 'a failed read shows nothing');
  console.log('Owner reasons UI: up to three lines under the since line, none when zero or on error (run, not just read).');
})();
