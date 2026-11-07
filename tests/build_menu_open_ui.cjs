// node tests/build_menu_open_ui.cjs: #735, opening the build menu does the least work it can. Runs the
// shipped createPanels from web/app.js and itemPicture from web/room.js on a small fake DOM.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');

// 1. Opening a window lays the page out once, after everything that opening changes.
let passes = 0, dockReads = 0;
class Element {
  constructor(tag, props = {}, children = []) {
    this.tag = tag; Object.assign(this, props); this.children = children.flat().filter(Boolean);
    this.style = { setProperty(k, v) { this[k] = v; } }; this.dataset = {}; this.offsetParent = {};
    this.classList = { add() {}, remove() {}, toggle() {}, contains() { return false; } };
  }
  append(...nodes) { this.children.push(...nodes); }
  replaceChildren(...nodes) { this.children = nodes; }
  setAttribute(k, v) { this[k] = v; }
  addEventListener() {}
  // layout() looks for the room's well once each time it runs.
  querySelector(selector) { if (selector === '.wd-well') passes += 1; return null; }
  querySelectorAll() { return []; }
  getBoundingClientRect() {
    if (this.class === 'window-dock') dockReads += 1;
    return { left: 0, top: 0, right: 1400, bottom: 900, width: 1400, height: 900 };
  }
  focus() {}
}
const h = (tag, props, ...children) => new Element(tag, props, children);
const root = h('div', {});
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
const start = app.indexOf('function createPanels(root) {');
const end = app.indexOf('// What goes in the panels', start);
assert.ok(start >= 0 && end > start, 'createPanels is in web/app.js');
const context = vm.createContext({ h, document: { activeElement: null }, navigator: {}, installSheetDrag() {},
  ResizeObserver: class { observe() {} disconnect() {} }, getComputedStyle: () => ({ display: 'block' }),
  localStorage: { getItem: () => null, setItem() {} }, Math });
vm.runInContext(`${app.slice(start, end)}\nthis.createPanels = createPanels;`, context);
const panels = context.createPanels(root);
let opened = 0, passesAtChange = null;
panels.add('build', 'Build', { onOpen: () => { opened += 1; } }, h('div', { class: 'build-catalog' }));
panels.add('room', 'Chat', {}, h('p', {}, 'Chat'));
// The room's handler shows the build dock, which the layout has to make room for.
panels.onChange = () => { passesAtChange = passes; };

passes = 0; dockReads = 0;
panels.open('build');
assert.equal(opened, 1, 'opening Build still loads the catalog');
assert.equal(panels.current, 'build');
assert.equal(root.children.find((n) => n.id === 'panel-build').hidden, false, 'and shows it');
assert.equal(passes, 1, `opening Build lays the windows out once, not ${passes} times`);
assert.equal(passesAtChange, 0, 'the one layout comes after the room has reacted to the change');
assert.equal(dockReads, 0, 'a hidden dock of minimized windows is not measured');

// Opening another, and coming back, is one pass each too.
panels.close(false);
passes = 0;
panels.open('room');
assert.equal(passes, 1, 'any window opens with one layout');
passes = 0;
panels.open('build');
assert.equal(passes, 1);
assert.equal(opened, 2, 'reopening a closed Build refreshes it, as before');

// With a minimized window the dock shows, and its size still counts.
panels.minimize('room');
dockReads = 0;
panels.open('build');
assert.ok(dockReads > 0, 'a dock that is showing is measured');

// 2. Item pictures, which fitPicture reads pixel by pixel, are drawn on a canvas meant to be read.
const room = fs.readFileSync('web/room.js', 'utf8').replace(/\r\n/g, '\n');
const pictureStart = room.indexOf('export function itemPicture(');
assert.ok(pictureStart >= 0, 'itemPicture is in web/room.js');
const picture = room.slice(pictureStart, room.indexOf('\n}\n', pictureStart) + 2).replace('export ', '');
const asked = [];
const pictureContext = vm.createContext({
  FLOOR_CELL: 12, SIGN_WIDE: 46,
  ITEMS: { lamp: { rise: 20, paint: () => {} } },
  document: { createElement: () => ({ width: 0, height: 0, getContext: (kind, options) => { asked.push([kind, options]); return {}; } }) },
});
vm.runInContext(`${picture}\nthis.itemPicture = itemPicture;`, pictureContext);
assert.ok(pictureContext.itemPicture('lamp', 1, 1), 'a known item gives a picture');
assert.equal(asked.length, 1);
assert.equal(asked[0][0], '2d');
assert.equal(asked[0][1]?.willReadFrequently, true, 'the item picture is kept where reading it back is cheap');
assert.equal(pictureContext.itemPicture('no_such_thing', 1, 1), null);

console.log('Build menu: one layout per open, no hidden dock measured, item pictures cheap to read.');
