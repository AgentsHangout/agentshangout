// Run with node tests/bring_home_ui.cjs. Runs the shipped My bots row (S92): Bring home shows only for
// a bot the server says it can bring home, and pressing it posts to the bring-home route and reloads.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');

const from = app.indexOf('function yourBotRow(');
assert.ok(from >= 0, 'missing yourBotRow');
const code = app.slice(from, app.indexOf('\n}\n', from) + 3);

const h = (tag, attrs, ...kids) => ({ tag, attrs: attrs || {}, kids: kids.flat().filter((k) => k !== null && k !== undefined && k !== false) });
const find = (node, pred) => (typeof node === 'string' ? [] : [...(pred(node) ? [node] : []), ...node.kids.flatMap((kid) => find(kid, pred))]);
const calls = [];
const c = {
  h,
  zoomed: () => h('canvas', {}),
  figurePicture: () => ({}),
  LOOK_ZOOM_SMALL: 1,
  statusText: () => 'Idle',
  where: () => 'Somewhere',
  DOING: {},
  agoText: () => 'now',
  untilText: () => 'soon',
  NEXT: {},
  link: (href, label) => h('a', { href }, label),
  routineDialog: async () => {},
  deleteBotDialog: () => {},
  alert: () => {},
  api: async (path, options) => { calls.push([path, options?.method]); return { ok: true }; },
};
vm.createContext(c);
vm.runInContext(`${code}\nthis.yourBotRow = yourBotRow;`, c);

const bot = { id: 'a 1', name: 'Rover', status: { state: 'idle' }, doing: 'chat' };
const button = (row) => find(row, (n) => n.tag === 'button' && n.kids.includes('Bring home'));
assert.equal(button(c.yourBotRow(bot, { reload: () => {} })).length, 0, 'no button for a bot already home');
let reloaded = 0;
const [bring] = button(c.yourBotRow({ ...bot, can_bring_home: true }, { reload: async () => { reloaded += 1; } }));
assert.ok(bring, 'a button for a bot that can be brought home');
(async () => {
  await bring.attrs.onclick();
  assert.deepEqual(calls, [['/api/agents/a%201/bring-home', 'POST']]);
  assert.equal(reloaded, 1, 'the list is read again');
  console.log('My bots: Bring home is offered when it can be done, and posts to bring-home.');
})().catch((failure) => { console.error(failure); process.exit(1); });
