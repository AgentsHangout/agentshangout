// Run from the repository root: node tests/game_menu_ui.cjs
// W109, #555: hover, click and right click draw one menu, with submenus, My bots and Room
// permissions, driven through the shipped functions of web/app.js with a small DOM stand-in.
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

let focused = null;
function element(tag, props = {}, ...children) {
  const node = {
    tag,
    props: props || {},
    children: children.flat(Infinity).filter((c) => c != null),
    removed: false,
    style: {},
    offsetWidth: 120,
    offsetHeight: 80,
    append(...more) { this.children.push(...more); },
    replaceChildren(...more) { this.children = more.flat(); },
    remove() { this.removed = true; },
    focus() { focused = this; },
    click() { this.props.onclick?.(); },
    getAttribute(name) { return this.props[name] ?? null; },
    setAttribute(name, value) { this.props[name] = value; },
    contains(other) { return other === this || this.children.some((c) => c && c.contains && c.contains(other)); },
  };
  return node;
}
const same = (a, b, message) => assert.equal(JSON.stringify(a), JSON.stringify(b), message);
const textOf = (node) => (node == null ? '' : typeof node === 'string' ? node : (node.children || []).map(textOf).join(' '));
const items = (node) => (node.children || []).flatMap((c) => (c && (c.tag === 'button' || c.tag === 'a') ? [c] : c && c.children ? items(c) : []));
const labels = (node) => items(node).map((b) => textOf(b).replace(/\s*[›‹]\s*/g, '').trim());
const went = [];
let cardData = new Promise(() => {});
const said = [];
const documentListeners = {};
const context = {
  h: element,
  go: (path) => went.push(path),
  setTimeout: (fn) => { fn(); return 1; },
  Date,
  api: () => cardData,
  clearTimeout() {},
  innerWidth: 1000,
  innerHeight: 800,
  encodeURIComponent,
  document: {
    get activeElement() { return focused; },
    addEventListener: (kind, fn) => { (documentListeners[kind] ||= []).push(fn); },
    removeEventListener: (kind, fn) => { documentListeners[kind] = (documentListeners[kind] || []).filter((f) => f !== fn); },
  },
};
vm.createContext(context);
vm.runInContext([
  extract('researchRepositoryLink'), extract('researchMenuLink'), extract('BOT_STATE_WORD', 'const'),
  extract('myBotsMenu'), extract('roomPermissionsMenu'), extract('popupMenu'),
  extract('githubCardRows'), extract('githubProfileUrl', 'const'), extract('personCardBody'), extract('personCards'),
  'this.popupMenu = popupMenu; this.myBotsMenu = myBotsMenu; this.roomPermissionsMenu = roomPermissionsMenu; this.personCards = personCards;',
].join('\n'), context);

const root = element('main');
let refocused = 0;
const menu = context.popupMenu(root, { refocus: () => { refocused += 1; } });
const shown = () => root.children.filter((c) => !c.removed).at(-1);
const head = (node) => node.children.find((c) => c && c.props.class === 'game-menu-head');
const press = (key) => {
  const event = { key, defaultPrevented: false, preventDefault() { this.defaultPrevented = true; } };
  shown().props.onkeydown(event);
  return event;
};

// My bots: one row per bot, saying what My bots says, then Open My bots.
let openedPanel = 0;
const bots = context.myBotsMenu([{ name: 'Rover', state: 'working' }, { name: 'Pip', where: 'In this room' }], () => { openedPanel += 1; });
assert.equal(bots.label, 'My bots');
same(bots.items.map((item) => item && [item.label, item.cost]), [['Rover', 'Working'], ['Pip', 'In this room'], null, ['Open My bots', undefined]]);
same(context.myBotsMenu([], () => {}).items.map((item) => item.label), ['Open My bots'], 'no bots, no gap');

// Room permissions: who can come in now, then the space's settings.
const perms = context.roomPermissionsMenu(false, '/s/space-1', (text) => said.push(text));
same(perms.items.map((item) => item.label), ['Private: only who you let in', 'Change who can come in']);
assert.match(context.roomPermissionsMenu(true, '/s/x', () => {}).items[0].label, /^Public/);

// One frame: a head, the rows, and a submenu marked as one.
let ran = 0;
const spec = { title: 'Ann’s cafe', sub: 'Cafe', items: [{ label: 'Go in', run: () => { ran += 1; } }, null, perms, bots] };
menu.open(spec, 40, 50, 'click');
assert.equal(shown().props.class, 'game-menu');
assert.equal(shown().props.role, 'menu');
assert.match(textOf(head(shown())), /Ann.s cafe/);
same(labels(shown()), ['Go in', 'Room permissions', 'My bots']);
assert.equal(items(shown())[2].props['aria-haspopup'], 'menu', 'a submenu says so');
assert.equal(focused, items(shown())[0], 'the first row has focus');
assert.equal(menu.isOpen, true);

// Into a submenu in place, with Back first, and out again by keyboard.
items(shown())[2].click();
same(labels(shown()), ['Back to Ann’s cafe', 'Rover Working', 'Pip In this room', 'Open My bots'], 'each row says what My bots says');
assert.match(textOf(head(shown())), /My bots/);
assert.equal(root.children.filter((c) => !c.removed).length, 1, 'one menu at a time');
assert.ok(press('Escape').defaultPrevented);
same(labels(shown()), ['Go in', 'Room permissions', 'My bots'], 'Escape in a submenu goes back');
focused = items(shown())[1];
press('ArrowRight');
same(labels(shown()), ['Back to Ann’s cafe', 'Private: only who you let in', 'Change who can come in']);
press('ArrowLeft');
same(labels(shown()), ['Go in', 'Room permissions', 'My bots'], 'ArrowLeft goes back');
items(shown())[1].click();
items(shown())[2].click();
same(went, ['/s/space-1'], 'Change who can come in opens the settings');
assert.equal(shown(), undefined, 'choosing a row closes the menu');
assert.equal(refocused, 1);

// Escape at the top closes, and a row runs once.
menu.open(spec, 40, 50, 'click');
press('Escape');
assert.equal(shown(), undefined);
menu.open(spec, 40, 50, 'click');
items(shown())[0].click();
assert.equal(ran, 1);
menu.open(spec, 40, 50, 'click');
items(shown())[2].click();
items(shown())[3].click();
assert.equal(openedPanel, 1, 'Open My bots opens the panel');

// A hover is the same frame, to read only, ending in what a click does.
menu.peek({ ...spec, body: element('div', { class: 'game-menu-body' }, 'Inside: Rover'), hint: 'Click for what you can do here' }, 10, 10);
assert.equal(shown().props.class, 'game-menu');
assert.equal(shown().props.role, 'tooltip');
assert.match(textOf(head(shown())), /Ann.s cafe/);
assert.equal(items(shown()).length, 0, 'a hover has no rows to press');
assert.match(textOf(shown()), /Inside: Rover/);
assert.match(textOf(shown()), /Click for what you can do here/);
assert.equal(menu.isPeek, true);
assert.equal(menu.isOpen, false);
const before = refocused;
menu.close(true);
assert.equal(refocused, before, 'closing a hover takes no focus');

// A plain link is an anchor, never followed by script.
menu.open({ title: 'Ann', sub: 'Person', items: [{ label: 'View on GitHub', href: 'https://github.com/ann' }] }, 0, 0, 'click');
const anchor = items(shown())[0];
assert.equal(anchor.tag, 'a');
assert.equal(anchor.props.rel, 'noopener noreferrer');
menu.close();

// A menu with nothing to press says so.
menu.open({ title: 'Me', sub: 'Person', items: [], note: 'Nothing else to do here.' }, 0, 0, 'click');
assert.match(textOf(shown()), /Nothing else to do here/);
menu.close();

// Through a person's card (#557 review): Escape in a submenu goes back, data coming in keeps focus,
// and closing returns focus to the person's row.
(async () => {
  const stageListeners = {};
  const stage = element('div');
  stage.classList = { toggle() {} };
  stage.addEventListener = (kind, fn) => { (stageListeners[kind] ||= []).push(fn); };
  const cardRoot = element('main');
  let release;
  cardData = new Promise((resolve) => { release = resolve; });
  const cards = context.personCards({
    room: { personAt: () => null },
    stage,
    root: cardRoot,
    locked: () => false,
    mention() {},
    view: () => ({ you: { person_id: 'p-me' }, people: [{ id: 'p-ann', name: 'Ann' }] }),
    extras: () => [context.myBotsMenu([{ name: 'Rover', where: 'In this room' }], () => {})],
  });
  const chip = element('span');
  chip.dataset = { person: 'p-ann' };
  const open = () => stageListeners.click[0]({
    clientX: 5,
    clientY: 5,
    target: { closest: (sel) => (sel === 'button, a' ? null : chip) },
    preventDefault() {},
    stopPropagation() {},
  });
  const card = () => cardRoot.children.filter((c) => !c.removed).at(-1);
  const key = (name) => {
    const event = { key: name, target: focused, defaultPrevented: false, preventDefault() { this.defaultPrevented = true; } };
    for (const fn of documentListeners.keydown || []) fn(event);
    if (card()) card().props.onkeydown(event);
  };

  open();
  same(labels(card()), ['Mention in chat', 'My bots']);
  focused = items(card())[1];
  release({ linked: true, login: 'ann-codes', stars: { total: 2 } });
  await new Promise((resolve) => setImmediate(resolve));
  same(labels(card()), ['Mention in chat', 'View on GitHub', 'My bots'], 'the link comes in');
  assert.equal(textOf(focused).trim(), 'View on GitHub', 'focus stays on the second row, not the first');
  focused = items(card())[2];
  key('ArrowRight');
  same(labels(card()), ['Back to Ann', 'Rover In this room', 'Open My bots']);
  key('Escape');
  assert.ok(card(), 'Escape in a submenu keeps the card');
  same(labels(card()), ['Mention in chat', 'View on GitHub', 'My bots'], 'and goes back a level');
  key('Escape');
  assert.equal(card(), undefined, 'Escape at the top closes it');
  assert.equal(focused, chip, 'and focus goes back to the person');
  cards.close();

  // Where the three meet in the page: the map's hover card, its menus, a person and the room.
assert.ok(source.includes("class: 'game-menu city-card', role: 'tooltip'"), 'the map hover card is the menu frame');
assert.ok(fs.readFileSync('web/city.js', 'utf8').includes("el('div', 'game-menu city-card')"), 'so is the city page’s');
assert.ok(/spec = \{ \.\.\.spec, items: \[\.\.\.spec\.items, null, myBotsMenu\(/.test(source), 'every map menu ends with My bots');
assert.ok(source.includes("if (details.space_id) own.push(roomPermissionsMenu("), 'Room permissions only with the owner’s space id');
assert.ok(source.includes('extras: roomBotsMenu,'), 'a pressed person card ends with My bots');
assert.ok(source.includes("place?.runs_it ? [roomPermissionsMenu("), 'the room menu has Room permissions only for whoever runs it');
assert.ok(!/function personCards[\s\S]*?h\('div', \{ class: 'game-menu person-card'/.test(source), 'a person card has no frame of its own');
console.log('Game menu: one frame for hover and press; submenus in place with Back; My bots and Room permissions reachable.');
})().catch((error) => { console.error(error); process.exitCode = 1; });
