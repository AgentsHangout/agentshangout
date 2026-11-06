// MM19 to MM23: the bot's mind map expanded, and its notes beside it, read reliably. Runs the shipped
// trust panel against a small DOM with focus, events and sizes, and a server whose replies the test
// holds back, fails or lets through, as trust_ui.cjs and memories_ui.cjs do.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
// How big the stage measures, as the browser would report it.
let stageSize = {width: 900, height: 640};
class Node {
  constructor(tag, props = {}, children = []) {
    this.tag = tag; this.tagName = tag.toUpperCase(); this.props = props; this.children = []; this.parent = null; this.listeners = {};
    Object.assign(this, props); this.className = props.class || ''; this.value = props.value ?? '';
    const names = () => this.className.split(' ').filter(Boolean);
    this.classList = {
      add: (...more) => { this.className = [...new Set([...names(), ...more])].join(' '); },
      toggle: (name, on) => { const has = names().includes(name); if (on ?? !has) { if (!has) this.className = [...names(), name].join(' '); } else this.className = names().filter(n => n !== name).join(' '); },
      contains: name => names().includes(name),
    };
    for (const [key, value] of Object.entries(props)) if (key.startsWith('on') && typeof value === 'function') this.addEventListener(key.slice(2), value);
    this.append(...children);
  }
  append(...children) { for (const c of children.flat(Infinity).filter(c => c != null && c !== false)) { if (c instanceof Node) c.parent = this; this.children.push(c); } }
  replaceChildren(...children) { for (const c of this.children) if (c instanceof Node) c.parent = null; this.children = []; this.append(...children); }
  contains(node) { return all(this).includes(node); }
  closest(selector) { for (let n = this; n; n = n.parent) if (selector.startsWith('.') ? n.classList.contains(selector.slice(1)) : selector === '[hidden]' ? n.hidden : false) return n; return null; }
  get isConnected() { let n = this; while (n.parent) n = n.parent; return n === root; }
  setAttribute(key, value) { this.props[key] = value; }
  addEventListener(type, fn) { (this.listeners[type] ||= []).push(fn); }
  removeEventListener(type, fn) { this.listeners[type] = (this.listeners[type] || []).filter(f => f !== fn); }
  // Events bubble from here up through its parents.
  dispatch(type, event = {}) { for (let n = this; n; n = n.parent) for (const fn of [...(n.listeners[type] || [])]) fn({target: this, ...event}); }
  querySelector(selector) { return this.querySelectorAll(selector)[0]; }
  querySelectorAll(selector) {
    return all(this).filter(n => selector === 'button' ? n.tag === 'button' : selector.startsWith('.') ? n.classList.contains(selector.slice(1)) : selector === '[aria-label="Bot to manage"]' ? n.props['aria-label'] === 'Bot to manage' : false);
  }
  getBoundingClientRect() { return this.classList.contains('trust-stage') || this.classList.contains('trust-graph') ? {left: 0, top: 0, ...stageSize} : {left: 0, top: 0, width: 0, height: 0}; }
  // Focus moves here: the old one hears focusout with where focus went, as a browser sends it.
  focus() { const was = context.document.activeElement; if (was === this) return; context.document.activeElement = this; was?.dispatch?.('focusout', {relatedTarget: this}); }
}
const h = (tag, props = {}, ...children) => new Node(tag, props, children);
const all = node => node instanceof Node ? [node, ...node.children.flatMap(all)] : [];
const text = node => node instanceof Node ? node.children.map(text).join(' ') : String(node);
const tick = () => new Promise(resolve => setImmediate(resolve));
const policy = () => Object.fromEntries(['wake', 'invite_public', 'invite_private'].map(k => [k, {mode: 'participants', people: [], bots: []}]));
// Nine bonds and a friend make a crowded map, as in trust_ui.cjs.
const data = {
  owner_id: 'owner',
  people: [{id: 'owner', name: 'You'}, {id: 'ana', name: 'Ana'}],
  bots: [{id: 'one', name: 'Fern', owner_id: 'owner'}, {id: 'other', name: 'Pine', owner_id: 'ana'}],
  agents: [{id: 'one', name: 'Fern', doorbell: {paused: false}, trust: policy()}],
  friendships: [{kind: 'person', from_id: 'owner', to_id: 'ana', status: 'accepted'}, {kind: 'bot', from_id: 'one', to_id: 'other', status: 'accepted'}],
  looks: {people: {}, bots: {}},
};
const bonds = ['Mac', 'Chainy', 'Victor Sausage', 'Axiom', 'Ron', 'Morgkorg', 'A very long bot name indeed', 'Aionoro', 'Kyle’s Agent'].map((name, i) => ({agent: {id: `x${i}`, name}, strength: i + 1, built: 0}));
const notes = () => ({
  bot: {id: 'one', name: 'Fern'}, slept_at: '2026-11-03T22:15:00Z',
  notes: [{id: 'n1', kind: 'note', subject: 'person:ana', name: 'Ana', text: 'Ana lends me paint.', when: '2026-11-03T22:15:00Z', feeling: 'fond', closeness: 2, recalls: 0}],
  moments: [{id: 'm1', kind: 'moment', subject: 'self', name: 'Fern', text: 'Painted a fox.', when: '2026-11-03T09:00:00Z', importance: 3, recalls: 0}],
  moments_total: 1,
  edges: [{from: 'one', to: 'ana', to_kind: 'person', name: 'Ana', feeling: 'fond', closeness: 2}],
});
// Each read of the notes waits here until the test answers it.
let held = [], requests = [];
const api = async (path, options = {}) => {
  requests.push({path, ...options});
  if (path === '/api/trust') return JSON.parse(JSON.stringify(data));
  if (path === '/api/agents/one/memories') return new Promise((resolve, reject) => held.push({resolve, reject}));
  if (path.endsWith('/monitoring')) return {connections: [], eligible_rooms: []};
  if (path.endsWith('/bonds')) return {bonds};
  if (path.endsWith('/notify')) return {only_me: false, url: null};
  throw Error(`Unexpected ${path}`);
};
// The page: a game panel holding the panel's body, as the app has it.
const root = h('main');
let body, options;
const gamePanel = h('section', {class: 'game-panel'});
root.append(gamePanel);
const panels = {add: (_n, _t, opts) => { options = opts; body = h('div'); gamePanel.append(body); return body; }, toggle: () => h('button'), open: () => options.onOpen()};
const listeners = {};
const frames = [];
const context = {
  h, api, panels, console, innerWidth: 1280, innerHeight: 800,
  document: {activeElement: null, body: h('body'), querySelector: () => null},
  addEventListener: (type, fn) => { (listeners[type] ||= []).push(fn); },
  removeEventListener: (type, fn) => { listeners[type] = (listeners[type] || []).filter(f => f !== fn); },
  requestAnimationFrame: fn => frames.push(fn),
};
vm.createContext(context);
vm.runInContext(fs.readFileSync('web/trust.js', 'utf8').replace('export function trustPanel', 'function trustPanel'), context);
const settle = async () => { while (frames.length) frames.shift()(); await tick(); await tick(); };
const nodes = () => all(body).filter(n => n.tag === 'g' && n.props.class?.includes('trust-node'));
const node = name => nodes().find(n => n.props['aria-label']?.startsWith(`${name}, `));
const pick = name => node(name).props.onkeydown({key: 'Enter', preventDefault() {}});
const findButton = label => all(body).find(n => n.tag === 'button' && text(n) === label);
const notesPanel = () => body.querySelector('.trust-notes');
const map = () => body.querySelector('.trust-map');
const picture = () => all(body).find(n => n.tag === 'svg' && n.props.class === 'trust-graph');
const viewBox = () => picture().props.viewBox.split(' ').map(Number);
const scale = () => Number(all(body).find(n => n.props.class === 'trust-world').props['data-scale']);
const nameInput = () => all(body).find(n => n.tag === 'input' && n.props['aria-label'] === 'Bot name');
const keyDown = key => { let stopped = false; for (const fn of [...(listeners.keydown || [])]) fn({key, preventDefault() {}, stopPropagation() { stopped = true; }}); return stopped; };
// Where each node is drawn and the box its name takes, in the picture's own units.
const drawn = () => nodes().map(g => {
  const [x, y] = g.props.transform.match(/-?[\d.]+/g).map(Number);
  const label = all(g).find(n => n.props.class === 'label'); const w = [...text(label).replace(/\s+/g, ' ').trim()].length * 6.4 + 6;
  const half = g.props.class.includes(' you') ? 22 : g.props.class.includes(' person') ? 17 : g.props.class.includes(' mine') ? 19 : 16;
  const cx = x + Number(label.props.x), cy = y + Number(label.props.y);
  return {name: g.props['aria-label'], x, y, half, box: {x: cx - w / 2, y: cy - 10, w, h: 13}};
});
const closest = () => { const d = drawn(); let best = Infinity; for (const a of d) for (const b of d) if (a !== b) best = Math.min(best, Math.hypot(a.x - b.x, a.y - b.y)); return best; };
const meet = (a, b) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
const noNameOverlaps = where => {
  const d = drawn();
  for (const a of d) for (const b of d) {
    if (a === b) continue;
    if (a.name < b.name) assert.equal(meet(a.box, b.box), false, `${where}: names ${a.name} and ${b.name} overlap`);
    assert.equal(meet(a.box, {x: b.x - b.half, y: b.y - b.half, w: b.half * 2, h: b.half * 2}), false, `${where}: name of ${a.name} sits on ${b.name}`);
  }
};
// Every node, with its ring and name, inside the picture after the view's transform.
const wholeInView = where => {
  const world = all(body).find(n => n.props.class === 'trust-world');
  const [ox, oy] = world.props.transform.match(/translate\(([-\d.e]+) ([-\d.e]+)\)/).slice(1).map(Number), k = scale();
  const [, , vw, vh] = viewBox();
  for (const p of drawn()) {
    const x0 = Math.min(p.x - p.half - 5, p.box.x), x1 = Math.max(p.x + p.half + 5, p.box.x + p.box.w), y0 = Math.min(p.y - p.half - 5, p.box.y), y1 = Math.max(p.y + p.half + 5, p.box.y + p.box.h);
    assert.ok(ox + x0 * k >= -0.5 && ox + x1 * k <= vw + 0.5 && oy + y0 * k >= -0.5 && oy + y1 * k <= vh + 0.5, `${where}: ${p.name} is inside the picture`);
  }
};

(async () => {
  const panel = context.trustPanel({h, api, panels, me: {name: 'You', kind: 'person'}, crew: () => {}});
  await panel.open(); await tick(); await tick();

  // MM22: while the notes are read, the panel says so, and that isn't the same as having none.
  assert.equal(held.length, 1, 'the notes are asked for once');
  assert.ok(text(notesPanel()).includes('Reading Fern’s notes'), 'a loading state');
  assert.equal(notesPanel().props['aria-busy'], 'true');
  assert.ok(!text(body).includes('No notes yet.'), 'loading is not empty');
  assert.equal(findButton('Retry'), undefined);

  // MM21: the notes sit with the map, ahead of the bot's permissions and friendships.
  const order = all(body);
  const permissions = order.find(n => n.tag === 'h4' && text(n) === 'Who can reach it');
  const friends = order.find(n => n.tag === 'h4' && text(n).includes('bot friends'));
  assert.ok(permissions && friends, 'the bot editor is shown');
  assert.ok(order.indexOf(notesPanel()) < order.indexOf(permissions) && order.indexOf(notesPanel()) < order.indexOf(friends), 'notes come before permissions and friendships');
  assert.ok(map().contains(notesPanel()) && map().contains(picture()), 'the notes are beside the graph, in the same row');
  assert.ok(!body.querySelector('.trust-inspector').contains(notesPanel()), 'and not buried in the settings');

  // MM23: a delayed reply that lands while the owner types the bot's name doesn't replace the field.
  const typed = nameInput();
  typed.focus(); typed.value = 'Fernie'; typed.dispatch('input', {target: typed});
  held.shift().resolve(notes()); await tick(); await tick();
  assert.equal(nameInput(), typed, 'the field being typed in is still the one on screen');
  assert.equal(typed.value, 'Fernie');
  assert.ok(text(notesPanel()).includes('Reading Fern’s notes'), 'the reply waits');
  // Going on to the Rename button keeps waiting, so a press there isn't drawn over.
  const renameButton = findButton('Rename');
  renameButton.focus();
  assert.equal(nameInput(), typed, 'nothing is drawn under a press on the panel');
  // Leaving the panel draws it: the notes are there, and the name typed is kept.
  context.document.activeElement = null; renameButton.dispatch('focusout', {relatedTarget: null});
  assert.ok(text(notesPanel()).includes('Ana lends me paint.') && text(notesPanel()).includes('What Fern remembers'), 'the reply is shown after blur');
  assert.notEqual(nameInput(), typed, 'the panel was drawn again');
  assert.equal(nameInput().props.value, 'Fernie', 'what was typed outlasts the redraw');
  assert.equal(all(body).filter(n => n.tag === 'path' && n.props.class?.split(' ').includes('feeling')).length, 1, 'the feeling is drawn on the graph');

  // Unsaved permissions outlast the panel being drawn again too.
  const wake = () => all(body).find(n => n.tag === 'select' && n.props['aria-label'] === 'Wake it by mention');
  wake().props.onchange({target: {value: 'owner'}});
  pick('Fern');
  assert.ok(text(body).includes('Unsaved changes'));
  assert.equal(wake().children.find(n => n.selected).value, 'owner');

  // MM22: a failed read says it failed, with the reason and Retry, and Retry reads again.
  pick('Ana');
  assert.ok(text(notesPanel()).includes('Ana lends me paint.'), 'the note on whoever is picked');
  await panel.load();
  assert.equal(held.length, 0, 'a draft keeps the panel from reloading');
  pick('Fern'); findButton('Reset').props.onclick();
  await panel.load(); await tick(); await tick();
  assert.equal(held.length, 1);
  held.shift().reject(Error('Server unavailable')); await tick();
  assert.ok(text(notesPanel()).includes('Couldn’t read Fern’s notes.') && text(notesPanel()).includes('Server unavailable'), 'a failed state with its reason');
  assert.ok(!text(notesPanel()).includes('No notes yet.'), 'a failure is not shown as no notes');
  const retry = findButton('Retry');
  assert.ok(retry, 'a visible Retry'); assert.equal(retry.props['aria-label'], 'Retry reading Fern’s notes');
  retry.props.onclick();
  assert.ok(text(notesPanel()).includes('Reading Fern’s notes'), 'Retry reads again');
  assert.equal(held.length, 1, 'one read for one Retry');
  // A failure again keeps Retry; then it works.
  held.shift().reject(Error('Still down')); await tick();
  assert.ok(text(notesPanel()).includes('Still down'));
  findButton('Retry').props.onclick();
  held.shift().resolve(notes()); await tick();
  assert.ok(text(notesPanel()).includes('Ana lends me paint.'), 'a retried read that works shows the notes');
  assert.equal(findButton('Retry'), undefined);

  // MM22: none at all is its own state, and a late answer to a read that was replaced is ignored.
  await panel.load(); await tick(); await tick();
  await panel.load(); await tick(); await tick();
  assert.equal(held.length, 2, 'two reads, the second replacing the first');
  const [older, newer] = held.splice(0);
  newer.resolve({...notes(), notes: [], moments: [], moments_total: 0, edges: []}); await tick();
  older.resolve(notes()); await tick();
  assert.ok(text(notesPanel()).includes('No notes yet.') && text(notesPanel()).includes('No moments yet.'), 'an empty state');
  assert.ok(!text(notesPanel()).includes('Ana lends me paint.'), 'the older read is ignored');
  pick('Ana');
  assert.ok(text(notesPanel()).includes('Fern has no note on Ana yet.'), 'and for whoever is picked');
  await panel.load(); await tick(); await tick();
  held.shift().resolve(notes()); await tick();
  pick('Ana');

  // MM19: Expand sits beside Fit, with a label.
  const fit = findButton('Fit'), expand = findButton('Expand');
  assert.ok(expand, 'an Expand button');
  const group = all(body).find(n => n.props.class === 'trust-zoom');
  assert.ok(group.contains(fit) && group.contains(expand), 'beside Fit');
  assert.equal(expand.props['aria-label'], 'Expand the map to fill the screen');
  const collapsedBox = viewBox(), collapsedClosest = closest(), collapsedScale = scale();
  assert.deepEqual(collapsedBox, [0, 0, 480, 300]);
  noNameOverlaps('the panel');
  const before = Object.fromEntries(drawn().map(p => [p.name, [p.x, p.y]]));

  // Expanding keeps what is picked, takes the panel, and focus stays on the button, now Exit expanded.
  expand.focus(); expand.props.onclick(); await settle();
  assert.ok(map().classList.contains('expanded'), 'the map is expanded');
  assert.ok(gamePanel.classList.contains('trust-expanded-panel'), 'the panel takes the space under the top bar');
  const exit = findButton('Exit expanded');
  assert.ok(exit, 'the button now reads Exit expanded'); assert.equal(exit.props['aria-label'], 'Exit expanded map');
  assert.equal(context.document.activeElement, exit, 'focus stays on the button');
  assert.ok(node('Ana').props.class.includes('focused'), 'the picked node stays picked');
  assert.ok(text(notesPanel()).includes('Ana lends me paint.'), 'with its notes beside the map');
  assert.equal(body.querySelector('.trust-inspector'), undefined, 'settings wait until it is not expanded');
  assert.ok(map().contains(all(body).find(n => n.props.class === 'trust-legend fine')), 'the key stays, under the notes');

  // MM20: the picture is the stage's shape at a unit a pixel, and a crowded map spreads out over it.
  assert.deepEqual(viewBox(), [0, 0, 900, 640], 'the picture is the space it has');
  assert.ok(closest() > collapsedClosest * 1.5, `nodes are further apart: ${closest()} from ${collapsedClosest}`);
  noNameOverlaps('expanded');
  wholeInView('expanded, fitted');
  // Fit still shows the whole map: zoom in, then Fit goes back to the fitted view.
  const fitted = scale();
  findButton('+').props.onclick(); assert.ok(scale() > fitted);
  findButton('Fit').props.onclick(); assert.equal(scale(), fitted, 'Fit is the whole map');
  wholeInView('expanded, after Fit');

  // Resizing the window measures the space again.
  stageSize = {width: 1200, height: 700};
  for (const fn of listeners.resize || []) fn(); await settle();
  assert.deepEqual(viewBox(), [0, 0, 1200, 700]);
  wholeInView('expanded, resized');
  noNameOverlaps('expanded, resized');

  // Escape exits before the page's own Escape, and focus goes back to the button.
  assert.equal(keyDown('Escape'), true, 'Escape is taken');
  assert.ok(!map().classList.contains('expanded'));
  assert.ok(!gamePanel.classList.contains('trust-expanded-panel'));
  const back = findButton('Expand');
  assert.equal(context.document.activeElement, back, 'focus returns to Expand');
  assert.equal((listeners.keydown || []).length, 0, 'nothing is left listening for Escape');
  assert.equal((listeners.resize || []).length, 0);
  assert.ok(node('Ana').props.class.includes('focused'), 'the picked node is still picked');
  assert.ok(body.querySelector('.trust-inspector'), 'settings are back');
  assert.deepEqual(viewBox(), [0, 0, 480, 300]);
  assert.ok(Math.abs(scale() - collapsedScale) < 0.15, `fitted as before: ${scale()} and ${collapsedScale}`);
  for (const p of drawn()) { const [x, y] = before[p.name]; assert.ok(Math.hypot(p.x - x, p.y - y) < 40, `${p.name} came back near where it was`); }
  noNameOverlaps('back in the panel');

  // Exit expanded does the same as Escape.
  findButton('Expand').props.onclick(); await settle();
  findButton('Exit expanded').props.onclick(); await settle();
  assert.equal(context.document.activeElement, findButton('Expand'));
  assert.equal(keyDown('Escape'), false, 'Escape is the page’s again');

  // Narrow: a phone 375 wide. The picture keeps the stage's shape and is never narrower than 480 units.
  context.innerWidth = 375; context.innerHeight = 812; stageSize = {width: 363, height: 420};
  findButton('Expand').props.onclick(); await settle();
  const [, , nw, nh] = viewBox();
  assert.equal(nw, 480, 'never narrower than the panel picture');
  assert.ok(Math.abs(nw / nh - 363 / 420) < 0.01, `the stage's shape: ${nw} by ${nh}`);
  noNameOverlaps('expanded on a phone');
  wholeInView('expanded on a phone');
  // Opening the panel again starts it not expanded, and leaves nothing listening.
  await panel.load(); await tick(); await tick();
  held.shift()?.resolve(notes()); await tick();
  assert.ok(!map().classList.contains('expanded') && !gamePanel.classList.contains('trust-expanded-panel'));
  assert.equal((listeners.keydown || []).length, 0);

  // The stylesheet has the expanded rules the script relies on, and a narrow layout for them.
  const css = fs.readFileSync('web/trust.css', 'utf8');
  for (const rule of ['.trust-map.expanded', '.game-panel.trust-expanded-panel', '.trust-notes']) assert.ok(css.includes(rule), rule);
  console.log('MM19 to MM23: expanded mind map and reliable notes passed');
})().catch(error => { console.error(error); process.exitCode = 1; });
