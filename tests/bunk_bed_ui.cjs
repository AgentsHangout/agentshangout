// node tests/bunk_bed_ui.cjs: #837. A bunk bed has two sleeping spots, the top one higher than the
// bottom one. Runs the shipped spotsOf and USES from web/room.js, with clearOf as the identity.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const room = fs.readFileSync('web/room.js', 'utf8').replace(/\r\n/g, '\n');
const slice = (from, end) => {
  const start = room.indexOf(from);
  assert.ok(start >= 0, from);
  return room.slice(start, room.indexOf(end, start) + end.length);
};
const uses = slice('const USES = {', '\n};\n');
// The paint functions are only referenced here, never run, so each can be a stand-in.
const context = { clearOf: (x, y) => [x, y] };
for (const name of new Set(uses.match(/\b(paint\w+|endOf)\b/g))) context[name] = () => () => {};
vm.createContext(context);
vm.runInContext(`${uses}
${slice('function spotsOf(', '\n}\n')}
this.USES = USES; this.spotsOf = spotsOf;`, context);

const item = { use: context.USES.bunk_bed, side: undefined, facing: 0, left: 40, top: 100, wide: 48, deep: 24, y: 124 };
const [bottom, top] = context.spotsOf(item);
assert.equal(context.spotsOf(item).length, 2, 'two sleeping spots');
assert.equal(bottom.pose, 'sleep');
assert.equal(top.pose, 'sleep');
assert.equal(bottom.x, top.x, 'one above the other, in the middle');
assert.ok(top.y < bottom.y, 'the top bunk is higher on the page');
assert.equal(top.y - bottom.y, -22);
// Turned to the side (facing 1 or 3), the two bunks still land apart: the top one is higher.
for (const facing of [1, 3]) {
  const turned = { ...item, side: 'left', facing };
  const [low, high] = context.spotsOf(turned);
  assert.equal(low.pose, 'sleep');
  assert.equal(high.pose, 'sleep');
  assert.ok(low.x !== undefined && (low.x !== high.x || low.y !== high.y), `facing ${facing}: bunks on one spot`);
  assert.ok(high.ay < low.ay, `facing ${facing}: the top bunk is higher on the page`);
  assert.equal(low.ay - high.ay, 22, `facing ${facing}: 22px apart`);
}
// A double bed still has one sleeping spot per bed, and each of its spots is a single bed's.
const double = { ...item, use: { spots: 1, pose: 'sleep', on: true } };
assert.equal(context.spotsOf(double).length, 1);
console.log('Bunk bed: two sleeping spots, the top one higher (run, not just read).');
