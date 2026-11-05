// node tests/tag_picker_ui.cjs: #548, the @ picker groups its choices, says whose each is, and still
// picks with the keyboard, skipping the headings.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
class Node {
  constructor(tag, props = {}, children = []) {
    this.tag = tag; this.props = { ...props }; this.children = []; this.style = {}; this.attrs = {};
    Object.assign(this, props); this.append(...children);
  }
  append(...c) { this.children.push(...c.flat(Infinity).filter((x) => x != null && x !== false)); }
  replaceChildren(...c) { this.children = []; this.append(...c); }
  setAttribute(k, v) { this.attrs[k] = v; } removeAttribute(k) { delete this.attrs[k]; }
  querySelector(sel) { return all(this).find((n) => sel === '[aria-selected="true"]' && n.props['aria-selected'] === 'true'); }
  scrollIntoView() {}
}
const all = (n) => (n instanceof Node ? [n, ...n.children.flatMap(all)] : []);
const text = (n) => (n instanceof Node ? n.children.map(text).join('') : String(n));
const h = (tag, props, ...children) => new Node(tag, props, children);
const listeners = {};
const input = Object.assign(new Node('input'), {
  value: '', selectionStart: 0,
  addEventListener: (type, fn) => { listeners[type] = fn; },
  setSelectionRange(a) { this.selectionStart = a; }, focus() {},
});
const context = { h };
vm.createContext(context);
const start = app.indexOf('let pickerCount = 0;');
vm.runInContext(app.slice(start, app.indexOf('\n}\n', app.indexOf('function mentionBox(')) + 2) + '\nthis.mentionBox = mentionBox;', context);
const names = () => [
  { name: 'room', kind: 'everyone here', first: true, group: 'Everyone', detail: 'every bot here that listens' },
  { name: 'Ann', kind: 'person', group: 'Here', detail: 'person', color: '#c33' },
  { name: 'Rover', kind: 'bot', group: 'Here', detail: "Ann's bot · online", color: '#c33' },
  { name: 'Robin', kind: 'person', group: 'In this city', detail: 'person' },
];
const wrap = context.mentionBox(input, { names });
const list = wrap.children[1];
const type = (value) => { input.value = value; input.selectionStart = value.length; listeners.input(); };
const key = (k) => listeners.keydown({ key: k, preventDefault() {}, stopPropagation() {} });

type('@');
const headings = list.children.filter((n) => n.props.class === 'mention-group').map(text);
assert.deepEqual(headings, ['Everyone', 'Here', 'In this city'], 'grouped, in the order they come');
const options = list.children.filter((n) => n.props.role === 'option');
assert.deepEqual(options.map((o) => text(o.children[1].children[0])), ['room', 'Ann', 'Rover', 'Robin']);
assert.equal(text(options[2].children[1].children[1]), "Ann's bot · online", 'one short line says whose and how');
assert.equal(options[1].children[0].style.background, '#c33', 'a dot in the colour of whose it is');
assert.equal(options[3].children[0].style.background, undefined, 'no colour, no paint');
// Typed letters are bold, and the match filters as before.
type('@Ro');
const shownNow = list.children.filter((n) => n.props.role === 'option');
assert.deepEqual(shownNow.map((o) => text(o.children[1].children[0])), ['room', 'Rover', 'Robin']);
const rover = shownNow[1].children[1].children[0];
assert.equal(rover.children[1].tag, 'b'); assert.equal(text(rover.children[1]), 'Ro');
// The keyboard moves over options only, never a heading, and Enter puts the whole name in.
key('ArrowDown');
assert.equal(input.attrs['aria-activedescendant'].endsWith('-1'), true);
key('Enter');
assert.equal(input.value, '@Rover ');
// A list with no groups looks as it did: no headings (the friend inviter).
const plainInput = Object.assign(new Node('input'), { value: '', selectionStart: 0, addEventListener: (t, fn) => { listeners['p' + t] = fn; }, setSelectionRange() {}, focus() {} });
const plain = context.mentionBox(plainInput, { names: () => [{ name: 'Zed', kind: 'friend' }] });
plainInput.value = '@'; plainInput.selectionStart = 1; listeners.pinput();
assert.equal(plain.children[1].children.filter((n) => n.props.class === 'mention-group').length, 0);
assert.ok(app.includes("add(bot.name, 'bot', { group: 'Here', detail: botLine(bot), color: colorOf(bot.owner_id) });"));
console.log('Tag picker: grouped with headings, whose and how on one line, bold match, keyboard skips headings.');
