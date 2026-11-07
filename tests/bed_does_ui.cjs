// node tests/bed_does_ui.cjs: #831. A bot dreaming in a bed (#800) is "using" an item with no
// `does`, and its status line must not throw. Runs the shipped doingOf, not just its text.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
const slice = (from, end) => {
  const start = app.indexOf(from);
  assert.ok(start >= 0, from);
  return app.slice(start, app.indexOf(end, start) + end.length);
};
const context = {
  snapshot: { items: [{ id: 'bed1', kind: 'bed', used_by: ['b1'], does: null }, { id: 'p1', kind: 'piano', used_by: ['b2'], does: 'playing the piano' }] },
  liveThinking: () => null,
  doingText: (activity) => activity,
};
vm.createContext(context);
vm.runInContext(`${slice('const sentence = ', ';\n')}
${slice('const STATE_TEXT = ', ';\n')}
const usedBy = (agent) => snapshot.items.find((item) => item.used_by?.includes(agent.id));
${slice('  function doingOf(agent) {', '\n  }\n')}
this.doingOf = doingOf;`, context);

assert.equal(context.doingOf({ id: 'b1', status: 'dreaming', state: 'offline' }), 'dreaming', 'a bot in a bed shows its status');
assert.equal(context.doingOf({ id: 'b1', state: 'offline' }), 'Napping');
assert.equal(context.doingOf({ id: 'b2', state: 'active' }), 'Playing the piano', 'an item that does something still says so');
console.log('Bed does: a dreaming bot in a bed has a status line (run, not just read).');
