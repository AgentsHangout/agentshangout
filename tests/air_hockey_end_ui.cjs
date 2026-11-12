// node tests/air_hockey_end_ui.cjs: #1121. The air hockey table is drawn at the size it's given:
// its front is unchanged at 3 cells wide, and turned (facing 1 or 3) it is drawn from its end, a
// 2-cell end face with its top running back 3 cells, with the cyan centre line along the top.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const src = fs.readFileSync('web/room.js', 'utf8').replace(/^export /gm, '');
const ctx = vm.createContext({ console, Math, Date });
vm.runInContext(src + '\n;globalThis.__t = { ITEMS, paintAirHockey, paintAirHockeyEnd };\n', ctx);
const { ITEMS, paintAirHockey, paintAirHockeyEnd } = ctx.__t;

function canvas() {
  const calls = [];
  return {
    calls,
    fillStyle: null,
    fillRect(x, y, w, h) { calls.push({ color: this.fillStyle, x, y, w, h }); },
  };
}

// The front, 3 cells wide at x=100 (the same rectangles as before #1121, at t=0).
const front = canvas();
paintAirHockey(front, 100, 100, 0, 36);
assert.deepEqual(front.calls.map(({ color, x, y, w, h }) => [color, x, y, w, h]), [
  ['#00000030', 82, 98, 36, 2],
  ['#3a3560', 84, 92, 3, 7],
  ['#3a3560', 113, 92, 3, 7],
  ['#26233f', 82, 86, 36, 7],
  ['#0d3a5c', 84, 80, 32, 7],
  ['#ff3ea5', 83, 79, 34, 1],
  ['#2fe6ff', 99, 80, 2, 6],
  ['#f4f1e8', 99, 82, 3, 3],
]);

// Turned 3x2: the end is 2 cells (24px) wide, its top 3 cells (36px) deep, so 12px of top band.
for (const t of [0, 300, 600, 900, 5000]) {
  const end = canvas();
  paintAirHockeyEnd(end, 100, 100, t, 24, 36);
  const face = end.calls.find((c) => c.color === '#26233f');
  assert.equal(face.w, 24, 'end face is 2 cells wide');
  const band = end.calls.find((c) => c.color === '#0d3a5c');
  assert.equal(band.h, 12, 'top runs back 3 cells');
  const line = end.calls.find((c) => c.color === '#2fe6ff');
  assert.deepEqual([line.w, line.h, line.y], [2, 12, band.y], 'cyan centre line runs back along the top');
  const puck = end.calls.find((c) => c.color === '#f4f1e8');
  assert.ok(puck.y >= band.y && puck.y + puck.h <= band.y + band.h, `puck stays on the top at t=${t}`);
  for (const c of end.calls) assert.ok([c.x, c.y, c.w, c.h].every(Number.isFinite) && c.w >= 0 && c.h >= 0, JSON.stringify(c));
}

// Sized from its arguments: a longer end (a turned 3x3 would be 36 deep) gives a taller top.
const longer = canvas();
paintAirHockeyEnd(longer, 100, 100, 0, 24, 48);
assert.equal(longer.calls.find((c) => c.color === '#0d3a5c').h, 16);

// The table's side is the end view, and it paints when turned.
const at = ITEMS.air_hockey;
assert.equal(at.side, paintAirHockeyEnd);
const all = canvas();
at.side(all, 100, 100, 0, 24, 36);
assert.ok(all.calls.length > 0);

console.log('air hockey end view: ok');
