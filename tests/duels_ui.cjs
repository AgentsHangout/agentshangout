// node tests/duels_ui.cjs: GM10, the owner's Duels dialog for one bot. It shows the bot's allow and
// cap as the server has them, and saving sends both, the cap as a number, for that bot. Runs the
// shipped duelsDialog against stubs, not just its text.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
const start = app.indexOf('  async function duelsDialog(');
assert.ok(start >= 0, 'duelsDialog is in web/app.js');
const code = app.slice(start, app.indexOf('\n  }\n', start) + 4);

class Node {
  constructor(tag, props = {}, children = []) {
    this.tag = tag; this.props = props;
    this.children = children.flat(Infinity).filter((c) => c != null && c !== false);
  }
}
const h = (tag, props, ...children) => new Node(tag, props || {}, children);
const all = (n) => (n instanceof Node ? [n, ...n.children.flatMap(all)] : []);
const formOf = (dialog) => dialog.content.find((c) => c instanceof Node && c.tag === 'form');
const chosen = (form) => all(form).filter((n) => n.tag === 'option').map((o) => [o.props.value, Boolean(o.props.selected)]);

const context = {
  h,
  formError: () => h('p', {}),
  field: (label, input) => h('label', {}, label, input),
  submitter: (_error, action) => action,
  dialogs: [],
  calls: [],
  saved: { allow: 'off', daily_cap: 5 },
  fail: false,
  modal(title, ...content) {
    const dialog = { title, content, closed: 0, close() { this.closed += 1; } };
    this.dialogs.push(dialog);
    return dialog;
  },
  async api(path, options = {}) {
    this.calls.push({ path, method: options.method || 'GET', body: options.body });
    if (options.method && this.fail) throw new Error('refused');
    return this.saved;
  },
};
context.modal = context.modal.bind(context);
context.api = context.api.bind(context);
vm.createContext(context);
vm.runInContext(`${code}\nthis.duelsDialog = duelsDialog;`, context);

(async () => {
  // Off, as every bot starts: Nobody is chosen, and the cap is the one the server has.
  await context.duelsDialog({ id: 'bot one', name: 'Scout' });
  const first = context.dialogs[0];
  assert.equal(first.title, 'Duels for Scout');
  assert.deepEqual(chosen(formOf(first)), [['off', true], ['room', false]]);
  assert.equal(all(formOf(first)).find((n) => n.props.name === 'daily_cap').props.value, '5');
  assert.equal(context.calls[0].path, '/api/agents/bot%20one/duels');

  // Turning it on sends the allow and the cap for that bot, and closes.
  await formOf(first).props.onsubmit({ allow: 'room', daily_cap: '3' });
  assert.equal(JSON.stringify(context.calls.at(-1)), JSON.stringify({ path: '/api/agents/bot%20one/duels', method: 'PUT', body: { allow: 'room', daily_cap: 3 } }));
  assert.equal(first.closed, 1);

  // On already: the same room is chosen. A save the server refuses leaves it open.
  context.saved = { allow: 'room', daily_cap: 2 };
  await context.duelsDialog({ id: 'b2', name: 'Rex' });
  const second = context.dialogs[1];
  assert.deepEqual(chosen(formOf(second)), [['off', false], ['room', true]]);
  context.fail = true;
  await assert.rejects(formOf(second).props.onsubmit({ allow: 'off', daily_cap: '2' }));
  assert.equal(second.closed, 0);
  console.log('Duels dialog: shows the saved allow and cap, saves both for the right bot, stays open on a refusal (run, not just read).');
})().catch((error) => { console.error(error); process.exitCode = 1; });
