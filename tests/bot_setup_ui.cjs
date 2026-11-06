// node tests/bot_setup_ui.cjs: #647 B, the page where a person approves the bot their agent drafted.
// It shows the draft for editing, sends the edited fields to approve, and Cancel declines. A lapsed
// or approved draft says so, and Needs you links to the page.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const page = fs.readFileSync('web/bot-setup.js', 'utf8').replace(/\r\n/g, '\n');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');

class Node {
  constructor(tag, props = {}, children = []) {
    this.tag = tag; this.props = { ...props }; this.children = [];
    Object.assign(this, props); this.append(...children);
  }
  append(...c) { this.children.push(...c.flat(Infinity).filter((x) => x != null && x !== false)); }
  replaceChildren(...c) { this.children = []; this.append(...c); }
}
const all = (n) => (n instanceof Node ? [n, ...n.children.flatMap(all)] : []);
const text = (n) => (n instanceof Node ? n.children.map(text).join('') : String(n));
const h = (tag, props, ...children) => new Node(tag, props, children);
const field = (label, input, hint) => h('label', {}, h('span', {}, label), input, hint && h('small', {}, hint));
const formError = () => h('p', { class: 'error', hidden: true });
const link = (path, label, props = {}) => h('a', { href: path, ...props }, label);

function load(drafts) {
  const calls = [];
  const api = async (path, options = {}) => {
    calls.push({ path, ...options });
    const id = path.split('/')[3];
    const draft = drafts[id];
    if (!draft) { const e = new Error('not found'); e.status = 404; throw e; }
    if (path.endsWith('/approve')) return { ...draft, ...options.body, status: 'approved', bot: { id: 'b1' } };
    if (path.endsWith('/decline')) return { ok: true };
    return draft;
  };
  const context = { document: { title: '' } };
  vm.createContext(context);
  vm.runInContext(page.replace('export async function', 'async function') + '\nthis.botSetupView = botSetupView;', context);
  return { view: (id) => context.botSetupView(id, { h, api, field, formError, link }), calls };
}

(async () => {
  const open = { id: 'd1', name: 'Scout', role: 'Keep the queue moving.', job: 'Review PRs', workplace: 'Fairground', day_plan: '9 to 5', status: 'open' };
  const { view, calls } = load({ d1: open, d2: { ...open, id: 'd2', status: 'approved' }, d3: { ...open, id: 'd3' } });

  // The draft is shown in fields the person can change, and approving sends what they left there.
  const root = await view('d1');
  const inputs = all(root).filter((n) => n.tag === 'input' || n.tag === 'textarea');
  assert.deepEqual([...inputs.map((n) => n.name)], ['name', 'role', 'job', 'workplace', 'day_plan']);
  assert.equal(inputs[1].value, 'Keep the queue moving.');
  assert.match(text(root), /can't approve it for you/);
  inputs[0].value = 'Scout Two';
  inputs[2].value = '';
  const form = all(root).find((n) => n.tag === 'form');
  form.onsubmit({ preventDefault() {} });
  await new Promise((r) => setImmediate(r));
  const sent = calls.find((c) => c.path.endsWith('/approve'));
  assert.equal(sent.method, 'POST');
  assert.deepEqual(JSON.parse(JSON.stringify(sent.body)), { name: 'Scout Two', role: 'Keep the queue moving.', job: '', workplace: 'Fairground', day_plan: '9 to 5' });
  assert.match(text(root), /You approved this/);
  assert.match(text(root), /My bots/);

  // Cancel declines.
  const other = await view('d3');
  const cancel = all(other).find((n) => n.tag === 'button' && text(n) === 'Cancel');
  cancel.onclick();
  await new Promise((r) => setImmediate(r));
  assert.ok(calls.some((c) => c.path === '/api/bot-setups/d3/decline' && c.method === 'POST'));
  assert.match(text(other), /declined/);

  // A draft that's done, or gone, says so and has no form.
  const done = await view('d2');
  assert.equal(all(done).some((n) => n.tag === 'form'), false);
  assert.match(text(done), /You approved this/);
  const gone = await view('nope');
  assert.match(text(gone), /isn't here/);

  // Needs you links to the page.
  const start = app.indexOf('  function botSetupItem(setup) {');
  const end = app.indexOf('\n  }\n', start) + 4;
  const ctx = {
    h, link, item: (...parts) => h('article', {}, ...parts),
    timeFormat: { format: () => '5:00 PM' }, fromUnix: (u) => new Date(u * 1000),
  };
  vm.createContext(ctx);
  vm.runInContext(app.slice(start, end) + '\nthis.botSetupItem = botSetupItem;', ctx);
  const shown = ctx.botSetupItem({ id: 'd 1', name: 'Scout', expires_at: 1 });
  assert.match(text(shown), /drafted a bot for you: Scout/);
  assert.equal(all(shown).find((n) => n.tag === 'a').href, '/setup/d%201');
  console.log('bot setup ui ok');
})().catch((error) => { console.error(error); process.exit(1); });
