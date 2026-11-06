// #552, L62: bots drive to and from places on the map, run with the shipped web/world.js on the real
// Commons map from a test server, not just read.
//   node tests/drive_ui.mjs <base url>
// An arriving bot drives from where it was; one leaving a place drives home to its owner's house, or
// with no house there, off the edge of town; and one with nowhere it came from drives in from the edge.
import { mkdtempSync, readdirSync, copyFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL, fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { failures, goTo } from './dom_stub.mjs';

// The stub collects errors a page throws; here any of them, a failed assertion included, fails the run.
process.on('exit', () => {
  if (failures.length) {
    console.error(failures.join('\n\n'));
    process.exitCode = 1;
  }
});
const [base] = process.argv.slice(2);
goTo(new URL('/world/commons', base).href);
const view = await (await fetch(new URL('/api/worlds/commons', base))).json();
const n = view.size;

const web = fileURLToPath(new URL('../web/', import.meta.url));
const dir = mkdtempSync(join(tmpdir(), 'drive-'));
writeFileSync(join(dir, 'package.json'), '{"type":"module"}');
for (const file of readdirSync(web)) if (file.endsWith('.js')) copyFileSync(join(web, file), join(dir, file));
const { World } = await import(pathToFileURL(join(dir, 'world.js')).href);

// Two spots beside the road, far apart and away from the edge: a place, and Ann's house.
const kindAt = new Map(view.tiles.map((t) => [`${t[0]},${t[1]}`, t[2]]));
const isRoad = (x, y) => kindAt.get(`${x},${y}`) === 'road';
const besideRoad = [];
for (const t of view.tiles) {
  if (t[2] !== 'road') continue;
  const [x, y] = t;
  if (x < 8 || y < 8 || x > n - 9 || y > n - 9) continue;
  for (const [dx, dy] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
    if (!kindAt.has(`${x + dx},${y + dy}`)) besideRoad.push({ road: [x, y], lot: [x + dx, y + dy] });
  }
}
assert.ok(besideRoad.length > 10, 'the map has roads');
const place = besideRoad[0];
const home = besideRoad.reduce((far, spot) => {
  const d = (s) => Math.abs(s.lot[0] - place.lot[0]) + Math.abs(s.lot[1] - place.lot[1]);
  return d(spot) > d(far) ? spot : far;
});
const tiles = [...view.tiles, [place.lot[0], place.lot[1], 'cafe', 0], [home.lot[0], home.lot[1], 'house', 0]];
const owners = [[home.lot[0], home.lot[1], 'Ann', "Ann's House"], [place.lot[0], place.lot[1], 'Cy', "Cy's Cafe"]];
const at = (name, owner, from) => [name, place.road[0], place.road[1], {}, from?.[0] ?? null, from?.[1] ?? null, null, owner, place.lot];
const withBots = (bots_out) => ({ ...view, tiles, owners, bots_out });

const run = (world, seconds) => {
  for (let i = 0; i < seconds * 20; i++) {
    world.time += 0.05;
    world.updatePlayers(0.05);
  }
};
const drove = (p) => p.path.some((step) => step[2]);

// An arrival from Ann's house drives, and gets to the place.
const world = new World(document.createElement('div'), {});
world.setView(withBots([]));
world.setView(withBots([at('Scout', 'Ann', home.road)]));
const scout = world.visitors.get('Scout');
assert.ok(scout && drove(scout) && scout.ride, `an arrival drives: ${JSON.stringify(scout?.path?.length)}`);
run(world, 120);
assert.equal(scout.path.length, 0, 'it got there');

// Leaving the place, it drives home to Ann's house, and is gone once it's there.
world.setView(withBots([]));
assert.ok(scout.homeward && drove(scout), 'it drives home');
const last = scout.path.at(-1);
assert.ok(Math.abs(last[0] - 0.5 - scout.ox - home.lot[0]) < 0.01 && Math.abs(last[1] - 0.5 - scout.oy - home.lot[1]) < 0.01, 'to her house');
assert.ok(!scout.leaving, 'not gone yet');
run(world, 120);
assert.ok(!world.visitors.has('Scout'), 'home, and gone');

// With no house on the map, it drives off the edge of town.
world.setView(withBots([at('Rover', 'Bob', home.road)]));
run(world, 120);
world.setView(withBots([]));
const rover = world.visitors.get('Rover');
assert.ok(rover.homeward, 'it drives off');
const end = rover.path.at(-1);
const [ex, ey] = [Math.round(end[0] - 0.5 - rover.ox), Math.round(end[1] - 0.5 - rover.oy)];
assert.ok(ex === 0 || ey === 0 || ex === n - 1 || ey === n - 1, `to the edge: ${ex},${ey}`);

// With nowhere it came from, it drives in from the edge instead of appearing.
world.setView(withBots([at('Pip', 'Dee', null)]));
const pip = world.visitors.get('Pip');
const [sx, sy] = [Math.round(pip.px - 0.5 - pip.ox), Math.round(pip.py - 0.5 - pip.oy)];
assert.ok(sx === 0 || sy === 0 || sx === n - 1 || sy === n - 1, `from the edge: ${sx},${sy}`);
assert.ok(drove(pip), 'and drives');

// Back before it got home: it turns round and goes back.
world.setView(withBots([at('Pip', 'Dee', null)]));
run(world, 120);
world.setView(withBots([]));
assert.ok(pip.homeward);
run(world, 3);
world.setView(withBots([at('Pip', 'Dee', null)]));
assert.ok(!pip.homeward && pip.path.length > 0, 'it turns round');

rmSync(dir, { recursive: true, force: true });
if (!failures.length) console.log('Driving: arrivals drive in, leavers drive home or off the edge (run, not just read).');
process.exit(failures.length ? 1 : 0);
