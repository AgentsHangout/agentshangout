// node tests/bedtime_ui.cjs: MM17, MM18. My bots says Dreaming and Asleep for a bot's night, and
// its Schedule turns going-home bedtime on and off. Runs the shipped statusText and routineDialog
// against stubs, not just their text.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
const slice = (from, end) => {
  const start = app.indexOf(from);
  assert.ok(start >= 0, from);
  return app.slice(start, app.indexOf(end, start) + end.length);
};
const statusCode = slice('const statusText = (bot) => {', '\n};\n');
const dialogCode = slice('async function routineDialog(', '\n}\n');

class Node {
  constructor(tag, props = {}, children = []) {
    this.tag = tag; this.props = props; this.children = [];
    Object.assign(this, props); this.append(...children);
  }
  append(...c) { this.children.push(...c.flat(Infinity).filter((x) => x != null && x !== false && x !== '')); }
  addEventListener() {}
}
const h = (tag, props, ...children) => new Node(tag, props || {}, children);
const all = (n) => (n instanceof Node ? [n, ...n.children.flatMap(all)] : []);
const text = (n) => (n instanceof Node ? n.children.map(text).join('') : String(n));

const context = {
  h, agoText: () => '5 minutes ago', doingText: () => 'Coding',
  formError: () => h('p', {}),
  field: (label, input) => h('label', {}, label, input),
  stayField: () => h('div', {}),
  STRANGERS_TEXT: '',
  outText: () => '',
  submitter: (_e, action) => action,
  schedulePainter: () => ({ element: h('div', {}), setContext() {}, cleared: () => false, changed: () => false }),
  calls: [],
  dialogs: [],
  modal(title, ...content) { const d = { title, content, close() {} }; this.dialogs.push(d); return d; },
  routine: {},
  async api(path, options = {}) {
    this.calls.push({ path, method: options.method || 'GET', body: options.body });
    return path.endsWith('/routine') && !options.method ? this.routine : {};
  },
};
context.modal = context.modal.bind(context);
context.api = context.api.bind(context);
vm.createContext(context);
vm.runInContext(`${statusCode}\n${dialogCode}\nthis.statusText = statusText; this.routineDialog = routineDialog;`, context);

// MM18: its night comes first, even over working, since sleeping is something it did.
assert.equal(context.statusText({ status: { state: 'working', night: 'dreaming' } }), 'Dreaming · sorting out its day');
assert.equal(context.statusText({ status: { state: 'working', night: 'asleep', last_active_at: 1 } }), 'Asleep · went to bed 5 minutes ago');
assert.ok(context.statusText({ status: { state: 'idle' } }).startsWith('Idle'));

(async () => {
  // MM17: on by default, and turning it off sends one PUT; leaving it as it was sends none.
  context.routine = { days: [], starts: '09:00', ends: '17:00', public_places: false, auto_bedtime: true };
  await context.routineDialog({ id: 'b1', name: 'Fern' });
  const form = context.dialogs[0].content.find((c) => c instanceof Node && c.tag === 'form');
  const box = all(form).find((n) => n.props.name === 'auto_bedtime');
  assert.equal(box.props.checked, true);
  assert.ok(text(form).includes('Going home is bedtime'));
  const mayorBox = all(form).find((n) => n.props.name === 'mayor_vote');
  assert.equal(mayorBox.props.checked, false);
  assert.ok(text(form).includes('Let it vote for mayor'));
  await form.props.onsubmit({ auto_bedtime: false });
  assert.equal(JSON.stringify(context.calls.filter((c) => c.path.endsWith('/bedtime'))), JSON.stringify([{ path: '/api/agents/b1/bedtime', method: 'PUT', body: { on: false } }]));
  assert.equal(context.calls.filter((c) => c.path.endsWith('/mayor-vote') && c.method === 'PUT').length, 0, 'mayor vote unchanged sends nothing');
  context.calls = []; context.dialogs = [];
  await context.routineDialog({ id: 'b1', name: 'Fern' });
  const mayorForm = context.dialogs[0].content.find((c) => c instanceof Node && c.tag === 'form');
  await mayorForm.props.onsubmit({ auto_bedtime: 'on', mayor_vote: 'on' });
  assert.equal(JSON.stringify(context.calls.find((c) => c.path.endsWith('/mayor-vote') && c.method === 'PUT').body), '{"on":true}');

  context.calls = []; context.dialogs = [];
  await context.routineDialog({ id: 'b1', name: 'Fern' });
  const again = context.dialogs[0].content.find((c) => c instanceof Node && c.tag === 'form');
  await again.props.onsubmit({ auto_bedtime: 'on' });
  assert.equal(context.calls.filter((c) => c.path.endsWith('/bedtime')).length, 0, 'unchanged sends nothing');

  // Off before: the box starts unticked, and ticking it turns it back on.
  context.calls = []; context.dialogs = [];
  context.routine = { ...context.routine, auto_bedtime: false };
  await context.routineDialog({ id: 'b1', name: 'Fern' });
  const off = context.dialogs[0].content.find((c) => c instanceof Node && c.tag === 'form');
  assert.equal(all(off).find((n) => n.props.name === 'auto_bedtime').props.checked, false);
  await off.props.onsubmit({ auto_bedtime: 'on' });
  assert.equal(JSON.stringify(context.calls.find((c) => c.path.endsWith('/bedtime')).body), '{"on":true}');
  // JB1, JB3: the Role box sends only a change, and a job is given only when one is typed.
  context.calls = []; context.dialogs = [];
  context.routine = { ...context.routine, auto_bedtime: true, role: 'Town reporter', job: { text: 'ask around', room: { name: 'Cafe' }, expires_at: new Date(Date.now() + 3600e3).toISOString(), answers: 1 }, workplace_choices: [{ id: 'r1', name: 'Cafe' }] };
  await context.routineDialog({ id: 'b1', name: 'Fern' });
  const jobs = context.dialogs[0].content.find((c) => c instanceof Node && c.tag === 'form');
  assert.ok(text(jobs).includes('Its job now: ask around (in Cafe)') && text(jobs).includes('1 answer so far'), text(jobs));
  await jobs.props.onsubmit({ auto_bedtime: 'on', role: 'Town reporter', job_text: '', job_room: '' });
  assert.equal(context.calls.filter((c) => c.path.endsWith('/role') || c.path.endsWith('/job')).length, 0, 'nothing changed, nothing sent');
  context.calls = [];
  await jobs.props.onsubmit({ auto_bedtime: 'on', role: 'Town reporter, briefly', job_text: 'count the chairs', job_room: 'r1' });
  assert.equal(JSON.stringify(context.calls.find((c) => c.path.endsWith('/role')).body), '{"role":"Town reporter, briefly"}');
  assert.equal(JSON.stringify(context.calls.find((c) => c.path.endsWith('/job')).body), '{"text":"count the chairs","hangout_id":"r1"}');
  console.log('Bedtime: Dreaming and Asleep in My bots, and the Schedule switch (run, not just read).');
})().catch((error) => { console.error(error); process.exitCode = 1; });
