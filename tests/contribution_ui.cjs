// HC36, HC37, HC43: a card's contributions, and Accept only where the server says the caller may.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
const start = source.indexOf('  function contributionsCardUi(card) {');
assert.ok(start >= 0, 'production contributionsCardUi exists');
const end = source.indexOf('\n  }\n', start) + 4;
const calls = [];
const errors = [];
let refreshes = 0;
let failPath = null;
const h = (tag, props = {}, ...children) => ({ tag, props, children: children.flat().filter(child => child !== null && child !== undefined && child !== false) });
const context = vm.createContext({
  h,
  act: (_error, run) => async () => {
    try { await run(); } catch (error) { errors.push(error.message); }
  },
  api: async (path, options) => {
    calls.push([path, options]);
    if (path === failPath) throw new Error('request failed');
  },
  confirm: () => true,
  boardsError: {},
  loadBoards: async () => { refreshes++; },
});
vm.runInContext(source.slice(start, end), context);
const all = node => node && typeof node === 'object' ? [node, ...(node.children || []).flatMap(all)] : [];
const texts = node => all(node).flatMap(n => n.children || []).filter(child => typeof child === 'string');
const offered = (overrides = {}) => ({ id: 'c/1', kind: 'source', text: 'See the trial', bot_name: 'Scout', owner_name: 'Bo', accepted: false, can_accept: false, ...overrides });

async function run() {
  // Nothing offered, nothing shown.
  assert.equal(context.contributionsCardUi({ id: 'card', contributions: [] }), null);
  assert.equal(context.contributionsCardUi({ id: 'card' }), null);

  // A member who isn't the card's owner sees what was offered, and no button.
  const member = context.contributionsCardUi({ id: 'card', contributions: [offered()] });
  assert.equal(all(member).filter(node => node.tag === 'button').length, 0);
  assert.ok(texts(member).includes('Source: '));
  assert.ok(texts(member).includes('Scout, for Bo'));

  // A guest's view has no owner, so only the bot is named.
  const guest = context.contributionsCardUi({ id: 'card', contributions: [{ id: 'c/1', kind: 'summary', text: 'In short', bot_name: 'Scout' }] });
  assert.ok(texts(guest).includes('Scout'));
  assert.ok(!texts(guest).some(text => text.includes('for ')));

  // The card's owner presses Accept: one call to the exact route, then the boards reload.
  const owner = context.contributionsCardUi({ id: 'card/a', contributions: [offered({ can_accept: true })] });
  const button = all(owner).find(node => node.tag === 'button');
  assert.ok(button.children.includes('Accept'));
  await button.props.onclick();
  assert.equal(JSON.stringify(calls.at(-1)), JSON.stringify(['/api/cards/card%2Fa/contributions/accept', { method: 'POST', body: { contribution_id: 'c/1' } }]));
  assert.equal(refreshes, 1);

  // An accepted one says so and offers nothing more.
  const paid = context.contributionsCardUi({ id: 'card', contributions: [offered({ accepted: true })] });
  assert.ok(texts(paid).includes('Accepted · the site paid 5 credits'));
  assert.equal(all(paid).filter(node => node.tag === 'button').length, 0);

  // A refused accept shows the error and doesn't reload as if it paid.
  failPath = '/api/cards/no/contributions/accept';
  const failing = context.contributionsCardUi({ id: 'no', contributions: [offered({ can_accept: true })] });
  await all(failing).find(node => node.tag === 'button').props.onclick();
  assert.equal(refreshes, 1);
  assert.deepEqual(errors, ['request failed']);
  console.log('HC36, HC37, HC43: contributions, Accept and refusals pass.');
}
run().catch(error => { console.error(error); process.exitCode = 1; });
