// I91: exercise the production object dispatcher and pin panel with existing permissions.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
function extract(name) {
  const start = source.indexOf(`  function ${name}(`);
  assert.ok(start >= 0, name);
  return source.slice(start, source.indexOf('\n  }\n', start) + 4);
}
const opened = [];
let guest = false;
let research = 0;
const node = (tag, props = {}, ...children) => ({tag, ...props, children:children.flat().filter(Boolean)});
const context = vm.createContext({
  panels: {open: name => opened.push(name)}, view: () => ({guest}), useDisplay: () => research++,
  h: node, id:'room', snapshot: {pins:[],can_pin:false,can_clear_pins:false},
  pinsPanelEl: {replaceChildren(...children) {this.children = children;}},
  pinBy: pin => pin.by, pinDialog: pin => opened.push(pin.id),
  addPinDialog: () => opened.push('new-pin'),
});
vm.runInContext(`${extract('openItemWorkspace')}\n${extract('renderPinsPanel')}`, context);
for (const action of ['boards','notes','calendar','pins']) context.openItemWorkspace({opens:action});
assert.deepEqual(opened, ['boards','notes','calendar','pins']);
guest = true;
context.openItemWorkspace({opens:'calendar'});
assert.equal(opened.length, 4, 'walk-ins do not open the private calendar');
context.openItemWorkspace({opens:'research'});
assert.equal(research, 1, 'research uses the existing access-checked display flow');
context.openItemWorkspace({opens:'arbitrary-action'});
context.openItemWorkspace(null);
assert.equal(opened.length, 4, 'unknown actions cannot navigate');
function all(node) { return node && typeof node === 'object' ? [node, ...(node.children || []).flatMap(all)] : []; }
context.renderPinsPanel();
assert.equal(all(context.pinsPanelEl).filter(x => x.tag === 'button').length, 0, 'read-only pin view has no write actions');
context.snapshot = {pins:[{id:'pin',title:'Pinned reference',kind:'link',by:'Bot'}],can_pin:true,can_clear_pins:false};
context.renderPinsPanel();
const buttons = all(context.pinsPanelEl).filter(x => x.tag === 'button');
assert.equal(buttons.length, 2, 'pin readers and authorized add action share the panel');
buttons[0].onclick(); buttons[1].onclick();
assert.deepEqual(opened.slice(-2), ['pin','new-pin']);
assert.ok(!buttons.some(x => x.children.includes('Clear pins')), 'ordinary pinners cannot clear others');
console.log('I91: object panel routing, guest calendar restriction, research reuse and pin controls pass.');
