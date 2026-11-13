// node tests/mayor_panel_ui.cjs: MY1 to MY11, #639, #1158. The Mayor voting box's panel shows the
// race with no totals, people/bots split, votes from this room, changes a vote, stands and
// withdraws, and shows a refusal in the server's words. Last week's mayor, when a person, is the
// one their card calls Mayor.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
class Node {
  constructor(tag, props = {}, children = []) {
    this.tag = tag; this.props = { ...props }; this.children = [];
    Object.assign(this, props); this.append(...children);
  }
  append(...c) { this.children.push(...c.flat(Infinity).filter((x) => x != null && x !== false)); }
  replaceChildren(...c) { this.children = []; this.append(...c); }
}
const all = (n) => (n instanceof Node ? [n, ...n.children.flatMap(all)] : []);
const text = (n) => (n instanceof Node ? n.children.map(text).join('') : String(n));
const h = (tag, props, ...children) => new Node(tag, props, children);
const formError = () => h('p', { class: 'error', hidden: true });
const tick = () => new Promise((r) => setImmediate(r));

function load(state) {
  const calls = [];
  const api = async (path, options = {}) => {
    calls.push({ path, method: options.method || 'GET', body: options.body });
    if (path === '/api/mayor' && (options.method || 'GET') === 'GET') return JSON.parse(JSON.stringify(state));
    if (state.refuse) { const e = new Error(state.refuse); e.status = 403; throw e; }
    if (path === '/api/mayor/ballot') state.your_ballot = { kind: options.body.kind, id: options.body.id };
    if (path === '/api/mayor/stand') state.you_stand = options.method === 'POST';
    if (path === '/api/mayor/candidates') state.candidates.push({ kind: 'bot', id: options.body.id, name: 'Scout' });
    return { ok: true };
  };
  const context = { h, api, formError };
  vm.createContext(context);
  context.botsHere = () => [{ id: 'b1', name: 'Eagle1' }, { id: 'b2', name: 'Scout' }];
  const start = app.indexOf('function mayorPanel(roomId');
  const end = app.indexOf('\n}\n', app.indexOf('function mayorPersonId() {')) + 3;
  vm.runInContext(app.slice(start, end) + '\nthis.mayorPanel = mayorPanel; this.mayorPersonId = mayorPersonId;', context);
  return { context, calls };
}
const button = (root, label) => all(root).find((n) => n.tag === 'button' && text(n) === label);

(async () => {
  const state = {
    closes_local: 'Saturday 1:00 PM Central', voted: 1, voted_people: 1, voted_bots: 0,
    candidates: [{ kind: 'person', id: 'p1', name: 'Ann' }, { kind: 'bot', id: 'b1', name: 'Eagle1' }],
    your_ballot: null, you_stand: false,
    last_week: { mayor: { kind: 'person', id: 'p9', name: 'Zed' }, votes: 4, people_votes: 3, bot_votes: 1, tie_draw: false },
    note: 'Ballots are secret: totals show once the week closes.',
  };
  const { context, calls } = load(state);
  const root = context.mayorPanel('room-1');
  await tick();
  assert.match(text(root), /Mayor this week: Zed \(4 votes: 3 people, 1 bot\)/);
  assert.match(text(root), /Voting closes Saturday 1:00 PM Central\. 1 ballot has been cast so far \(1 person, 0 bots\)/);
  assert.match(text(root), /Eagle1 \(bot\)/);
  assert.match(text(root), /Ballots are secret/);

  // Voting sends the candidate and this room, then shows it as the vote, and lets them change it.
  await button(root, 'Vote').onclick({ currentTarget: {} });
  await tick();
  const cast = calls.find((c) => c.path === '/api/mayor/ballot');
  assert.equal(cast.method, 'PUT');
  assert.equal(JSON.stringify(cast.body), JSON.stringify({ kind: 'person', id: 'p1', hangout_id: 'room-1' }));
  assert.match(text(root), /Ann\s*Your vote/);
  assert.ok(button(root, 'Change to them'), 'the other candidate can be chosen instead');

  // Stand, then withdraw.
  await button(root, 'Stand for mayor').onclick({ currentTarget: {} });
  await tick();
  assert.ok(calls.some((c) => c.path === '/api/mayor/stand' && c.method === 'POST'));
  await button(root, 'Withdraw').onclick({ currentTarget: {} });
  await tick();
  assert.ok(calls.some((c) => c.path === '/api/mayor/stand' && c.method === 'DELETE'));

  // A refusal is shown in the server's words.
  state.refuse = 'vote from a public floor you are on';
  await button(root, 'Change to them').onclick({ currentTarget: {} });
  await tick();
  const error = all(root).find((n) => n.props.class === 'error');
  assert.equal(error.hidden, false);
  assert.equal(error.textContent, 'vote from a public floor you are on');

  // Nobody standing, no mayor.
  const empty = load({ ...state, refuse: null, candidates: [], last_week: null, voted: 0, voted_people: 0, voted_bots: 0 });
  const quiet = empty.context.mayorPanel('room-1');
  await tick();
  assert.match(text(quiet), /No mayor this week\./);
  assert.match(text(quiet), /Nobody is standing yet\./);
  assert.match(text(quiet), /0 ballots have been cast so far \(0 people, 0 bots\)/);

  // MY9: someone with god mode on gets a picker of the bots the server says it will accept.
  const god = load({ ...state, refuse: null, can_nominate: true, candidates: [{ kind: 'bot', id: 'b1', name: 'Eagle1' }], nominatable: [{ kind: 'bot', id: 'b2', name: 'Scout' }] });
  const godPanel = god.context.mayorPanel('room-1', god.context.botsHere);
  await tick();
  const select = all(godPanel).find((n) => n.tag === 'select');
  assert.deepEqual(select.children.map((o) => o.value), ['b2'], 'only the eligible bot is offered');
  select.value = 'b2';
  await button(godPanel, 'Nominate a bot').onclick({ currentTarget: {} });
  await tick();
  const nominated = god.calls.find((c) => c.path === '/api/mayor/candidates');
  assert.equal(JSON.stringify(nominated.body), JSON.stringify({ kind: 'bot', id: 'b2' }));
  assert.match(text(godPanel), /Scout \(bot\)/);
  // Nobody eligible: a line saying so, not an empty picker.
  const none = load({ ...state, refuse: null, can_nominate: true, candidates: [], nominatable: [] });
  const nonePanel = none.context.mayorPanel('room-1', none.context.botsHere);
  await tick();
  assert.equal(all(nonePanel).some((n) => n.tag === 'select'), false);
  assert.match(text(nonePanel), /No bot is out on a public floor right now/);
  // Without can_nominate, no picker.
  assert.equal(all(root).some((n) => n.tag === 'select'), false);

  // The badge: a person mayor's id, asked once; a bot mayor badges no person.
  const gets = () => calls.filter((c) => c.path === '/api/mayor' && c.method === 'GET').length;
  assert.equal(await context.mayorPersonId(), 'p9');
  const once = gets();
  assert.equal(await context.mayorPersonId(), 'p9');
  assert.equal(gets(), once, 'asked once a page');
  const botMayor = load({ ...state, last_week: { mayor: { kind: 'bot', id: 'b1', name: 'Eagle1' }, votes: 2 } });
  assert.equal(await botMayor.context.mayorPersonId(), null);
  console.log('mayor panel ui ok');
})().catch((error) => { console.error(error); process.exit(1); });
