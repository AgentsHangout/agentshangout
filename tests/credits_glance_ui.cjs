// node tests/credits_glance_ui.cjs: HC54, the credits glance. Runs the shipped nextBuy and
// creditGlance from web/app.js against a credits reply, not just their text.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
const slice = (from, end) => {
  const start = app.indexOf(from);
  assert.ok(start >= 0, from);
  return app.slice(start, app.indexOf(end, start) + end.length);
};
const context = {};
vm.createContext(context);
vm.runInContext(`${slice('const sentence = ', ';\n')}
${slice('function nextBuy(credits) {', '\n}\n')}
${slice('function creditGlance(credits) {', '\n}\n')}
this.nextBuy = nextBuy; this.creditGlance = creditGlance;`, context);

const credits = (balance, packs) => ({ balance, earned_today: 6, packs });
const stargazer = {
  pack: 'stargazer', cost: 40, bought: false, over: false,
  sold_alone: [{ kind: 'orrery', label: 'Orrery', cost: 8, bought: false }, { kind: 'globe', label: 'Globe', cost: 3, bought: true }],
};
const spooky = { pack: 'spooky', cost: 1, bought: false, over: true, sold_alone: [] };
// An over pack that still has pieces listed. The server keeps over packs in sold_alone (economy.rs
// builds it from every pack, over ones included) and spend() refuses them, so nextBuy must drop
// the whole pack, pieces too. This piece is cheaper than the orrery, so it would win if it leaked.
const harvest = {
  pack: 'harvest', cost: 2, bought: false, over: true,
  sold_alone: [{ kind: 'scarecrow', label: 'Scarecrow', cost: 2, bought: false }],
};

// The cheapest one you don't have and can still buy: the bought globe and the season that's over
// don't count.
assert.deepEqual({ ...context.nextBuy(credits(5, [stargazer, spooky])) }, { name: 'Orrery', cost: 8 });
assert.deepEqual({ ...context.nextBuy(credits(5, [stargazer, spooky, harvest])) }, { name: 'Orrery', cost: 8 });
assert.equal(context.creditGlance(credits(5, [stargazer, spooky, harvest])), '6 earned today · Orrery: 3 more');
assert.equal(context.creditGlance(credits(8, [stargazer])), '6 earned today · You can buy the orrery now');
// With every piece yours, the pack itself is next.
const allPieces = { ...stargazer, sold_alone: stargazer.sold_alone.map((p) => ({ ...p, bought: true })) };
assert.equal(context.creditGlance(credits(50, [allPieces])), '6 earned today · You can buy the stargazer pack now');
// Nothing left to buy: just today.
assert.equal(context.creditGlance(credits(50, [{ ...stargazer, bought: true }])), '6 earned today');
assert.equal(context.creditGlance(credits(0, [])), '6 earned today');
console.log('Credits glance: earned today and the next thing to buy (run, not just read).');
