// node tests/outside_floors_ui.mjs: #1122, the outside builder's floors go 1, then 2, then 1, and Undo
// walks them back one step at a time (E25: only the server's word counts). Runs the shipped
// outsideBuilder from web/outside.js with a stub page and a stub server that keeps the document,
// reads floors with the shipped bodyOf, and sends every change through the shipped save/undo path.
import assert from 'node:assert/strict';
import { bodyOf } from '../web/outside-spec.js';

// A page element: whatever isn't set reads as a no-op, so the builder's drawing does nothing here.
const noop = () => {};
// Anything a canvas is asked for answers with another stub, so gradients and paths can be called.
const anyThing = () => new Proxy(function () {}, { get: (t, k) => (k === Symbol.toPrimitive ? () => '' : anyThing()), set: () => true, apply: () => anyThing() });
const canvasCtx = anyThing();
const made = [];
function h(tag, props = {}, ...children) {
  const el = {
    tag,
    props,
    children: children.flat(Infinity).filter((c) => c !== null && c !== undefined && c !== false),
    style: {},
    classList: { add: noop, remove: noop, toggle: noop, contains: () => false },
    hidden: false,
    disabled: false,
    textContent: '',
    append: noop,
    prepend: noop,
    contains: () => false,
    closest: () => null,
    querySelector: () => null,
    querySelectorAll: () => [],
    replaceWith: noop,
    before: noop,
    after: noop,
    appendChild: noop,
    insertBefore: noop,
    replaceChildren: noop,
    remove: noop,
    focus: noop,
    blur: noop,
    setAttribute: noop,
    removeAttribute: noop,
    addEventListener: noop,
    removeEventListener: noop,
    getContext: () => canvasCtx,
    getBoundingClientRect: () => ({ left: 0, top: 0, width: 0, height: 0 }),
    parentElement: null,
    ...props,
  };
  made.push(el);
  return el;
}
const text = (el) => (el.children || []).map((c) => (typeof c === 'string' ? c : text(c))).join('');
const findTile = (name) => [...made].reverse().find((el) => el.props?.name === name && el.props?.onclick);
const findButton = (label) => [...made].reverse().find((el) => el.props?.onclick && el.props?.class?.includes('wd-btn') && text(el) === label);

// The stub server: one document per tile, with the same shape the site keeps.
const kind = 'house';
let stored = {};
const writes = [];
const catalog = { groups: [{ group: 'size', label: 'Size' }], parts: [], templates: [], packs: [], yard: [], pets: { species: [] }, limits: { items: 40, pets: 6 } };
const api = async (url, opts = {}) => {
  if (url === '/api/outside') return catalog;
  if (opts.method === 'PUT') {
    writes.push(opts.body);
    stored = JSON.parse(JSON.stringify(opts.body));
    return { outside: JSON.parse(JSON.stringify(stored)) };
  }
  return { x: 3, y: 4, kind, outside: JSON.parse(JSON.stringify(stored)), yours: true, can_edit: true, floors: bodyOf(kind, stored).floors };
};
const floorsOnServer = () => bodyOf(kind, stored).floors;

globalThis.window = { devicePixelRatio: 1, addEventListener: noop, removeEventListener: noop };
globalThis.document = { activeElement: null, addEventListener: noop, removeEventListener: noop, createElement: (t) => h(t) };
globalThis.matchMedia = () => ({ matches: false, addEventListener: noop, removeEventListener: noop });
globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} };
globalThis.requestAnimationFrame = (fn) => setTimeout(fn, 0);
globalThis.cancelAnimationFrame = noop;
globalThis.localStorage = { getItem: () => null, setItem: noop, removeItem: noop };

const { outsideBuilder } = await import('../web/outside.js');

const panel = h('div', {});
const builder = outsideBuilder({
  h,
  api,
  modal: () => ({ close: noop }),
  popupMenu: () => ({ isOpen: false, open: noop, close: noop }),
  formError: () => h('div', {}),
  selectedBuildCollection: () => null,
  collectionOffers: () => [],
  buildCollectionPicker: () => h('div', {}),
  frame: { host: { parentElement: h('div', {}) }, root: h('div', {}), actions: h('div', {}) },
  world: { placeLook: () => ({ door: 'L', variant: 0, pal: 0 }), sky: undefined },
  panels: { open: noop, close: noop, add: noop, toggle: () => h('button', {}) },
  beside: h('div', {}),
  say: noop,
  current: () => JSON.parse(JSON.stringify(stored)),
  changed: noop,
});

const tick = () => new Promise((r) => setTimeout(r, 5));
const floorsNow = () => floorsOnServer();

await builder.open({ x: 3, y: 4, kind });
await tick();
assert.equal(floorsNow(), 1, 'opens on one floor');

// 1 -> 2 -> 1, each one a change the server takes.
await findTile('floors-2').props.onclick();
assert.equal(floorsNow(), 2, 'two floors after the first press');
await findTile('floors-1').props.onclick();
assert.equal(floorsNow(), 1, 'one floor again');
assert.equal(writes.length, 2, 'two saves so far');

// Undo walks back: the 2-floor document, then the first one (the 1-floor start, which is empty).
assert.ok(findButton('Undo'), 'Undo is on the dock');
await findButton('Undo').props.onclick();
assert.equal(floorsNow(), 2, 'undo brings back two floors');
await findButton('Undo').props.onclick();
assert.equal(floorsNow(), 1, 'undo again brings back the start');
assert.equal(writes.length, 4, 'each undo is one more save');
assert.deepEqual(writes.at(-1), {}, 'the start is the empty document');

// Nothing left to undo: a press does nothing and sends nothing.
await findButton('Undo').props.onclick();
assert.equal(writes.length, 4, 'an empty undo sends nothing');

console.log('Outside floors 1-2-1 and Undo: pass (run through outsideBuilder with a stub server).');
