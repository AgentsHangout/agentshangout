// node tests/outside_night_ui.cjs: #567, a building's outside view opens as the map looks: at
// night after dark and by day in daylight, through an evening in the viewer's own time. Runs the
// shipped nightOnMap on the map's own sky (skyAt, skyFor), not just their text.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const read = (file) => fs.readFileSync(file, 'utf8').replace(/\r\n/g, '\n');
const world = read('web/world.js'), room = read('web/room.js'), outside = read('web/outside.js');
const slice = (source, from, end) => {
  const start = source.indexOf(from);
  assert.ok(start >= 0, from);
  return source.slice(start, source.indexOf(end, start) + end.length);
};
const code = [
  slice(room, 'export function skyFor(hour) {', '\n}\n').replace('export ', ''),
  // The sky's colours don't decide night, only its dim, so mixing them is left out.
  'const mixHex = (a) => a;',
  slice(world, 'const lerp = ', ';\n'),
  slice(world, 'function skyAt(hour) {', '\n}\n'),
  slice(outside, 'export function nightOnMap(sky) {', '\n}\n').replace('export ', ''),
].join('\n');
const context = {};
vm.createContext(context);
vm.runInContext(`${code}\nthis.skyAt = skyAt; this.nightOnMap = nightOnMap;`, context);
const { skyAt, nightOnMap } = context;

// Through a whole day, every ten minutes: day from morning to early evening, night late, and it only
// changes twice, at dusk and at dawn, never back and forth.
const at = (hour) => nightOnMap(skyAt(hour));
assert.equal(at(12), false, 'noon is day');
assert.equal(at(23), true, '11 PM is night');
assert.equal(at(2), true, '2 AM is night');
assert.equal(at(9), false, '9 AM is day');
let flips = 0;
for (let m = 10; m < 24 * 60; m += 10) if (at(m / 60) !== at((m - 10) / 60)) flips++;
assert.equal(flips, 2, 'one dusk and one dawn');
// Before the map has drawn a sky, it opens by day as before.
assert.equal(nightOnMap(undefined), false);
assert.equal(nightOnMap({}), false);

// And the view opens on it, with the button saying which way it would turn.
assert.ok(outside.includes('night = nightOnMap(world.sky);'));
assert.ok(outside.includes("nightButton.textContent = night ? 'See it by day' : 'See it at night';"));
console.log('Outside view: opens at night after dark, by day in daylight (run, not just read).');
