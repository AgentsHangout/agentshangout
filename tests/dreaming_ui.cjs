// BT12, BT13, BT14: the Dreaming graph's Expand, its notes panel and its names, run with the shipped
// web/trust.js against a small DOM that keeps track of focus, and a server whose replies wait until
// the test lets them go.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const document = {activeElement: null, body: null, querySelector: () => null};
class Node {
  constructor(tag, props = {}, children = []) {
    this.tag = tag; this.tagName = tag.toUpperCase(); this.props = props; this.children = []; this.parentNode = null;
    Object.assign(this, props); this.className = props.class || ''; this.value = props.value || '';
    this.classList = {add: (...names) => { this.className += ` ${names.join(' ')}`; }};
    // Only the names: an inline size the page sets is kept here, so the test can see it.
    if (tag === 'text') this.style = {setProperty: (key, value) => { this.style[key] = value; }};
    this.append(...children);
  }
  append(...children) {
    for (const c of children.flat(Infinity).filter(c => c != null && c !== false)) { if (c instanceof Node) c.parentNode = this; this.children.push(c); }
  }
  replaceChildren(...children) {
    // As in a browser, a focused element taken out of the page loses its focus.
    for (const c of this.children) if (c instanceof Node && c.contains(document.activeElement)) document.activeElement = null;
    this.children = []; this.append(...children);
  }
  contains(node) { return all(this).includes(node); }
  closest(selector) { for (let n = this; n; n = n.parentNode) if (n.matches(selector)) return n; return null; }
  matches(selector) { return selector.startsWith('.') ? this.className.split(' ').includes(selector.slice(1)) : this.tag === selector; }
  setAttribute(key, value) { this.props[key] = value; }
  querySelector(selector) { return this.querySelectorAll(selector)[0]; }
  querySelectorAll(selector) {
    if (selector === '[aria-label="Bot to manage"]') return all(this).filter(n => n.props['aria-label'] === 'Bot to manage');
    return all(this).filter(n => n.matches(selector));
  }
  focus() { if (attached(this)) document.activeElement = this; }
  getClientRects() { return shownPanel ? [{}] : []; }
}
const h = (tag, props = {}, ...children) => new Node(tag, props, children);
const all = node => node instanceof Node ? [node, ...node.children.flatMap(all)] : [];
const text = node => node instanceof Node ? node.children.map(text).join(' ') : String(node);
const attached = node => { while (node.parentNode) node = node.parentNode; return node === body; };
const findButton = label => all(body).find(n => n.tag === 'button' && text(n) === label);
const tick = () => new Promise(resolve => setImmediate(resolve));
let shownPanel = true;
const policy = () => Object.fromEntries(['wake', 'invite_public', 'invite_private'].map(k => [k, {mode: 'participants', people: [], bots: []}]));
const data = {
  owner_id: 'owner',
  people: [{id: 'owner', name: 'You'}, {id: 'ana', name: 'Ana'}],
  bots: [{id: 'one', name: 'Fern', owner_id: 'owner'}],
  agents: [{id: 'one', name: 'Fern', doorbell: {paused: false}, trust: policy()}],
  friendships: [{kind: 'person', from_id: 'owner', to_id: 'ana', status: 'accepted'}],
  looks: {people: {}, bots: {}},
};
const notes = () => ({
  bot: {id: 'one', name: 'Fern'}, slept_at: '2026-11-03T22:15:00Z',
  notes: [{id: 'n1', kind: 'note', subject: 'person:ana', name: 'Ana', text: 'Ana lends me paint.', when: '2026-11-03T22:15:00Z', feeling: 'fond', closeness: 2, recalls: 0}],
  moments: [], moments_total: 0,
  edges: [{from: 'one', to: 'ana', to_kind: 'person', name: 'Ana', feeling: 'fond', closeness: 2}],
});
// Each read of the notes waits here until the test answers it.
const reads = [];
const api = async path => {
  if (path === '/api/trust') return JSON.parse(JSON.stringify(data));
  if (path === '/api/agents/one/memories') return new Promise((resolve, reject) => reads.push({resolve, reject}));
  if (path.endsWith('/monitoring')) return {connections: [], eligible_rooms: []};
  if (path.endsWith('/bonds')) return {bonds: []};
  if (path.endsWith('/notify')) return {only_me: false, url: null};
  throw Error(`Unexpected ${path}`);
};
let body, options;
const panels = {add: (_n, _t, opts) => { options = opts; body = h('div'); return body; }, toggle: () => h('button'), open: () => options.onOpen()};
// The page's listeners, by type, with whether each listens before the page's own (capture).
const listeners = [];
const context = {h, api, panels, document, console,
  addEventListener: (type, fn, capture) => listeners.push({type, fn, capture: !!capture}),
  removeEventListener: (type, fn, capture) => { const i = listeners.findIndex(l => l.type === type && l.fn === fn && l.capture === !!capture); if (i >= 0) listeners.splice(i, 1); }};
vm.createContext(context);
vm.runInContext(fs.readFileSync('web/trust.js', 'utf8').replace('export function trustPanel', 'function trustPanel'), context);
const build = () => context.trustPanel({h, api, panels, me: {name: 'You', kind: 'person'}, crew: () => {}});
const notesPanel = () => all(body).find(n => n.matches('.trust-notes'));
const feelings = () => all(body).filter(n => n.tag === 'path' && n.className.split(' ').includes('feeling'));
const node = name => all(body).find(n => n.tag === 'g' && n.props['aria-label']?.startsWith(`${name}, `));
const pick = name => node(name).props.onkeydown({key: 'Enter', preventDefault() {}});
const map = () => all(body).find(n => n.matches('.trust-map'));
const scale = () => Number(all(body).find(n => n.matches('.trust-world')).props['data-scale']);
const escapeKey = () => {
  const event = {key: 'Escape', stopped: false, prevented: false, preventDefault() { this.prevented = true; }, stopPropagation() { this.stopped = true; }};
  for (const l of listeners.filter(l => l.type === 'keydown' && l.capture)) l.fn(event);
  return event;
};
(async () => {
  // BT13: the notes panel says the notes are being read, then that reading failed, with Retry.
  const panel = build(); await panel.open(); await tick();
  assert.ok(notesPanel(), 'a notes panel');
  assert.ok(text(notesPanel()).includes('Reading Fern’s notes…'), text(notesPanel()));
  assert.equal(findButton('Retry'), undefined, 'no Retry while reading');
  // It comes before the picked node's permissions and friendships.
  const order = all(body);
  assert.ok(order.indexOf(notesPanel()) < order.indexOf(all(body).find(n => n.matches('.trust-inspector'))), 'notes before the settings');
  assert.ok(notesPanel().closest('.trust-map'), 'the notes sit with the graph');
  reads.shift().reject(Error('Failed to fetch')); await tick();
  assert.ok(text(notesPanel()).includes('Couldn’t read Fern’s notes.'), text(notesPanel()));
  assert.ok(!text(notesPanel()).includes('No notes yet'), 'a failure does not look like no notes');
  assert.equal(feelings().length, 0);
  // Retry reads them again, and says so while it does.
  findButton('Retry').props.onclick(); await tick();
  assert.equal(reads.length, 1, 'Retry asks again');
  assert.ok(text(notesPanel()).includes('Reading Fern’s notes…'));
  assert.equal(document.activeElement, notesPanel().querySelector('.trust-notes-head'), 'focus stays on the notes');
  // With none, it says there are none.
  reads.shift().resolve({bot: {id: 'one', name: 'Fern'}, slept_at: null, notes: [], moments: [], moments_total: 0, edges: []}); await tick();
  assert.ok(text(notesPanel()).includes('No notes yet.'), text(notesPanel()));
  assert.equal(findButton('Retry'), undefined);

  // BT13: notes that come while the bot's name is being typed show at once, and leave the field,
  // its words and its focus alone.
  const typing = build(); await typing.open(); await tick();
  const name = all(body).find(n => n.tag === 'input' && n.props['aria-label'] === 'Bot name');
  name.focus(); name.value = 'Fern the Bold';
  assert.equal(document.activeElement, name);
  reads.shift().resolve(notes()); await tick();
  assert.ok(text(notesPanel()).includes('Ana lends me paint.'), 'the notes show while the field is in use');
  assert.equal(feelings().length, 1, 'and the graph draws their feeling');
  assert.ok(body.contains(name), 'the field is still on the page');
  assert.equal(name.value, 'Fern the Bold', 'with what was typed');
  assert.equal(document.activeElement, name, 'and its focus');
  // Once the field is left, the panel draws as usual, with the notes.
  name.value = 'Fern'; document.activeElement = null; pick('Ana');
  assert.ok(text(notesPanel()).includes('Ana lends me paint.') && text(notesPanel()).includes('Feels fond'));

  // BT12: Expand sits beside Fit, keeps the picked node and fills the screen.
  const zoom = all(body).find(n => n.matches('.trust-zoom'));
  const labels = zoom.children.map(text);
  assert.equal(labels.indexOf('Expand'), labels.indexOf('Fit') + 1, `Expand beside Fit: ${labels}`);
  assert.equal(findButton('Expand').props['aria-label'], 'Expand the graph to fill the screen');
  findButton('+').props.onclick();
  const before = scale();
  findButton('Expand').focus(); findButton('Expand').props.onclick();
  assert.ok(map().className.split(' ').includes('expanded'), 'the graph is expanded');
  assert.ok(map().contains(notesPanel()), 'with the notes beside it');
  assert.ok(node('Ana').props.class.includes('focused'), 'Ana is still picked');
  assert.ok(text(notesPanel()).includes('Ana lends me paint.'), 'and her note still shows');
  assert.equal(scale(), before, 'the view is kept');
  assert.equal(findButton('Exit expanded')?.props['aria-label'], 'Exit expanded view');
  assert.equal(document.activeElement, findButton('Exit expanded'), 'focus stays on the button');
  // Picking someone else while expanded keeps it expanded, and the notes take focus.
  pick('You');
  assert.ok(map().className.includes('expanded'));
  assert.equal(document.activeElement, notesPanel().querySelector('.trust-notes-head'));
  pick('Ana');
  // Zoomed in while expanded, Escape puts the view back as it was and gives focus to Expand.
  findButton('+').props.onclick(); assert.ok(scale() > before);
  const pressed = escapeKey();
  assert.ok(pressed.stopped && pressed.prevented, 'Escape is taken, so the panel stays open');
  assert.ok(!map().className.includes('expanded'), 'Escape leaves expanded');
  assert.equal(scale(), before, 'and puts the view back');
  assert.equal(document.activeElement, findButton('Expand'), 'focus goes back to Expand');
  assert.ok(node('Ana').props.class.includes('focused'), 'Ana is still picked');
  assert.equal(listeners.filter(l => l.type === 'keydown').length, 0, 'nothing listens for Escape any more');
  assert.equal(escapeKey().stopped, false, 'and Escape is left to the panel');
  // Exit expanded does the same.
  findButton('Expand').props.onclick(); findButton('Exit expanded').props.onclick();
  assert.ok(!map().className.includes('expanded')); assert.equal(document.activeElement, findButton('Expand'));
  // With the panel closed while expanded, Escape is left to the page and the next look isn't expanded.
  findButton('Expand').props.onclick(); shownPanel = false;
  assert.equal(escapeKey().stopped, false, 'a hidden panel does not take Escape');
  shownPanel = true; await typing.open(); await tick();
  assert.ok(!map().className.includes('expanded'), 'opening again is not expanded');
  reads.splice(0).forEach(r => r.resolve(notes())); await tick();

  // BT14: names are sized for the screen, inline, so a stylesheet can't undo it.
  const picture = () => all(body).find(n => n.tag === 'svg' && n.matches('.trust-graph'));
  const sizes = () => all(body).filter(n => n.tag === 'text' && n.matches('.label')).map(t => [Number(t.props['font-size']), t.style['font-size']]);
  const resize = () => listeners.filter(l => l.type === 'resize').forEach(l => l.fn());
  const expect = (want, why) => { for (const [attr, inline] of sizes()) { assert.ok(Math.abs(attr - want) < 0.01, `${why}: ${attr} not ${want}`); assert.equal(inline, `${attr}px`, `${why}: set inline`); } };
  findButton('Fit').props.onclick();
  let k = scale();
  expect(Math.min(15, Math.max(11, 11 / k)), 'drawn at its own size');
  // A narrow screen: the 480 wide picture is drawn 240 wide, so names are drawn larger.
  picture().getBoundingClientRect = () => ({left: 0, top: 0, width: 240, height: 150}); resize();
  expect(Math.min(15, Math.max(11, 11 / (k * 0.5))), 'on a narrow screen');
  assert.ok(sizes()[0][0] > 11, 'larger than at full size');
  // Zoomed out, larger still, up to 15.
  for (let i = 0; i < 10; i++) findButton('−').props.onclick();
  picture().getBoundingClientRect = () => ({left: 0, top: 0, width: 240, height: 150}); resize();
  expect(15, 'zoomed out on a narrow screen');
  // Expanded on a wide screen, names don't grow with the picture, and are placed again at their
  // size, closer to their nodes, so a crowded graph spreads out.
  findButton('Fit').props.onclick();
  const offset = () => { const l = all(node('Ana')).find(n => n.matches('.label')); return Math.hypot(Number(l.props.x), Number(l.props.y)); };
  const full = offset();
  findButton('Expand').props.onclick(); k = scale();
  picture().getBoundingClientRect = () => ({left: 0, top: 0, width: 1440, height: 900}); resize();
  expect(Math.min(15, Math.max(6, 11 / (k * 3))), 'expanded');
  assert.ok(sizes()[0][0] < 11, 'smaller in the graph than the picture would make them');
  assert.ok(offset() < full, `the name is placed for its size: ${offset()} against ${full}`);
  escapeKey();
  // trust.css sets no size on a name, in any rule, so the size the graph sets is the one drawn.
  const css = fs.readFileSync('web/trust.css', 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');
  const rules = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)].filter(([, selector]) => /\.label\b/.test(selector));
  assert.ok(rules.length, 'trust.css still styles the names');
  for (const [, selector, block] of rules) assert.ok(!/(^|;)\s*font(-size)?\s*:/.test(block), `${selector.trim()} sets a font size`);
  console.log('BT12, BT13, BT14: Expand, the notes panel and readable names passed');
})().catch(error => { console.error(error); process.exitCode = 1; });
