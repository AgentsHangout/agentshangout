// W122, #1183: the map's place search. Run with the shipped web/world.js.
//   node tests/place_search_ui.mjs
// It lists the places on The Commons whose names match what's typed, from the view's owners list,
// and picking one moves the view to it. Only names are searched: never an owner, a bot or a person.
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
globalThis.setInterval = () => 1;
globalThis.requestAnimationFrame = () => 1;

const web = fileURLToPath(new URL('../web/', import.meta.url));
const dir = mkdtempSync(join(tmpdir(), 'place-search-'));
writeFileSync(join(dir, 'package.json'), '{"type":"module"}');
for (const file of readdirSync(web)) if (file.endsWith('.js')) copyFileSync(join(web, file), join(dir, file));
const { World, placeMatches, placeLabel } = await import(pathToFileURL(join(dir, 'world.js')).href);

// The owners list as the server sends it: [x, y, owner, name]. Owners are people's names.
const owners = [
  [10, 12, 'Ann Lee', "Ann's Café"],
  [4, 8, 'Bo Park', 'Bo Park'],
  [20, 3, 'Cy Ray', 'Green Corner'],
  [7, 7, 'Dee', "Dee's Park"],
  [30, 30, 'Eve', 'Cafe Nero'],
  [2, 2, 'Town Square', 'Town Square'],
];
const names = (rows) => rows.map((h) => h.name);

// Typed text finds names, starts-with first.
assert.deepEqual(placeMatches(owners, 'caf').map((h) => [h.x, h.y]), [[30, 30], [10, 12]], 'starts-with names come first');
assert.deepEqual(names(placeMatches(owners, 'nero')), ['Cafe Nero']);
assert.deepEqual(names(placeMatches(owners, 'square')), ['Town Square']);
assert.deepEqual(names(placeMatches(owners, 'park')).sort(), ['Bo Park', 'Park']);
assert.deepEqual(placeMatches(owners, '  '), [], 'an empty box finds nothing');
assert.deepEqual(placeMatches(owners, 'a'.repeat(200)), [], 'a long query is cut and finds nothing');

// Owners are never searched: a person's name that is in no place's name finds nothing.
assert.deepEqual(placeMatches(owners, 'eve'), [], 'an owner\'s name finds nothing');
assert.deepEqual(placeMatches(owners, 'cy ray'), [], 'an owner\'s name finds nothing, whatever the case');
assert.deepEqual(placeMatches(owners, 'dee'), [], 'an unnamed place does not carry its owner\'s name');

// A place nobody named is called after its owner and kind: only the kind is searched.
assert.equal(placeLabel('Ann Lee', "Ann Lee's Café"), 'Café');
assert.equal(placeLabel('Ann Lee', "Ann's Garden"), "Ann's Garden", 'a name the owner typed stays whole');
assert.equal(placeLabel('Ann Lee', 42), '', 'a name that is not text is not searched');
assert.deepEqual(placeMatches([[1, 1, 'Ann Lee', "Ann Lee's Café"]], 'lee'), [], 'the owner\'s part is never matched');
assert.equal(placeMatches([[1, 1, 'Ann Lee', "Ann Lee's Café"]], 'café')[0].name, 'Café');
assert.deepEqual(placeMatches([['x', 1, 'a', 'Cafe']], 'cafe'), [], 'a bad tile is skipped');
// #1189: an unnamed place of a 60-character owner is "<owner cut to 51>'s Café" on the server.
const longOwner = `${'A'.repeat(51)}BCDEFGHIJ`;
const longCut = 'A'.repeat(51);
assert.equal(placeLabel(longOwner, `${longCut}'s Café`), 'Café', 'a long owner\'s unnamed place is called by its kind');
assert.equal(placeLabel(longOwner, `${longCut}'s Garden`), `${longCut}'s Garden`, 'a long owner\'s typed name stays whole');
assert.deepEqual(placeMatches([[1, 1, longOwner, `${longCut}'s Café`]], 'BCDEF'), [], 'the cut-off owner tail is never matched');
assert.equal(placeMatches([[1, 1, longOwner, `${longCut}'s Café`]], 'café')[0].name, 'Café');

// The box on the page: typing fills the list, and a pick moves the view to the place.
const world = new World(document.createElement('div'), { search: true });
assert.ok(world.search, 'the search box is built when asked for');
assert.equal(new World(document.createElement('div'), {}).search, undefined, 'no search box unless asked for');
world.n = 64; // the size the view sets on its first draw
world.ownerRows = owners;
world.ready = true;
world.cssW = 100;
world.setCalm(true);
world.search.input.value = 'caf';
world.renderSearch();
assert.equal(world.search.list.children.length, 2, 'two matches listed');
assert.equal(world.search.list.hidden, false, 'the list shows');
assert.equal(world.search.list.children[0].textContent, 'Cafe Nero');
world.goToPlace(world.search.hits[1]);
assert.deepEqual([world.pulse.x, world.pulse.y], [10, 12], 'the view moves to the place and highlights it');
assert.equal(world.search.list.hidden, true, 'the list closes after a pick');
world.search.input.value = 'nothing like this';
world.renderSearch();
assert.equal(world.search.list.hidden, true, 'no match, no list');
// W122, #1186: Up and Down move the highlight through the matches, wrapping at the ends.
const key = (k) => world.searchKey({ key: k, preventDefault() {}, stopPropagation() {} });
world.search.input.value = 'caf';
world.renderSearch();
assert.equal(world.search.active, -1, 'nothing highlighted until an arrow is pressed');
key('ArrowDown');
assert.equal(world.search.active, 0, 'Down highlights the first match');
assert.equal(world.search.list.children[0].getAttribute('aria-selected'), 'true');
assert.equal(world.search.input.getAttribute('aria-activedescendant'), world.search.list.children[0].id);
key('ArrowDown');
assert.equal(world.search.active, 1, 'Down moves to the next match');
assert.equal(world.search.list.children[1].getAttribute('aria-selected'), 'true');
assert.equal(world.search.list.children[0].getAttribute('aria-selected'), 'false');
key('ArrowDown');
assert.equal(world.search.active, 0, 'Down past the last match wraps to the first');
key('ArrowUp');
assert.equal(world.search.active, 1, 'Up from the first match wraps to the last');
key('Escape');
world.search.input.value = 'caf';
world.renderSearch();
key('ArrowUp');
assert.equal(world.search.active, 1, 'Up with nothing highlighted lands on the last match');
key('ArrowDown');
assert.equal(world.search.active, 0, 'Down with nothing highlighted lands on the first match');
key('ArrowDown');
assert.equal(world.search.active, 1);

// Enter goes to the highlighted match, not always the first one.
world.pulse.x = world.pulse.y = -1;
key('Enter');
assert.deepEqual([world.pulse.x, world.pulse.y], [10, 12], 'Enter goes to the highlighted place');
assert.equal(world.search.list.hidden, true, 'the list closes after Enter');
assert.equal(world.search.input.getAttribute('aria-expanded'), 'false');

// Escape closes the list and keeps the text. With the list shut, arrows and Enter do nothing.
world.search.input.value = 'caf';
world.renderSearch();
key('ArrowDown');
key('Escape');
assert.equal(world.search.list.hidden, true, 'Escape closes the list');
assert.equal(world.search.input.value, 'caf', 'Escape keeps the text');
assert.equal(world.search.active, -1);
key('ArrowDown');
assert.equal(world.search.active, -1, 'arrows do nothing with the list closed');
world.pulse.x = world.pulse.y = -1;
key('Enter');
assert.deepEqual([world.pulse.x, world.pulse.y], [-1, -1], 'Enter with no list goes nowhere');
console.log('The place search lists matching names and moves the view to the pick.');
