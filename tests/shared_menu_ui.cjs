// Run from the repository root: node tests/shared_menu_ui.cjs
// #1005: the core menu entries (Town square, Research labs, My bots, My friends, Places) come in one
// order on the map and in a room, and both views build them through the one shared function.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const source = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');

function extract(name, keyword = 'function') {
  const start = source.indexOf(`${keyword} ${name}`);
  assert.ok(start >= 0, `missing shipped ${name}`);
  if (keyword === 'const') return source.slice(start, source.indexOf('\n', start) + 1);
  return source.slice(start, source.indexOf('\n}\n', start) + 3);
}

const context = {};
vm.createContext(context);
vm.runInContext([
  extract('CORE_MENU_ORDER', 'const'),
  extract('coreMenuButtons'),
  'this.CORE_MENU_ORDER = CORE_MENU_ORDER; this.coreMenuButtons = coreMenuButtons;',
].join('\n'), context);

const { CORE_MENU_ORDER, coreMenuButtons } = context;
const same = (a, b, message) => assert.deepEqual(JSON.parse(JSON.stringify(a)), JSON.parse(JSON.stringify(b)), message);

// The order every view must show.
same([...CORE_MENU_ORDER], ['square', 'research', 'bots', 'friends', 'places'], 'core order');

// A stand-in button for each entry, labelled by its id.
const button = (id) => ({ tag: 'button', id });
const everything = Object.fromEntries(CORE_MENU_ORDER.map((id) => [id, button(id)]));
const ids = (list) => list.map((b) => b.id);

// The map and a room give the same entries in the same order, however they were passed in.
const mapControls = { places: button('places'), friends: button('friends'), bots: button('bots'), research: button('research'), square: button('square') };
const roomControls = { ...everything };
same(ids(coreMenuButtons(mapControls)), CORE_MENU_ORDER, 'map shows the core list in order');
same(ids(coreMenuButtons(roomControls)), CORE_MENU_ORDER, 'room shows the core list in order');
same(ids(coreMenuButtons(mapControls)), ids(coreMenuButtons(roomControls)), 'map and room agree');

// Someone who can't have an entry (a guest's My friends, no research lab) just doesn't get it; the rest keep their order.
const guest = { ...everything, friends: null, bots: null, research: null };
same(ids(coreMenuButtons(guest)), ['square', 'places'], 'guest keeps the rest in order');
same(ids(coreMenuButtons({})), [], 'nothing given, nothing shown');

// Both bars and both More panels build from the shared function, not their own copies.
const mapBar = source.indexOf('frame.actions.append(...[\n    guide?.toggle,\n    ...coreMenuButtons(');
const roomBar = source.indexOf('frame.actions.append(\n    guide.toggle,\n    ...coreMenuButtons(');
assert.ok(mapBar > 0, 'map bar uses coreMenuButtons');
assert.ok(roomBar > 0, 'room bar uses coreMenuButtons');
assert.equal(source.split('coreMenuButtons({').length - 1, 4, 'map bar, map More, room bar and room More each build from it');

console.log('shared menu contract: ok');
