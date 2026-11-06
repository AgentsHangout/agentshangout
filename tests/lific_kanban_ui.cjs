// I131: the Lific board opens as a kanban, run from the real panel code in web/app.js.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const source = fs.readFileSync('web/app.js', 'utf8');
const start = source.indexOf('  // I123 to I127: a Lific board');
const end = source.indexOf('  function countdownText(');
assert.ok(start >= 0 && end > start, 'the Lific panel code is where the test looks');

const h = (tag, props = {}, ...children) => ({ tag, ...props, children: children.flat().filter((c) => c !== null && c !== undefined && c !== false), replaceChildren(...kids) { this.children = kids; } });
const text = (el) => typeof el === 'string' ? el : (el.textContent || '') + (el.children || []).map(text).join('');
const find = (el, cls, out = []) => { if (el && typeof el === 'object') { if ((el.class || '').split(' ').includes(cls)) out.push(el); (el.children || []).forEach((c) => find(c, cls, out)); } return out; };

let board, opened, panel;
const c = {
  h, field: (label, input) => h('label', {}, label, input), formError: () => ({ hidden: true }),
  submitter: (_e, run) => run, act: (_e, run) => run,
  api: async () => board,
  panels: { add: (name, title, opts, ...content) => { panel = h('aside', {}, ...content); }, open: (name) => { opened = name; } },
};
vm.createContext(c);
vm.runInContext(source.slice(start, end) + ';this.openLific = openLific; this.loadLific = loadLific;', c);
const [info, , columns, link] = panel.children;
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

  // Unavailable says why, with no columns.
  board = { available: false, unavailable: 'Lific is busy', you_can_link: false, columns: [] };
  c.openLific({ id: 'item-1' });
  await settle();
  assert.equal(columns.children.length, 0);
  assert.equal(text(info), 'Lific is busy');
  console.log('Lific kanban UI: four columns even when empty, cards as text, Cancelled when used, link form folded passed');
})().catch((e) => { console.error(e); process.exitCode = 1; });
