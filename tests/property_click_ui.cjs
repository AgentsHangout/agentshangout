// W102: one rule for hovering and clicking a place on the map and a thing in a room.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const read = (f) => fs.readFileSync(f, 'utf8').replace(/\r\n/g, '\n');
const world = read('web/world.js');
const app = read('web/app.js');

// pickAt: a left click opens the menu on a figure or a clickable place, never on bare land.
const start = world.indexOf('  pickAt(clientX, clientY, via) {');
assert.ok(start >= 0, 'pickAt');
const body = world.slice(start, world.indexOf('\n  }\n', start) + 4);
const World = vm.runInNewContext(`(class { ${body} })`, { performance: { now: () => 1000 } });
function pick(via, { ent = null, kind = null } = {}) {
  const w = new World();
  const seen = [];
  Object.assign(w, {
    onPick: (p) => seen.push(p), ready: true, menuAt: 0,
    canvas: { getBoundingClientRect: () => ({ left: 0, top: 0 }) },
    pickEntity: () => ent, toArt: (x, y) => [x, y], tileAtArt: () => ({ x: 1, y: 2 }),
    tileInfo: () => ({ x: 1, y: 2, kind }), figureInfo: (e) => e, hideTip() {},
    clickable: (t) => t.kind === 'house',
  });
  return { opened: w.pickAt(5, 5, via), seen };
}
assert.ok(pick('click', { kind: 'house' }).opened, 'left click on a place opens the menu');
assert.ok(pick('context', { kind: 'house' }).opened, 'right click on a place opens the menu');
assert.ok(pick('press', { kind: 'house' }).opened, 'press and hold on a place opens the menu');
assert.ok(pick('click', { ent: { type: 'player' } }).opened, 'click on a figure still opens its menu');
assert.ok(!pick('click', { kind: 'road' }).opened, 'a click on a road does not open a menu');
assert.ok(!pick('click', {}).opened, 'a click on bare land pans');
assert.ok(pick('context', {}).opened, 'right click on bare land still opens the build menu');

// The card says what a click does, once, for everyone, and no longer names a second gesture.
const hint = app.match(/const placeClickHint = \(touch\) => (.*);/);
assert.ok(hint, 'placeClickHint');
const text = vm.runInNewContext(`(touch) => ${hint[1]}`);
assert.equal(text(false), 'Click for what you can do here');
assert.equal(text(true), 'Tap for what you can do here');
assert.ok(!/Right-click to build outside|Press and hold to build outside/.test(app), 'no hidden second gesture on the card');
assert.ok(app.includes("clickable: (t) => BUILDING_KINDS.has(t.kind)"), 'the map says which tiles a click opens');
assert.ok(/title: name \|\| owner/.test(app), 'the place menu is headed by the card\'s name');

// Rooms: hovering says what a click does, and a right click does the same as a click.
assert.ok(app.includes('Click to open') && app.includes('Click for options'), 'room hover text');
assert.ok(/stage\.addEventListener\('contextmenu'[\s\S]*?stageActivate\(event\)/.test(app), 'room right click = click');
assert.ok(/stage\.title = itemHint/.test(app), 'room hover sets its text');
console.log('W102: hover names the click, click and right click open the one menu on the map and in rooms.');
