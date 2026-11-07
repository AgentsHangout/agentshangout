// W116, #739: a calm map (reduced motion) still follows the clock. Run with the shipped web/world.js.
//   node tests/calm_map_ui.mjs
// A calm map draws only when something changes, so without its minute tick the light would stay at
// the hour it last drew until a zoom. Here nothing touches the map but the tick, and the next frame
// it draws is lit for the new hour.
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
// The minute timer the map starts, caught so the test can fire it.
const timers = [];
globalThis.setInterval = (fn, ms) => { timers.push({ fn, ms }); return timers.length; };
const frames = [];
globalThis.requestAnimationFrame = (fn) => { frames.push(fn); return frames.length; };

const web = fileURLToPath(new URL('../web/', import.meta.url));
const dir = mkdtempSync(join(tmpdir(), 'calm-'));
writeFileSync(join(dir, 'package.json'), '{"type":"module"}');
for (const file of readdirSync(web)) if (file.endsWith('.js')) copyFileSync(join(web, file), join(dir, file));
const { World } = await import(pathToFileURL(join(dir, 'world.js')).href);

const world = new World(document.createElement('div'), {});
const minute = timers.find((t) => t.ms === 60000);
assert.ok(minute, 'the map keeps a minute timer');
world.setCalm(true);
world.ready = true;
world.cssW = 100;
world.inView = true;
frames.length = 0;
world.raf = 0;
world.dirty = false;

// Calm and idle: nothing is scheduled, so the frame on screen would stay as it is.
world.schedule();
assert.equal(frames.length, 0, 'a calm, idle map draws nothing on its own');
// The minute passes and it's now night: the tick asks for one frame with the sky redrawn.
world.hourOverride = 23;
minute.fn();
assert.equal(frames.length, 1, 'the tick draws one frame');
assert.equal(world.skyDirty, true, 'with the sky for the hour now');
// Not while the page is hidden or the map is off screen, and not once it's gone.
frames.length = 0;
world.raf = 0;
world.inView = false;
minute.fn();
assert.equal(frames.length, 0, 'an off-screen map waits');
world.inView = true;
world.setCalm(false);
frames.length = 0;
world.raf = 0;
world.dirty = false;
const before = frames.length;
world.clockTick();
assert.equal(frames.length, before, 'a moving map already draws every frame, so the tick does nothing');
console.log('A calm map draws one frame a minute, so its light follows the clock.');
