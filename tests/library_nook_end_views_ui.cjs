// node tests/library_nook_end_views_ui.cjs: the Library nook end views. The writing desk, bookcase
// ladder, card catalog, map cabinet and book cart have their own end when turned, not a plain edge.
// Each end stays inside its turned reach: deep + rise - wide above its base (the desk is 24 across
// and 36 deep, the other four 12 by 24).
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const src = fs.readFileSync('web/room.js', 'utf8').replace(/^export /gm, '');
const ctx = vm.createContext({ console, Math, Date });
vm.runInContext(src + '\n;globalThis.__t = { ITEMS };\n', ctx);
const { ITEMS } = ctx.__t;

function canvas() {
  const calls = [];
  return {
    calls,
    fillStyle: null,
    fillRect(x, y, w, h) { calls.push({ color: this.fillStyle, x, y, w, h }); },
  };
}

const kinds = [
  ['writing_desk', 24, 36],
  ['bookcase_ladder', 12, 24],
  ['card_catalog', 12, 24],
  ['map_cabinet', 12, 24],
  ['book_cart', 12, 24],
];
for (const [kind, wide, deep] of kinds) {
  const item = ITEMS[kind];
  assert.ok(item.side, `${kind} has no side`);
  assert.notEqual(item.side, item.paint, `${kind} is still drawn from its front when turned`);
  const reach = deep + item.rise - wide;
  for (const t of [0, 300, 600, 5000]) {
    const end = canvas();
    item.side(end, 100, 100, t, wide, deep);
    assert.ok(end.calls.length > 0, `${kind} draws nothing from its end at t=${t}`);
    for (const c of end.calls) {
      assert.ok([c.x, c.y, c.w, c.h].every(Number.isFinite) && c.w >= 0 && c.h >= 0, `${kind} ${JSON.stringify(c)}`);
      assert.ok(c.x >= 100 - wide / 2 && c.x + c.w <= 100 + wide / 2, `${kind} is wider than its end: ${JSON.stringify(c)}`);
      assert.ok(c.y >= 100 - reach, `${kind} rises above its turned reach: ${JSON.stringify(c)}`);
    }
  }
}

console.log('library nook end views: ok');
