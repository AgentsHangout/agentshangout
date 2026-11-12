// node tests/bean_bag_end_ui.cjs: #1121. The bean bag is one cell, so its front is unchanged, and
// turned (facing 1 or 3) it is drawn from its end instead of showing the same front picture again.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const src = fs.readFileSync('web/room.js', 'utf8').replace(/^export /gm, '');
const ctx = vm.createContext({ console, Math, Date });
vm.runInContext(src + '\n;globalThis.__t = { ITEMS, paintBeanBag, paintBeanBagEnd };\n', ctx);
const { ITEMS, paintBeanBag, paintBeanBagEnd } = ctx.__t;

function canvas() {
  const calls = [];
  return {
    calls,
    fillStyle: null,
    fillRect(x, y, w, h) { calls.push({ color: this.fillStyle, x, y, w, h }); },
  };
}

// The front, unchanged: the same rectangles as before the end view.
const front = canvas();
paintBeanBag(front, 100, 100);
assert.deepEqual(front.calls.map(({ color, x, y, w, h }) => [color, x, y, w, h]), [
  ['#00000030', 93, 99, 14, 2],
  ['#d92d86', 93, 91, 14, 8],
  ['#ff3ea5', 94, 89, 12, 3],
  ['#ff3ea5', 95, 88, 10, 1],
]);

// The end: one cell across (12px), inside its turned reach of 12px above the base, at every frame.
const bean = ITEMS.bean_bag_chair;
assert.equal(bean.side, paintBeanBagEnd, 'the bean bag has an end view');
for (const t of [0, 300, 5000]) {
  const end = canvas();
  paintBeanBagEnd(end, 100, 100, t, 12, 12);
  assert.ok(end.calls.length > 0, 'the end paints something');
  for (const c of end.calls) {
    assert.ok([c.x, c.y, c.w, c.h].every(Number.isFinite) && c.w >= 0 && c.h >= 0, JSON.stringify(c));
    assert.ok(c.x >= 94 && c.x + c.w <= 106, `inside its one-cell width: ${JSON.stringify(c)}`);
    assert.ok(c.y >= 88 && c.y + c.h <= 102, `inside its reach: ${JSON.stringify(c)}`);
  }
}

// Turned, the bean bag draws its end, not its front.
const turned = canvas();
bean.side(turned, 100, 100, 0, 12, 12);
assert.notDeepEqual(turned.calls.map((c) => [c.x, c.w]), front.calls.map((c) => [c.x, c.w]));

console.log('bean bag end view: ok');
