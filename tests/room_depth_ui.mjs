// #807: the room's draw order and solid cells, from the shipped web/room.js.
//   node tests/room_depth_ui.mjs
// Furniture sorts by its front edge (the bottom of the rect it's drawn in), as bots sort by their feet,
// so a bot just above a front edge is drawn behind that piece and just below it, in front. The desk and
// table cells that bots can't walk through are the rects they're drawn in.
import assert from 'node:assert/strict';
import { pathToFileURL, fileURLToPath } from 'node:url';

const { FURNITURE, fillGrid, clear, furnitureScene } = await import(pathToFileURL(fileURLToPath(new URL('../web/room.js', import.meta.url))).href);

const rect = (word) => FURNITURE.find(([each]) => each === word).slice(1);
const inRect = (x, y, [left, top, wide, deep]) => x >= left && x < left + wide && y >= top && y < top + deep;

// Where a piece of furniture and a bot standing at botY come out after the same sort the room does.
function drawnFirst(scene, botY) {
  const entries = [...scene.map((entry, index) => ({ ...entry, index })), { y: botY, index: 'bot' }];
  entries.sort((a, b) => a.y - b.y);
  return entries.findIndex((entry) => entry.index === 'bot') < entries.findIndex((entry) => entry.index === 0);
}

// The desk is first in the scene and the table second, both with a full room.
const room = furnitureScene(null, [], 0, [], [], null);
const desk = room[0];
const table = room[1];
assert.equal(desk.y, rect('desk')[1] + rect('desk')[3], 'the desk sorts by its front edge');
assert.equal(table.y, rect('table')[1] + rect('table')[3], 'the table sorts by its front edge, not a fixed 150');

// Desk front edge is 136: a bot above it is drawn behind the desk, and one below it in front.
const deskFront = desk.y;
const deskBefore = (botY) => drawnFirst(room.slice(0, 1), botY);
assert.equal(deskBefore(deskFront - 2), true, 'a bot just above the desk front edge is drawn behind the desk');
assert.equal(deskBefore(deskFront + 2), false, 'a bot just below the desk front edge is drawn in front of the desk');

// Table front edge is 172: same on each side of it, and it moves with the table.
const tableBefore = (botY, scene = room) => drawnFirst(scene.slice(1, 2), botY);
assert.equal(tableBefore(table.y - 2), true, 'a bot just above the table front edge is drawn behind the table');
assert.equal(tableBefore(table.y + 2), false, 'a bot just below the table front edge is drawn in front of the table');
const moved = furnitureScene(null, [], 0, [], [], { x: 12, y: 3 });
assert.equal(moved[1].y, table.y + 24, 'a table moved two floor cells down sorts two floor cells lower');
assert.equal(tableBefore(moved[1].y - 2, moved), true);
assert.equal(tableBefore(moved[1].y + 2, moved), false);

// The solid cells are the rects the desk and table are drawn in, for the room as it stands and when the
// table has moved. Everything else is hidden so only those two block; walls are left out of the check.
const solidMatchesDrawn = (shift, drawnTable) => {
  fillGrid(['bar', 'arcade', 'plants', 'couch'], [], shift);
  for (let x = 6; x < 474; x += 6) { // cell columns from the grid's own size
    for (let y = 78; y < 246; y += 6) {
      const cx = x + 3;
      const cy = y + 3;
      const drawnSolid = inRect(cx, cy, rect('desk')) || inRect(cx, cy, drawnTable);
      assert.equal(!clear(cx, cy), drawnSolid, `cell at ${cx},${cy} ${drawnSolid ? 'is drawn as furniture but is walkable' : 'is solid but not drawn'}`);
    }
  }
};
solidMatchesDrawn([0, 0], rect('table'));
solidMatchesDrawn([24, 24], rect('table').map((v, i) => (i < 2 ? v + 24 : v)));

// Put back as the room is when nothing is moved.
fillGrid([]);
console.log('room depth and solid cells: ok');
