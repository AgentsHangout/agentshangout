// W117, I152: an open map or room leaves the graphics to everything else. Run with the shipped
// web/world.js and web/room.js.
//   node tests/frame_pace_ui.mjs
// A second of a 144 Hz screen goes by on a fake clock, and the test counts the frames each one draws:
// at most 30 while idling, 8 while the window isn't in use, and every one while a pan or a placing
// needs them. A change to the map draws on the next frame, without waiting.
import { mkdtempSync, readdirSync, copyFileSync, writeFileSync } from 'node:fs';
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
goTo('http://localhost/world/commons');

const web = fileURLToPath(new URL('../web/', import.meta.url));
const dir = mkdtempSync(join(tmpdir(), 'pace-'));
writeFileSync(join(dir, 'package.json'), '{"type":"module"}');
for (const file of readdirSync(web)) if (file.endsWith('.js')) copyFileSync(join(web, file), join(dir, file));
const { World } = await import(pathToFileURL(join(dir, 'world.js')).href);
const { Room } = await import(pathToFileURL(join(dir, 'room.js')).href);

// One clock for animation frames, timeouts and performance.now.
let clock = 1000;
let frames = [];
let timeouts = [];
let focused = true;
performance.now = () => clock;
globalThis.requestAnimationFrame = (fn) => { frames.push(fn); return frames.length; };
globalThis.cancelAnimationFrame = () => {};
globalThis.setTimeout = (fn, ms) => { const t = { fn, at: clock + ms }; timeouts.push(t); return t; };
globalThis.clearTimeout = (t) => { timeouts = timeouts.filter((x) => x !== t); };
document.hasFocus = () => focused;
const VSYNC = 1000 / 144;
const run = (ms) => {
  const end = clock + ms;
  while (clock < end - 0.01) {
    clock += VSYNC;
    const due = timeouts.filter((t) => t.at <= clock);
    timeouts = timeouts.filter((t) => t.at > clock);
    for (const t of due) t.fn();
    const now = frames;
    frames = [];
    for (const fn of now) fn(clock);
  }
};

// The map: ready, on screen and moving, with its drawing counted.
const world = new World(document.createElement('div'), {});
world.setCalm(false);
world.ready = true;
world.cssW = 100;
world.inView = true;
let drawn = 0;
world.render = () => { drawn++; };
world.warmUp = () => {};
world.raf = 0;
world.kick();
run(500);

drawn = 0;
run(1000);
assert.ok(drawn <= 31 && drawn >= 25, `an idle map draws about 30 frames a second, not ${drawn}`);
focused = false;
run(200);
drawn = 0;
run(1000);
assert.ok(drawn <= 9 && drawn >= 6, `an idle map in a window not in use draws about 8, not ${drawn}`);
focused = true;
run(200);

world.panning = true;
world.kick();
drawn = 0;
run(1000);
assert.ok(drawn >= 140, `a pan draws every frame the screen shows, not ${drawn}`);
world.panning = false;
run(200);

// A change draws on the very next frame, without waiting out the idle gap.
run(VSYNC);
drawn = 0;
world.kick();
run(VSYNC);
assert.equal(drawn, 1, 'a change to the map draws on the next frame');
world.destroy();
assert.equal(timeouts.length, 0, 'a map that has gone waits for nothing');

// The room: its frames counted the same way.
const stage = document.createElement('div');
const room = new Room(stage);
let roomFrames = 0;
const frame = room.frame.bind(room);
room.frame = (t) => { roomFrames++; frame(t); };
run(500);
roomFrames = 0;
run(1000);
assert.ok(roomFrames <= 31 && roomFrames >= 25, `a room draws about 30 frames a second, not ${roomFrames}`);
focused = false;
run(200);
roomFrames = 0;
run(1000);
assert.ok(roomFrames <= 9 && roomFrames >= 6, `a room in a window not in use draws about 8, not ${roomFrames}`);
focused = true;
room.ghost = { table: true, ok: true, left: 10, top: 10, wide: 20, deep: 20 };
run(200);
roomFrames = 0;
run(1000);
assert.ok(roomFrames >= 140, `placing an item draws every frame, not ${roomFrames}`);
room.ghost = null;

// A tag is moved only when its bot has moved.
let moves = 0;
const bot = room.addBot({ id: 'still', name: 'Still', color: '#fff' });
bot.tag = { style: new Proxy({}, { set: (t, k, v) => { if (k === 'left' || k === 'top') moves++; t[k] = v; return true; } }), classList: { toggle() {} } };
bot.x = 100;
bot.y = 150;
bot.path = [];
room.update = () => {};
run(200);
assert.equal(moves, 2, 'a tag is placed once');
moves = 0;
run(500);
assert.equal(moves, 0, 'and left alone while its bot stands still');
bot.x = 120;
run(100);
assert.equal(moves, 2, 'and moved when its bot moves');
room.destroy();
console.log('A map and a room draw at most 30 frames a second while idling, and 8 in a window not in use.');
