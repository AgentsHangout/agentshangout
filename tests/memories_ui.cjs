// MM16: the Trust panel's graph draws a bot's feelings from GET /api/agents/{id}/memories (MM15).
// Runs the shipped trust panel against a small DOM and a stubbed server, as trust_ui.cjs does.
// tests/memories_ui.rs passes a file holding what the real route returned, so the stub below is
// checked against it field by field, and the panel is run on the real reply too.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
class Node {
  constructor(tag, props = {}, children = []) {
    this.tag = tag; this.tagName = tag.toUpperCase(); this.props = props; this.children = [];
    Object.assign(this, props); this.className = props.class || ''; this.value = props.value || '';
    this.classList = {add: (...names) => { this.className += ` ${names.join(' ')}`; }};
    this.append(...children);
    if (tag === 'select') this.value = this.children.find(c => c.selected)?.value || this.children[0]?.value || '';
  }
  append(...children) { this.children.push(...children.flat(Infinity).filter(c => c != null && c !== false)); }
  replaceChildren(...children) { this.children = []; this.append(...children); }
  contains(node) { return all(this).includes(node); }
  setAttribute(key, value) { this.props[key] = value; }
  querySelector(selector) { return this.querySelectorAll(selector)[0]; }
  querySelectorAll(selector) {
    return all(this).filter(n => selector === 'button' ? n.tag === 'button' : selector.startsWith('.') ? n.className.split(' ').includes(selector.slice(1)) : selector === '[aria-label="Bot to manage"]' ? n.props['aria-label'] === 'Bot to manage' : false);
  }
  focus() {}
}
const h = (tag, props = {}, ...children) => new Node(tag, props, children);
const all = node => node instanceof Node ? [node, ...node.children.flatMap(all)] : [];
const text = node => node instanceof Node ? node.children.map(text).join(' ') : String(node);
const tick = () => new Promise(resolve => setImmediate(resolve));
const policy = () => Object.fromEntries(['wake', 'invite_public', 'invite_private'].map(k => [k, {mode: 'participants', people: [], bots: []}]));
const data = {
  owner_id: 'owner',
  people: [{id: 'owner', name: 'You'}, {id: 'ana', name: 'Ana'}],
  bots: [{id: 'one', name: 'Fern', owner_id: 'owner'}, {id: 'other', name: 'Pine', owner_id: 'ana'}],
  agents: [{id: 'one', name: 'Fern', doorbell: {paused: false}, trust: policy()}],
  friendships: [{kind: 'person', from_id: 'owner', to_id: 'ana', status: 'accepted'}],
  looks: {people: {}, bots: {}},
};
const evil = '<img src=x onerror=alert(1)>';
// Exactly what the server sends: ISO times, bare ids on edges with to_kind, and subject keys on
// memories. A note's feeling and closeness are left out when unset.
const fresh = () => ({
  bot: {id: 'one', name: 'Fern'},
  slept_at: '2026-11-03T22:15:00Z',
  notes: [
    {id: 'n1', kind: 'note', subject: 'person:ana', name: 'Ana', text: 'Ana lends me paint.', when: '2026-11-03T22:15:00Z', feeling: 'fond', closeness: 2, recalls: 0},
    {id: 'n2', kind: 'note', subject: 'person:quill', name: 'Quill', text: evil, when: '2026-11-03T22:15:00Z', feeling: 'wary', closeness: -1, recalls: 1},
    {id: 'n3', kind: 'note', subject: 'agent:other', name: 'Pine', text: 'Pine hums all day.', when: '2026-11-03T22:15:00Z', feeling: 'curious', closeness: 0, recalls: 0},
    {id: 'n4', kind: 'note', subject: 'room:studio', name: 'Studio', text: 'A quiet room.', when: '2026-11-03T22:15:00Z', recalls: 0},
  ],
  moments: [
    {id: 'm2', kind: 'moment', subject: 'person:quill', name: 'Quill', text: 'Met Quill at the pier.', when: '2026-11-03T20:00:00Z', importance: 4, recalls: 0},
    {id: 'm1', kind: 'moment', subject: 'self', name: 'Fern', text: 'Painted a fox.', when: '2026-11-03T09:00:00Z', importance: 3, recalls: 2},
  ],
  moments_total: 2,
  edges: [
    {from: 'one', to: 'ana', to_kind: 'person', name: 'Ana', feeling: 'fond', closeness: 2},
    {from: 'one', to: 'quill', to_kind: 'person', name: 'Quill', feeling: 'wary', closeness: -1},
    {from: 'one', to: 'other', to_kind: 'bot', name: 'Pine', feeling: 'curious', closeness: 0},
  ],
});
let memories = null, requests = [];
const api = async (path, options = {}) => {
  requests.push({path, ...options});
  if (path === '/api/trust') return JSON.parse(JSON.stringify(data));
  if (path === '/api/agents/one/memories') {
    if (!memories) { const e = Error('Not found'); e.status = 404; throw e; }
    return JSON.parse(JSON.stringify(memories));
  }
  if (path.startsWith('/api/agents/one/memories/') && options.method === 'DELETE') {
    const id = decodeURIComponent(path.split('/').pop());
    for (const key of ['notes', 'moments']) memories[key] = memories[key].filter(m => m.id !== id);
    memories.moments_total = memories.moments.length;
    const gone = fresh().notes.find(m => m.id === id);
    if (gone) memories.edges = memories.edges.filter(e => `${e.to_kind === 'bot' ? 'agent' : 'person'}:${e.to}` !== gone.subject);
    return {ok: true, deleted: id};
  }
  if (path.endsWith('/monitoring')) return {connections: [], eligible_rooms: []};
  if (path.endsWith('/bonds')) return {bonds: []};
  if (path.endsWith('/notify')) return {only_me: false, url: null};
  throw Error(`Unexpected ${path}`);
};
let body, options;
const panels = {add: (_n, _t, opts) => { options = opts; body = h('div'); return body; }, toggle: () => h('button'), open: () => options.onOpen()};
const context = {h, api, panels, document: {activeElement: null, body: h('body')}, console};
vm.createContext(context);
vm.runInContext(fs.readFileSync('web/trust.js', 'utf8').replace('export function trustPanel', 'function trustPanel'), context);
const build = () => context.trustPanel({h, api, panels, me: {name: 'You', kind: 'person'}, crew: () => {}});
const nodes = () => all(body).filter(n => n.tag === 'g' && n.props.class?.includes('trust-node'));
const node = name => nodes().find(n => n.props['aria-label']?.startsWith(`${name}, `));
const pick = name => node(name).props.onkeydown({key: 'Enter', preventDefault() {}});
const lines = () => all(body).filter(n => n.tag === 'path' && n.props.class?.includes('trust-edge'));
const feelings = () => lines().filter(n => n.props.class.split(' ').includes('feeling'));
const lineTo = name => feelings().find(n => n.props['aria-label']?.endsWith(name) || n.props['aria-label']?.includes(`${name},`));
const where = () => Object.fromEntries(nodes().map(g => [g.props['aria-label'], g.props.transform.match(/-?[\d.]+/g).map(Number)]));
const shape = () => ({nodes: nodes().map(g => g.props['aria-label']).sort(), lines: lines().map(l => l.props.class).sort()});
(async () => {
  // A 404 draws the graph exactly as it was without memories, and lists none.
  const panel = build(); await panel.open(); await tick(); await tick();
  const before = shape(), at = where();
  assert.equal(feelings().length, 0);
  assert.ok(requests.some(r => r.path === '/api/agents/one/memories'), 'memories are asked for when the bot is picked');
  assert.ok(!text(body).includes('remembers'), 'no memories section without memories');
  assert.ok(before.nodes.some(n => n.startsWith('Ana, ')) && before.nodes.some(n => n.startsWith('Fern, ')));

  // With memories: one line per note on a person or bot, coloured by feeling, dashed when distant.
  memories = fresh(); await panel.open(); await tick(); await tick();
  assert.equal(feelings().length, 3, 'one line per note on a person or a bot, none for the room');
  const fond = lineTo('Ana'), wary = lineTo('Quill'), curious = lineTo('Pine');
  assert.equal(fond.props.stroke, '#b3261e'); assert.equal(fond.props['stroke-dasharray'], undefined);
  assert.equal(wary.props.stroke, '#3f6aa8'); assert.ok(wary.props['stroke-dasharray'], 'negative closeness is dashed');
  assert.equal(curious.props.stroke, '#2b8a8a'); assert.equal(curious.props['stroke-dasharray'], undefined);
  for (const l of feelings()) assert.ok(l.props.stroke && l.props['stroke-width'] && l.props['aria-label'], 'BT8: each line carries its own stroke, and a label');
  assert.equal(fond.props['aria-label'], 'Fern feels fond about Ana');
  assert.equal(wary.props['aria-label'], 'Fern feels wary about Quill, at a distance');
  assert.ok(text(body).includes('Feelings, warm to cool, dashed when distant'));
  // trust.css restyles lines by kind; no rule may paint over the colour a feeling line carries.
  const css = fs.readFileSync('web/trust.css', 'utf8');
  assert.ok(!/(^|[\s,}])\.trust-edge(\.feeling)?\s*\{[^}]*stroke\s*:/m.test(css), 'a stylesheet rule would recolour feeling lines');
  // Someone with no other link is added as a node, and Pine, a bot only known from a note, too.
  assert.ok(node('Quill'), 'an unknown subject is added'); assert.ok(node('Quill').props['aria-label'].includes('In Fern’s memories'));
  assert.ok(node('Pine'), 'a bot with no other link is added');
  assert.equal(nodes().filter(n => n.props['aria-label'].startsWith('Ana, ')).length, 1, 'Ana is not drawn twice');
  // Nodes already on screen barely move: the layout gives them a tenth of a new node's freedom.
  for (const [label, [x, y]] of Object.entries(at)) {
    const now = where()[label]; assert.ok(now, label);
    assert.ok(Math.hypot(now[0] - x, now[1] - y) < 24, `${label} moved from ${x},${y} to ${now}`);
  }

  // Picking another node shows the bot's note on it, as text.
  pick('Ana');
  assert.ok(text(body).includes('Fern’s note') && text(body).includes('Ana lends me paint.') && text(body).includes('Feels fond'));
  pick('Quill');
  assert.ok(text(body).includes(evil) && text(body).includes('Feels wary'));
  assert.equal(all(body).some(n => n.tag === 'img'), false, 'bot text is never markup');

  // Picking the bot shows when it slept, its notes and its newest moments first.
  pick('Fern');
  const shown = text(body);
  for (const words of ['What Fern remembers', 'Last slept', 'Ana lends me paint.', evil, 'Pine hums all day.', 'A quiet room.', 'Painted a fox.', 'Met Quill at the pier.']) assert.ok(shown.includes(words), words);
  assert.ok(shown.indexOf('Met Quill at the pier.') < shown.indexOf('Painted a fox.'), 'newest moment first');
  assert.equal(all(body).some(n => n.tag === 'img'), false);

  // Delete calls the owner's route for that memory, then reads them again.
  const deletes = () => all(body).filter(n => n.tag === 'button' && text(n) === 'Delete');
  assert.equal(deletes().length, 6, 'a delete button for each note and moment');
  requests.length = 0;
  await deletes().find(b => b.props['aria-label'] === 'Delete memory about Ana').props.onclick();
  assert.deepEqual(requests.map(r => [r.method || 'GET', r.path]), [['DELETE', '/api/agents/one/memories/n1'], ['GET', '/api/agents/one/memories']]);
  assert.ok(!text(body).includes('Ana lends me paint.'));
  assert.equal(feelings().length, 2); assert.equal(lineTo('Ana'), undefined);
  // And from a picked node.
  pick('Pine'); requests.length = 0;
  await all(body).find(n => n.tag === 'button' && text(n) === 'Delete note').props.onclick();
  assert.deepEqual(requests.map(r => [r.method || 'GET', r.path]), [['DELETE', '/api/agents/one/memories/n3'], ['GET', '/api/agents/one/memories']]);

  // Memories that become unreachable leave the graph as it was before them.
  memories = null; const plain = build(); await plain.open(); await tick(); await tick();
  assert.deepEqual(shape(), before);
  pick('Fern'); assert.ok(!text(body).includes('remembers'));
  // A network failure is the same.
  const failing = async (path, o) => { if (path.includes('/memories')) throw Error('Failed to fetch'); return api(path, o); };
  const down = context.trustPanel({h, api: failing, panels, me: {name: 'You', kind: 'person'}, crew: () => {}});
  await down.open(); await tick(); await tick();
  assert.deepEqual(shape(), before);

  // The real route's reply, when tests/memories_ui.rs passes one: the stub has the same fields, and
  // the panel draws the real reply the same way.
  const file = process.argv[2];
  if (file) {
    const real = JSON.parse(fs.readFileSync(file, 'utf8'));
    const {view} = real;
    const keys = value => Object.keys(value).sort();
    const stub = fresh();
    assert.deepEqual(keys(stub), keys(view), 'top-level fields');
    assert.deepEqual(keys(stub.bot), keys(view.bot));
    assert.equal(typeof view.slept_at, 'string', 'slept_at is an ISO time'); assert.ok(!Number.isNaN(Date.parse(view.slept_at)));
    const full = list => list.find(m => 'feeling' in m && 'closeness' in m);
    assert.deepEqual(keys(full(stub.notes)), keys(full(view.notes)), 'a note with a feeling');
    assert.deepEqual(keys(stub.notes[3]), keys(view.notes.find(m => m.subject.startsWith('room:'))), 'a note on a room');
    assert.deepEqual(keys(stub.moments[0]), keys(view.moments[0]), 'a moment');
    assert.deepEqual(keys(stub.edges[0]), keys(view.edges[0]), 'an edge');
    for (const e of view.edges) {
      assert.equal(e.from, real.bot, 'edges come from the bare bot id');
      assert.ok(view.notes.some(m => m.subject === `${e.to_kind === 'bot' ? 'agent' : 'person'}:${e.to}`), `edge to ${e.to} has its note`);
    }
    // Draw it: the real people, the real bot, and the real reply.
    data.owner_id = real.owner; data.people = real.people; data.bots = real.bots;
    data.agents = [{id: real.bot, name: view.bot.name, doorbell: {paused: false}, trust: policy()}]; data.friendships = [];
    const live = async (path, o = {}) => path === `/api/agents/${real.bot}/memories` ? JSON.parse(JSON.stringify(view)) : api(path, o);
    const shown = context.trustPanel({h, api: live, panels, me: {name: 'You', kind: 'person'}, crew: () => {}});
    await shown.open(); await tick(); await tick();
    assert.equal(feelings().length, view.edges.length, 'a line for each real edge');
    for (const e of view.edges) assert.ok(node(e.name), `${e.name} is in the graph`);
    for (const m of [...view.notes, ...view.moments]) assert.ok(text(body).includes(m.text), m.text);
    assert.ok(text(body).includes('Last slept'));
    assert.equal(all(body).some(n => n.tag === 'img'), false);
  }
  console.log('MM16: feelings on the trust graph passed');
})().catch(error => { console.error(error); process.exitCode = 1; });
