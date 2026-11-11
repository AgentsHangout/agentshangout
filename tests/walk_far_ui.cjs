// Run with node tests/walk_far_ui.cjs. W121: runs the shipped walkLegs, walkInLegs and walkItem from
// web/app.js. A far "Walk here" is cut into legs of at most 16 tiles, shows its cost, sends nothing
// without enough moves, and a refused leg stops the chain.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');

const grab = (start) => {
  const from = app.indexOf(start);
  assert.ok(from >= 0, `missing ${start}`);
  return app.slice(from, app.indexOf('\n  }\n', from) + 5);
};
const range = app.match(/const WALK_RANGE = \d+;/)[0];
const code = [range, grab('function walkLegs('), grab('async function walkInLegs('), grab('function walkItem(')].join('\n');

function setup({ moves, refuse }) {
  const sent = [];
  const said = [];
  const c = {
    latest: { players: [[7, 'me', 10, 50]] },
    myId: '7',
    moves,
    world: { n: 100, findPath: (sx, sy, tx, ty) => { const r = []; for (let x = sx + 1; x <= tx; x += 1) r.push([x, ty]); return r; } },
    say: (text) => said.push(text),
    send: async (body) => { sent.push(body); return !(refuse && sent.length === refuse); },
  };
  vm.createContext(c);
  vm.runInContext(`${code}\nthis.walkItem = walkItem; this.walkLegs = walkLegs;`, c);
  return { c, sent, said };
}

(async () => {
  // A 40-tile straight walk: 3 legs, none over 16 tiles, and the menu says 3 moves.
  let { c, sent } = setup({ moves: 10 });
  const item = c.walkItem({ x: 50, y: 50 });
  assert.equal(item.label, 'Walk here');
  assert.equal(item.cost, '3 moves');
  await item.run();
  assert.equal(sent.length, 3);
  let at = 10;
  for (const leg of sent) {
    assert.equal(leg.action, 'walk');
    assert.ok(leg.x - at <= 16 && leg.x > at, 'each leg is at most 16 tiles');
    at = leg.x;
  }
  assert.equal(at, 50, 'the last leg ends at the target');

  // A target within range is one walk, as before.
  ({ c, sent } = setup({ moves: 10 }));
  const near = c.walkItem({ x: 26, y: 50 });
  assert.equal(near.cost, '1 move');
  await near.run();
  assert.equal(JSON.stringify(sent), JSON.stringify([{ action: 'walk', x: 26, y: 50 }]));

  // Too few moves: it says so up front and sends nothing.
  let said;
  ({ c, sent, said } = setup({ moves: 2 }));
  const short = c.walkItem({ x: 50, y: 50 });
  assert.equal(short.cost, '3 moves, you have 2');
  await short.run();
  assert.equal(sent.length, 0);
  assert.equal(said.length, 1);
  assert.match(said[0], /3 moves/);

  // A refused leg stops the chain (send has shown the reason).
  ({ c, sent } = setup({ moves: 10, refuse: 2 }));
  await c.walkItem({ x: 50, y: 50 }).run();
  assert.equal(sent.length, 2, 'nothing is sent after the refused leg');
  console.log('Map: a far Walk here goes in legs of at most 16 tiles, shows its cost, and stops on a refusal.');
})().catch((failure) => { console.error(failure); process.exit(1); });
