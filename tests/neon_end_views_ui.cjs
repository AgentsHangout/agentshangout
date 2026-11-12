// node tests/neon_end_views_ui.cjs: #1121 (I47). The 2-cell Neon Nights kinds (claw machine, neon sign,
// prize counter) and the retro cabinet have their own end view when turned, not their front again.
// Turned, a 2x1 is 1 cell (12px) across and 2 deep (24px), and its end must fit its turned reach.
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

const kinds = ['claw_machine', 'neon_sign', 'prize_counter', 'retro_cabinet'];
for (const kind of kinds) {
  const item = ITEMS[kind];
  assert.ok(item.side, `${kind} has no side`);
  assert.notEqual(item.side, item.paint, `${kind} is still drawn from its front when turned`);
  for (const t of [0, 300, 600, 900, 5000]) {
    const end = canvas();
    item.side(end, 100, 100, t, 12, 24);
    assert.ok(end.calls.length > 0, `${kind} draws nothing from its end at t=${t}`);
    for (const c of end.calls) {
      assert.ok([c.x, c.y, c.w, c.h].every(Number.isFinite) && c.w >= 0 && c.h >= 0, `${kind} ${JSON.stringify(c)}`);
      // Stays within its turned width (12px, centred on 100) and within its turned height.
      assert.ok(c.x >= 94 && c.x + c.w <= 106, `${kind} is wider than its end: ${JSON.stringify(c)}`);
      assert.ok(c.y >= 100 - item.rise - 12, `${kind} rises above its turned reach: ${JSON.stringify(c)}`);
    }
  }
}

console.log('neon end views: ok');
