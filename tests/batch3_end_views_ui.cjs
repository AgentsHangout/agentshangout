// node tests/batch3_end_views_ui.cjs: #1121 batch 3. Six pieces (guitar, drum kit, radio, telescope,
// dartboard, teapot) get an end view for when they're turned. Each end stays inside its footprint
// across its wide and inside its turned reach (deep + rise - wide above its base).
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

const BATCH = {
  guitar: { cells: 1, paint: 'paintGuitar' },
  drum_kit: { cells: 3, paint: 'paintDrumKit' },
  radio: { cells: 1, paint: 'paintRadio' },
  telescope: { cells: 2, paint: 'paintTelescope' },
  dartboard: { cells: 1, paint: 'paintDartboard' },
  teapot: { cells: 1, paint: 'paintTeapot' },
};

for (const [kind, { cells, paint }] of Object.entries(BATCH)) {
  const item = ITEMS[kind];
  assert.ok(item, `${kind} is a kind`);
  assert.equal(typeof item.side, 'function', `${kind} has an end view`);
  assert.notEqual(item.side, item.paint, `${kind}'s end view is not its front`);

  const wide = 12 * cells, deep = 12 * cells, reach = deep + item.rise - wide;
  const left = -wide / 2, right = wide / 2;
  const front = canvas();
  item.paint(front, 100, 100, 0);
  assert.ok(front.calls.length > 0, `${kind} front paints`);

  for (const t of [0, 300, 5000]) {
    const end = canvas();
    item.side(end, 100, 100, t, wide, deep);
    assert.ok(end.calls.length > 0, `${kind} end paints something at t=${t}`);
    for (const c of end.calls) {
      assert.ok([c.x, c.y, c.w, c.h].every(Number.isFinite) && c.w >= 0 && c.h >= 0, `${kind}: ${JSON.stringify(c)}`);
      assert.ok(c.x - 100 >= left && c.x + c.w - 100 <= right, `${kind} inside its width: ${JSON.stringify(c)}`);
      assert.ok(c.y >= 100 - reach && c.y + c.h <= 101, `${kind} inside its reach: ${JSON.stringify(c)}`);
    }
  }

  // Turned, the piece draws its end, not its front.
  const turned = canvas();
  item.side(turned, 100, 100, 0, wide, deep);
  assert.notDeepEqual(
    turned.calls.map((c) => [c.x, c.w]),
    front.calls.map((c) => [c.x, c.w]),
    `${kind} turned is not the front again`,
  );
}

console.log('batch 3 end views (six pieces): ok');
