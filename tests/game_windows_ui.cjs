// W114: shared window lifecycle, without a server or account.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
class Element {
  constructor(tag, props = {}, children = []) {
    this.tag = tag; Object.assign(this, props); this.children = children.flat().filter(Boolean);
    this.style = { setProperty(k, v) { this[k] = v; } }; this.dataset = {}; this.offsetParent = {};
    this.classList = { add() {}, remove() {}, toggle() {} };
  }
  append(...nodes) { this.children.push(...nodes); }
  replaceChildren(...nodes) { this.children = nodes; }
  setAttribute(k, v) { this[k] = v; }
  addEventListener() {}
  querySelector() { return null; }
  querySelectorAll() { return []; }
  getBoundingClientRect() { return {left:0, top:0, right:900, bottom:600, width:900, height:600}; }
  focus() { this.focused = true; }
}
const h = (tag, props, ...children) => new Element(tag, props, children);
const root = h('div', {});
const source = fs.readFileSync('web/app.js', 'utf8');
const start = source.indexOf('function createPanels(root) {');
const end = source.indexOf('// What goes in the panels', start);
const context = vm.createContext({h, document: {activeElement:null}, navigator:{}, installSheetDrag() {},
  ResizeObserver: class {observe() {} disconnect() {}}, getComputedStyle: () => ({display:'block'}),
  requestAnimationFrame: fn => fn(), window: {innerWidth:900, innerHeight:600}, Math});
vm.runInContext(source.slice(start, end) + '\nthis.createPanels = createPanels;', context);
const panels = context.createPanels(root);
let opens = 0, closes = 0;
const input = h('input', {value:'unfinished message'});
panels.add('chat', 'Chat', {onOpen:() => opens++, onClose:() => closes++}, input);
panels.add('bots', 'My bots', {}, h('p', {}, 'Connections'));
const find = name => root.children.find(n => n.id === `panel-${name}`);
panels.open('chat'); panels.open('bots');
assert.equal(find('chat').hidden, false, 'opening another window keeps chat visible');
panels.minimize('chat');
assert.equal(find('chat').hidden, true);
assert.equal(closes, 0, 'minimizing does not close or discard input');
panels.open('chat');
assert.equal(input.value, 'unfinished message');
assert.equal(opens, 1, 'restoring does not rerun the content loader');
panels.maximize('chat'); panels.maximize('chat');
panels.close();
assert.equal(closes, 1);
assert.equal(find('chat').hidden, true);
assert.equal(find('bots').hidden, false);
panels.open('chat');
assert.equal(opens, 2, 'reopening a closed window refreshes it');
panels.minimizeAll();
assert.equal(panels.current, null);
assert.equal(find('chat').hidden, true);
assert.equal(find('bots').hidden, true);
assert.equal(closes, 1, 'returning to the room preserves windows');
console.log('W114 window lifecycle passed');
