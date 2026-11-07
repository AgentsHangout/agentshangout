// node tests/coin_drop_ui.cjs: HC53. A drawn coin's line in Your credits names the bot that found
// it. Runs the shipped history row from web/app.js, not just its text.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
const slice = (from, end) => {
  const start = app.indexOf(from);
  assert.ok(start >= 0, from);
  const stop = app.indexOf(end, start);
  assert.ok(stop >= 0, end);
  return app.slice(start, stop + end.length);
};
const code = slice('const PAID_FOR = ', 'utcDateFormat.format(fromUnix(line.at))}`));\n');
const context = {
  h: (tag, attrs, ...children) => ({ tag, attrs, children }),
  packName: (pack) => pack,
  fromUnix: (at) => new Date(at * 1000),
  dateFormat: { format: () => 'local' },
  utcDateFormat: { format: () => 'utc' },
};
vm.createContext(context);
vm.runInContext(`${code}\nthis.historyRow = historyRow;`, context);
const text = (line) => context.historyRow(line).children.map((part) => part.children.join('')).join('');

assert.equal(text({ kind: 'coin', amount: 1, at: 600, bot: 'Zed' }), '+1 · A coin found by Zed · local');
// A coin whose bot was deleted still shows, without a name.
assert.equal(text({ kind: 'coin', amount: 1, at: 600, bot: null }), '+1 · A coin found · local');
// The other lines are as they were.
assert.equal(text({ kind: 'gathering', amount: 20, at: 0, city: 'Oak' }), '+20 · A gathering in Oak · utc');
assert.equal(text({ kind: 'spend', amount: -15, at: 0, pack: 'winter', bot: 'Zed' }), '-15 · winter, bought by Zed · local');
console.log('A drawn coin says which bot found it (run, not just read).');
