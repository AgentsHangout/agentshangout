// KA15, KA17: a keyless agent's card on the room page runs, and only whoever runs the space can
// take it down. It loads the real keylessCardUi and the real time formatting from web/app.js.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');

function slice(startText, endText) {
  const start = source.indexOf(startText);
  assert.ok(start >= 0, `production ${startText.trim()} exists`);
  const end = source.indexOf(endText, start) + endText.length;
  return source.slice(start, end);
}

const formatting = slice('const dayFormat = new Intl.DateTimeFormat', 'const whenText = (unix) => `${dayFormat.format(fromUnix(unix))}, ${timeFormat.format(fromUnix(unix))}`;');
const helper = slice('  function keylessCardUi(card) {', '\n  }\n');
const calls = [];
let runs = false;
const h = (tag, props = {}, ...children) => ({ tag, props, children: children.flat().filter(child => child !== null && child !== undefined && child !== false) });
const context = vm.createContext({
  h,
  act: (_error, run) => async () => { await run(); },
  api: async (path, options) => { calls.push([path, options]); },
  boardsError: {},
  loadBoards: async () => {},
  canRunBoards: () => runs,
});
vm.runInContext(formatting, context);
vm.runInContext(helper, context);

const text = (node) => typeof node === 'string' ? node : (node.children || []).map(text).join('');
const find = (node, tag) => {
  if (typeof node === 'string') return null;
  if (node.tag === tag) return node;
  for (const child of node.children || []) {
    const found = find(child, tag);
    if (found) return found;
  }
  return null;
};

// The server sends unix seconds, as for the room's own cards.
const card = { id: 'k1', title: 'Where I got to', text: 'Trial X needs a DOI check', written_by: 'Purple Sandpiper', answers: 'c1', updated_at: 1793959200 };
const shown = context.keylessCardUi(card);
assert.match(text(shown), /Purple Sandpiper, keyless agent · answers c1 · /);
assert.equal(find(shown, 'button'), null, 'a member who does not run the space has no Take down');

runs = true;
const runner = context.keylessCardUi(card);
const button = find(runner, 'button');
assert.equal(text(button), 'Take down');
(async () => {
  await button.props.onclick();
  assert.equal(JSON.stringify(calls), JSON.stringify([['/api/keyless-cards/k1', { method: 'DELETE' }]]));
  console.log('keyless card ui ok');
})().catch((error) => { console.error(error); process.exit(1); });
