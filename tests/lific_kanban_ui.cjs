// I132: the Lific board opens as a kanban, run from the real panel code in web/app.js.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const source = fs.readFileSync('web/app.js', 'utf8');
const start = source.indexOf('  // I123 to I127: a Lific board');
const end = source.indexOf('  function countdownText(');
assert.ok(start >= 0 && end > start, 'the Lific panel code is where the test looks');
// The real helpers, so a handler that needs its event gets one the way the page gives it.
const helper = (name) => {
  const at = source.indexOf(`function ${name}(`);
  assert.ok(at >= 0, `${name} is in web/app.js`);
  return source.slice(at, source.indexOf('\n}', at) + 2) + '\n';
};

const h = (tag, props = {}, ...children) => ({ tag, ...props, children: children.flat().filter((c) => c !== null && c !== undefined && c !== false), replaceChildren(...kids) { this.children = kids; } });
const text = (el) => typeof el === 'string' ? el : (el.textContent || '') + (el.children || []).map(text).join('');
const find = (el, cls, out = []) => { if (el && typeof el === 'object') { if ((el.class || '').split(' ').includes(cls)) out.push(el); (el.children || []).forEach((c) => find(c, cls, out)); } return out; };

let board, opened, panel, confirmed = true;
const calls = [];
const c = {
  h, field: (label, input) => h('label', {}, label, input), formError: () => ({ hidden: true }),
  FormData: class { constructor(form) { return Object.entries(form.values); } }, confirm: () => confirmed,
  api: async (path, options) => { if (options) { calls.push(JSON.parse(JSON.stringify({ path, ...options }))); return { ok: true }; } return board; },
  panels: { add: (name, title, opts, ...content) => { panel = h('aside', {}, ...content); }, open: (name) => { opened = name; } },
};
vm.createContext(c);
vm.runInContext(helper('submitter') + helper('act') + source.slice(start, end) + ';this.openLific = openLific; this.loadLific = loadLific;', c);
const [info, error, columns, link, credit] = panel.children;
// Events as the browser sends them: the element the handler is on, and what it needs.
const submit = (values) => ({ preventDefault() {}, target: { values, querySelector: () => null } });
const on = (el, more = {}) => ({ currentTarget: el, ...more });
const ok = () => assert.equal(error.hidden, true, `no error shown: ${error.textContent}`);
const col = (status, issues = []) => ({ status, issues });
const settle = () => new Promise((r) => setImmediate(r));

(async () => {
  // An empty project made for the room still looks like a board, and nobody is asked to link.
  board = { available: true, project: 'RH1WY', shown: 0, total: 0, truncated: false, you_can_link: true,
    columns: [col('backlog'), col('todo'), col('active'), col('done'), col('cancelled')] };
  c.openLific({ id: 'item-1' });
  await settle();
  assert.equal(opened, 'lific');
  assert.deepEqual(columns.children.map((s) => s['aria-label']), ['Backlog', 'To do', 'Active', 'Done']);
  assert.equal(find(columns, 'lific-empty').length, 4);
  assert.equal(link.hidden, false, 'the owner can still link a different project');
  assert.equal(link.open, false, 'but the link form starts folded away');
  assert.equal(link.tag, 'details');
  assert.match(text(info), /RH1WY: 0 of 0 issues/);

  // Cards carry id, title and details as text; Cancelled shows once it has issues.
  board = { ...board, shown: 2, total: 2, you_can_link: false, columns: [col('backlog'), col('todo', [{ id: 'RH1WY-1', title: '<b>Paint</b> the door', priority: 'high', module: 'Art' }]), col('active'), col('done'), col('cancelled', [{ id: 'RH1WY-2', title: 'Old idea', priority: 'none', module: null }])] };
  c.openLific({ id: 'item-1' });
  await settle();
  assert.equal(columns.children.length, 5);
  const cards = find(columns, 'lific-card');
  assert.equal(cards.length, 2);
  assert.equal(text(cards[0]), 'RH1WY-1<b>Paint</b> the doorhigh · Art');
  assert.ok(cards[0].children.every((child) => !('innerHTML' in child)), 'titles go in as text');
  assert.equal(text(cards[1]), 'RH1WY-2Old idea');
  assert.equal(find(columns, 'lific-empty').length, 3);
  assert.equal(link.hidden, true, 'someone who can\'t link sees no link form');

  // I140: someone who may change it gets an add box per working column, Move to on every note,
  // and Remove only where they may; each goes to the board's card routes.
  board = { ...board, can_change: true, you_can_link: false, columns: [col('backlog'), col('todo', [
    { id: 'RH1WY-1', title: 'Mine', priority: 'urgent', can_remove: true, added_by: { name: 'Ann', is_bot: false } },
    { id: 'RH1WY-3', title: 'Theirs', priority: 'none', can_remove: false }]), col('active'), col('done'), col('cancelled')] };
  c.openLific({ id: 'item-1' });
  await settle();
  const adders = find(columns, 'lific-add');
  assert.equal(adders.length, 4, 'one add box per working column');
  await adders[1].onsubmit(submit({ title: 'New note' }));
  ok();
  assert.deepEqual(calls.at(-1), { path: '/api/items/item-1/lific/cards', method: 'POST', body: { title: 'New note', status: 'todo' } });
  const notes = find(columns, 'lific-card');
  assert.equal(notes[0]['data-priority'], 'urgent', 'a note is coloured by its priority');
  assert.equal(notes[0].draggable, true);
  assert.match(text(notes[0]), /added by Ann/);
  const [mineActions, theirActions] = find(columns, 'lific-card-actions');
  const move = mineActions.children[0];
  assert.ok(!move.children.some((o) => o.value === 'todo'), 'Move to leaves out where it is');
  await move.onchange(on(move, { target: { value: 'done' } }));
  ok();
  assert.deepEqual(calls.at(-1), { path: '/api/items/item-1/lific/cards/RH1WY-1', method: 'PUT', body: { status: 'done' } });
  assert.equal(theirActions.children.length, 1, 'no Remove on a note they may not take down');
  confirmed = false;
  const before = calls.length;
  await mineActions.children[1].onclick(on(mineActions.children[1]));
  assert.equal(calls.length, before, 'Remove asks first');
  confirmed = true;
  await mineActions.children[1].onclick(on(mineActions.children[1]));
  ok();
  assert.deepEqual(calls.at(-1), { path: '/api/items/item-1/lific/cards/RH1WY-1', method: 'DELETE' });
  // A drop on another column moves the note there.
  const done = columns.children.find((s) => s['aria-label'] === 'Done');
  let dropped = false;
  await done.ondrop(on(done, { preventDefault() { dropped = true; }, dataTransfer: { getData: () => 'RH1WY-3' } }));
  ok();
  assert.ok(dropped, 'the drop is taken, so the browser doesn\'t open the card id');
  assert.deepEqual(calls.at(-1).body, { status: 'done' });

  // I147: the panel credits Lific, linked to its repository, and Star shows the count once known.
  const links = (el, out = []) => { if (el && typeof el === 'object') { if (el.tag === 'a') out.push(el); (el.children || []).forEach((k) => links(k, out)); } return out; };
  assert.ok(!credit.hidden, 'the credit always shows');
  assert.match(text(credit), /Lific.*VoidNullable/);
  assert.ok(links(credit).length >= 2 && links(credit).every((a) => a.href === 'https://github.com/VoidNullable/lific' && a.target === '_blank' && /noopener/.test(a.rel)));
  assert.doesNotMatch(text(credit), /\d/, 'no count until the server has one');
  board = { ...board, credit: { url: 'https://github.com/VoidNullable/lific', stars: 1234 } };
  c.openLific({ id: 'item-1' });
  await settle();
  assert.match(text(credit), /Star.*1,234/);

  // Unavailable says why, with no columns.
  board = { available: false, unavailable: 'Lific is busy', you_can_link: false, columns: [] };
  c.openLific({ id: 'item-1' });
  await settle();
  assert.equal(columns.children.length, 0);
  assert.equal(text(info), 'Lific is busy');
  console.log('Lific kanban UI: four columns even when empty, cards as text, Cancelled when used, link form folded, sticky notes added, moved, dropped and removed with real events, Lific credited with its stars passed');
})().catch((e) => { console.error(e); process.exitCode = 1; });
