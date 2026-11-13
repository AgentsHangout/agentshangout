// node tests/turn_step_ui.cjs: #1169. Turn goes a quarter for a kind with an end to show. A kind with
// none goes 0 -> 2 (the mirror) and to 0 from any other facing, since the server refuses 1 and 3 for it.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const src = fs.readFileSync('web/room.js', 'utf8').replace(/^export /gm, '');
const ctx = vm.createContext({ console, Math, Date });
vm.runInContext(src + '\n;globalThis.__t = { ITEMS, turnsToEnd, nextFacing };\n', ctx);
const { ITEMS, turnsToEnd, nextFacing } = ctx.__t;

assert.equal(turnsToEnd('dining_chair'), true);
assert.equal(turnsToEnd('plant'), false);
assert.equal(turnsToEnd('potted_tree'), false, 'its side is its front again');
assert.equal(turnsToEnd('no_such_kind'), false);
assert.equal(turnsToEnd('constructor'), false);

// An end kind turns a quarter each press, through all four facings.
let facing = 0;
for (const expected of [1, 2, 3, 0]) {
  facing = nextFacing('dining_chair', facing);
  assert.equal(facing, expected);
}

// A no-end kind from a plant at 0 goes 0 -> 2 -> 0, and never to 1 or 3.
facing = 0;
for (const expected of [2, 0, 2, 0]) {
  facing = nextFacing('plant', facing);
  assert.equal(facing, expected);
}

// A no-end kind stored at facing 1 or 3 goes to 0, not to 3 or 1 (which the server would refuse).
assert.equal(nextFacing('plant', 1), 0);
assert.equal(nextFacing('plant', 3), 0);
assert.equal(nextFacing('plant', 2), 0);

// Every kind with a side that isn't its front is turnable, and the rest are not, so the two lists agree.
for (const [kind, entry] of Object.entries(ITEMS)) {
  const hasEnd = Boolean(entry.side) && entry.side !== entry.paint;
  assert.equal(turnsToEnd(kind), hasEnd, kind);
}
console.log('turn step ok');
