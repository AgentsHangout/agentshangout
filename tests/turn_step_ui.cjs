// node tests/turn_step_ui.cjs: #1169. Turn steps a quarter (1) for a kind with an end to show, and
// by 2 (to the mirror, facing 2) for a kind with none, since the server refuses facing 1 and 3 for it.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const src = fs.readFileSync('web/room.js', 'utf8').replace(/^export /gm, '');
const ctx = vm.createContext({ console, Math, Date });
vm.runInContext(src + '\n;globalThis.__t = { ITEMS, turnsToEnd };\n', ctx);
const { ITEMS, turnsToEnd } = ctx.__t;

// The step app.js takes for a kind, as its turnStep does.
const turnStep = (kind) => (turnsToEnd(kind) ? 1 : 2);

assert.equal(turnsToEnd('dining_chair'), true);
assert.equal(turnStep('dining_chair'), 1);
assert.equal(turnsToEnd('plant'), false);
assert.equal(turnStep('plant'), 2);
assert.equal(turnsToEnd('potted_tree'), false, 'its side is its front again');
assert.equal(turnsToEnd('no_such_kind'), false);
assert.equal(turnsToEnd('constructor'), false);

// A no-end kind goes 0 -> 2 -> 0 and never reaches 1 or 3, so every press is one the server takes.
let facing = 0;
for (const expected of [2, 0, 2]) {
  facing = (facing + turnStep('plant')) % 4;
  assert.equal(facing, expected);
}

// Every kind with a side that isn't its front is turnable, and the rest are not, so the two lists agree.
for (const [kind, entry] of Object.entries(ITEMS)) {
  const hasEnd = Boolean(entry.side) && entry.side !== entry.paint;
  assert.equal(turnsToEnd(kind), hasEnd, kind);
}
console.log('turn step ok');
