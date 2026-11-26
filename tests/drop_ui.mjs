// Thanksgiving Drop Party: hats, turkeys and food only in the 7 PM–7 AM CT window.
//   node tests/drop_ui.mjs
import { mkdtempSync, readdirSync, copyFileSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL, fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { failures, goTo } from './dom_stub.mjs';

process.on('exit', () => {
  if (failures.length) {
    console.error(failures.join('\n\n'));
    process.exitCode = 1;
  }
});
goTo('http://localhost/h/fairground');

const web = fileURLToPath(new URL('../web/', import.meta.url));
const dir = mkdtempSync(join(tmpdir(), 'drop-'));
writeFileSync(join(dir, 'package.json'), '{"type":"module"}');
for (const file of readdirSync(web)) if (file.endsWith('.js')) copyFileSync(join(web, file), join(dir, file));
const drop = await import(pathToFileURL(join(dir, 'room.js')).href);

const {
  DROP_START_MS, DROP_END_MS, DROP_BANNER, DROP_ROOM_TURKEYS, DROP_MAP_TURKEYS, DROP_FOOD_MAX,
  DROP_FOODS, DROP_FAIRGROUND, dropActive, dropHat, dropRoomTurkeys, dropFoods, dropMapFlock,
  stepDropTurkeys, stepDropMapFlock,
} = drop;

const BEFORE = DROP_START_MS - 1;
const START = DROP_START_MS;
const MID = DROP_START_MS + 60 * 60 * 1000;
const LAST = DROP_END_MS - 1;
const AFTER = DROP_END_MS;

assert.equal(DROP_START_MS, Date.UTC(2026, 10, 27, 1, 0, 0), 'window starts 7 PM CT 26 Nov 2026');
assert.equal(DROP_END_MS, Date.UTC(2026, 10, 27, 13, 0, 0), 'window ends 7 AM CT 27 Nov 2026');
assert.ok(DROP_BANNER.includes('Thanksgiving') && DROP_BANNER.includes('Fairground'));

assert.equal(dropActive(BEFORE), false, 'before 7 PM CT the drop is off');
assert.equal(dropActive(START), true, 'at 7 PM CT the drop is on');
assert.equal(dropActive(MID), true, 'overnight the drop is on');
assert.equal(dropActive(LAST), true, 'just before 7 AM CT the drop is on');
assert.equal(dropActive(AFTER), false, 'at 7 AM CT the drop is off');

assert.equal(dropHat('none', BEFORE), 'none');
assert.equal(dropHat('cap', BEFORE), 'cap');
assert.equal(dropHat('none', START), 'turkey', 'every bot wears a turkey hat in the window');
assert.equal(dropHat('cap', MID), 'turkey', 'a chosen hat is covered for the party');
assert.equal(dropHat(undefined, LAST), 'turkey');
assert.equal(dropHat('none', AFTER), 'none');
assert.equal(dropHat('cap', AFTER), 'cap', 'after the window the chosen hat is back');

assert.deepEqual(dropRoomTurkeys('room', BEFORE), []);
assert.equal(dropRoomTurkeys('room', START).length, DROP_ROOM_TURKEYS);
assert.equal(dropRoomTurkeys('room', AFTER).length, 0);
const birds = dropRoomTurkeys('fair', MID);
assert.equal(birds.length, DROP_ROOM_TURKEYS);
const still = birds.map((bird) => ({ ...bird }));
stepDropTurkeys(birds, 0);
assert.deepEqual(birds, still, 'a zero step leaves turkeys where they were');
stepDropTurkeys(birds, 0.5);
assert.ok(birds.some((bird, i) => bird.x !== still[i].x || bird.y !== still[i].y), 'turkeys wander');

assert.deepEqual(dropFoods(BEFORE, 'room'), []);
assert.equal(dropFoods(START, 'room').length, 1);
assert.equal(dropFoods(START + DROP_FOOD_MAX * 30_000, 'room').length, DROP_FOOD_MAX);
assert.deepEqual(dropFoods(AFTER, 'room'), []);
const kinds = new Set(dropFoods(START + 9 * 30_000, 'feast').map((food) => food.kind));
for (const kind of ['pumpkin_pie', 'apple_basket', 'cornucopia', 'cider_barrel', 'stuffing', 'cranberry']) {
  assert.ok(DROP_FOODS.includes(kind), `${kind} is not in the drop`);
}
assert.ok(kinds.has('pumpkin_pie'), 'pie drops');
assert.ok(kinds.has('stuffing') || kinds.has('cranberry') || kinds.size >= 4, 'the feast has more than pie');

assert.deepEqual(dropMapFlock(BEFORE), []);
assert.equal(dropMapFlock(START).length, DROP_MAP_TURKEYS);
assert.equal(dropMapFlock(AFTER).length, 0);
const flock = dropMapFlock(MID, 128);
const nearFair = flock.filter((bird) => {
  const dx = bird.px - DROP_FAIRGROUND.x;
  const dy = bird.py - DROP_FAIRGROUND.y;
  return dx * dx + dy * dy <= 49;
});
assert.ok(nearFair.length >= 8, `Commons Fairground needs a flock, had ${nearFair.length}`);
const moved = flock.map((bird) => ({ ...bird }));
stepDropMapFlock(flock, 0.8, 128);
assert.ok(flock.some((bird, i) => bird.px !== moved[i].px || bird.py !== moved[i].py), 'the map flock wanders');

const roomSrc = readFileSync(join(web, 'room.js'), 'utf8');
const worldSrc = readFileSync(join(web, 'world.js'), 'utf8');
const appSrc = readFileSync(join(web, 'app.js'), 'utf8');
const looksSrc = readFileSync(fileURLToPath(new URL('../src/looks.rs', import.meta.url)), 'utf8');

assert.ok(looksSrc.includes('word("turkey", "Turkey hat")'), 'the wardrobe has a turkey hat');
assert.ok(/^\s*turkey:\s*\(g, x, top, half\)/m.test(roomSrc), 'the room draws the turkey hat');
assert.ok(/^\s*turkey:\s*\(g, cx, hy, hw\)/m.test(worldSrc), 'the map draws the turkey hat');
assert.ok(roomSrc.includes('const hat = dropActive() ? HATS.turkey : bot.wears.hat'), 'bots wear it in the window');
assert.ok(worldSrc.includes('const hat = dropHat(o.hat)'), 'map bots wear it in the window');
assert.ok(roomSrc.includes('paintTurkey(g, Math.round(bird.x), Math.round(bird.y), still)'), 'rooms paint wandering turkeys');
assert.ok(roomSrc.includes('stuffing: paintStuffing') && roomSrc.includes('cranberry: paintCranberry'), 'rooms paint the feast');
assert.ok(worldSrc.includes("case 'drop_turkey'") && worldSrc.includes('dropMapFlock'), 'the map paints the flock');
assert.ok(appSrc.includes('DROP_BANNER') && roomSrc.includes('drop-banner'), 'a banner names the party');
assert.ok(roomSrc.includes('prefers-reduced-motion') && roomSrc.includes('this.calm'), 'less motion still holds');

console.log('Drop: hats, turkeys and food only between 7 PM CT and 7 AM CT.');
