// node tests/credits_glance_ui.cjs: HC54, the credits glance. Runs the shipped nextBuy,
// creditGlance, stylesAlone and styleTile from web/app.js against a credits reply, not just their text.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
const slice = (from, end) => {
  const start = app.indexOf(from);
  assert.ok(start >= 0, from);
  return app.slice(start, app.indexOf(end, start) + end.length);
};
const h = (tag, props = {}, ...children) => ({ tag, ...props, children: children.flat().filter(Boolean) });
const count = (n, one, many) => `${n} ${n === 1 ? one : many}`;
const fitPicture = (p) => p;
const wallPicture = (p) => `wall:${p}`;
const floorPicture = (f) => `floor:${f}`;
const itemPicture = (i) => `item:${i}`;
const act = () => () => {};
const error = () => {};
let confirming = null;
let now = { balance: 10 };
const context = { h, count, fitPicture, wallPicture, floorPicture, itemPicture, act, error, confirming, now };
vm.createContext(context);
const tileCode = slice('const singleTile = ', ');\n    const packRow').replace('\n    const packRow', '');
vm.runInContext(`${slice('const sentence = ', ';\n')}
${slice('function nextBuy(credits) {', '\n}\n')}
${slice('function creditGlance(credits) {', '\n}\n')}
${slice('function stylesAlone(offer) {', '\n}\n')}
${tileCode}
this.nextBuy = nextBuy; this.creditGlance = creditGlance; this.stylesAlone = stylesAlone; this.styleTile = styleTile;`, context);

const credits = (balance, packs) => ({ balance, earned_today: 6, packs });
const stargazer = {
  pack: 'stargazer', cost: 40, bought: false, over: false,
  sold_alone: [{ kind: 'orrery', label: 'Orrery', cost: 8, bought: false }, { kind: 'globe', label: 'Globe', cost: 3, bought: true }],
  styles: [
    { kind: 'paint', style: 'nebula', label: 'Nebula walls', cost: 5, bought: true },
    { kind: 'flooring', style: 'starfield', label: 'Starfield floor', cost: 5, bought: false },
  ],
};
const spooky = { pack: 'spooky', cost: 1, bought: false, over: true, sold_alone: [], styles: [] };
// An over pack that still has pieces listed. The server keeps over packs in sold_alone (economy.rs
// builds it from every pack, over ones included) and spend() refuses them, so nextBuy must drop
// the whole pack, pieces too. This piece is cheaper than the orrery, so it would win if it leaked.
const harvest = {
  pack: 'harvest', cost: 2, bought: false, over: true,
  sold_alone: [{ kind: 'scarecrow', label: 'Scarecrow', cost: 2, bought: false }],
  styles: [],
};

// The cheapest one you don't have and can still buy: the bought globe, the bought nebula walls,
// and the season that's over don't count. The unbought starfield floor (5) is cheaper than the orrery (8).
assert.deepEqual({ ...context.nextBuy(credits(5, [stargazer, spooky])) }, { name: 'Starfield floor', cost: 5 });
assert.deepEqual({ ...context.nextBuy(credits(5, [stargazer, spooky, harvest])) }, { name: 'Starfield floor', cost: 5 });
assert.equal(context.creditGlance(credits(3, [stargazer, spooky, harvest])), '6 earned today · Starfield floor: 2 more');
assert.equal(context.creditGlance(credits(5, [stargazer])), '6 earned today · You can buy the starfield floor now');

// With both styles bought, the orrery is next.
const stylesBought = {
  ...stargazer,
  styles: stargazer.styles.map((s) => ({ ...s, bought: true })),
};
assert.deepEqual({ ...context.nextBuy(credits(5, [stylesBought, spooky])) }, { name: 'Orrery', cost: 8 });
assert.equal(context.creditGlance(credits(5, [stylesBought, spooky])), '6 earned today · Orrery: 3 more');
assert.equal(context.creditGlance(credits(8, [stylesBought])), '6 earned today · You can buy the orrery now');

// With every piece and style yours, the pack itself is next.
const allPieces = { ...stylesBought, sold_alone: stargazer.sold_alone.map((p) => ({ ...p, bought: true })) };
assert.equal(context.creditGlance(credits(50, [allPieces])), '6 earned today · You can buy the stargazer pack now');
// Nothing left to buy: just today.
assert.equal(context.creditGlance(credits(50, [{ ...stargazer, bought: true }])), '6 earned today');
assert.equal(context.creditGlance(credits(0, [])), '6 earned today');

// Styles alone: both styles offered before the pack is bought; none once bought.
assert.deepEqual([...context.stylesAlone(stargazer)].map((s) => ({ ...s })), stargazer.styles);
assert.deepEqual([...context.stylesAlone({ ...stargazer, bought: true })], []);

// Style tiles: bought style shows "Yours" pill; unbought style shows Buy button.
const boughtTile = context.styleTile(stargazer.styles[0]);
assert.equal(boughtTile.children.some((c) => c.class === 'pill on' && c.children.includes('Yours')), true);
const unboughtTile = context.styleTile(stargazer.styles[1]);
assert.equal(unboughtTile.children.some((c) => c.tag === 'button' && c.children.includes('Buy')), true);

console.log('Credits glance: earned today, styles, and the next thing to buy (run, not just read).');
