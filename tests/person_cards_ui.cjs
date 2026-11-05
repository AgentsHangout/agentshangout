// Run from the repository root: node tests/person_cards_ui.cjs
// A88 to A92: the person card in a room, and its preview in Settings, driven through the shipped
// functions of web/app.js with a small DOM stand-in.
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

function element(tag, props = {}, ...children) {
  return {
    tag,
    props: props || {},
    children: children.flat(Infinity).filter((c) => c != null),
    removed: false,
    style: {},
    offsetWidth: 100,
    offsetHeight: 60,
    append(...more) { this.children.push(...more); },
    replaceChildren(...more) { this.children = more.flat(); },
    remove() { this.removed = true; },
    focus() {},
    contains(other) { return other === this || this.children.some((c) => c && c.contains && c.contains(other)); },
    querySelector(selector) {
      const walk = (node) => {
        for (const child of node.children || []) {
          if (child && child.tag === selector) return child;
          const found = child && child.children && walk(child);
          if (found) return found;
        }
        return null;
      };
      return walk(this);
    },
  };
}
const same = (a, b, message) => assert.equal(JSON.stringify(a), JSON.stringify(b), message);
const textOf = (node) => (node == null ? '' : typeof node === 'string' ? node : (node.children || []).map(textOf).join(' '));
const buttons = (node) => (node.children || []).flatMap((c) => (c && (c.tag === 'button' || c.tag === 'a') ? [c] : c && c.children ? buttons(c) : []));

const calls = [];
let publicData = {};
const timers = [];
const documentListeners = {};
const context = {
  h: element,
  api: async (path) => {
    calls.push(path);
    const data = publicData[path.split('/')[3]];
    if (data === 'fail') throw Error('nope');
    return data;
  },
  setTimeout: (fn) => { timers.push(fn); return timers.length; },
  clearTimeout: () => {},
  Date,
  innerWidth: 1000,
  innerHeight: 800,
  encodeURIComponent,
  document: {
    addEventListener: (kind, fn) => { (documentListeners[kind] ||= []).push(fn); },
    removeEventListener: (kind, fn) => { documentListeners[kind] = (documentListeners[kind] || []).filter((f) => f !== fn); },
  },
};
vm.createContext(context);
vm.runInContext([
  extract('githubCardRows'), extract('githubProfileUrl', 'const'), extract('personCardBody'),
  extract('researchRepositoryLink'), extract('researchMenuLink'), extract('popupMenu'), extract('personCards'),
  'this.popupMenu = popupMenu; this.githubCardRows = githubCardRows; this.githubProfileUrl = githubProfileUrl; this.personCardBody = personCardBody; this.personCards = personCards;',
].join('\n'), context);

// What the card says: only sections the public response carries (A88).
same(context.githubCardRows({ linked: true, login: 'ann' }), [], 'no sections on: nothing');
same(context.githubCardRows({ linked: false }), []);
same(context.githubCardRows(null), []);
const stars = context.githubCardRows({ linked: true, stars: { total: 12 } });
same(stars.map((r) => r[0]), ['Stars received']);
const all = context.githubCardRows({
  linked: true,
  repos: [{ name: 'alpha', stargazers_count: 3 }],
  stars: { total: 3 },
  languages: [{ language: 'Rust', count: 2 }],
  badge: { merged_prs: 4 },
});
same(all.map((r) => r[0]), ['Public repos', 'Stars received', 'Top languages', 'Merged pull requests']);
assert.ok(!/discord/i.test(JSON.stringify(all)), 'no Discord line unless the person turned it on (A93)');
const withDiscord = context.githubCardRows({ linked: true, stars: { total: 1 }, discord: { name: 'ann#1', id: '99887766' } });
same(withDiscord, [['Stars received', '1'], ['Discord', 'ann#1']]);
same(context.githubCardRows({ linked: false, discord: { name: 'ann#1' } }), [['Discord', 'ann#1']], 'Discord alone, GitHub not shown');
assert.ok(!JSON.stringify(withDiscord).includes('99887766'), 'the id is never drawn');
// Text, never HTML (A71): a hostile repo name stays a string.
const hostile = context.githubCardRows({ linked: true, repos: [{ name: '<img src=x onerror=alert(1)>', stargazers_count: 1 }] });
assert.equal(typeof hostile[0][1], 'string');
assert.equal(context.githubProfileUrl('ann-codes'), 'https://github.com/ann-codes');
assert.equal(context.githubProfileUrl('../evil'), null);
assert.equal(context.githubProfileUrl('a b'), null);
assert.equal(context.githubProfileUrl(undefined), null);
assert.match(textOf(context.personCardBody(null)), /Nothing shared/);
assert.match(textOf(context.personCardBody(undefined)), /Loading/);

// The room: hover, press, cache.
const listeners = {};
const stage = {
  classList: { toggle() {} },
  addEventListener: (kind, fn) => { (listeners[kind] ||= []).push(fn); },
};
const root = element('main');
let locked = false;
const mentioned = [];
const room = { personAt: () => null };
const you = 'p-me';
const cards = context.personCards({
  room,
  stage,
  root,
  locked: () => locked,
  mention: (name) => mentioned.push(name),
  view: () => ({ you: { person_id: you }, people: [{ id: 'p-ann', name: 'Ann' }, { id: 'p-bob', name: 'Bob' }, { id: you, name: 'Me' }] }),
});
const fire = (kind, id) => {
  const event = {
    clientX: 50,
    clientY: 60,
    pointerType: 'mouse',
    defaultPrevented: false,
    stopped: false,
    target: { closest: (sel) => (sel === 'button, a' ? null : id ? { dataset: { person: id } } : null) },
    preventDefault() { this.defaultPrevented = true; },
    stopPropagation() { this.stopped = true; },
  };
  for (const fn of listeners[kind] || []) fn(event);
  return event;
};
const flush = async () => {
  while (timers.length) timers.shift()();
  await new Promise((resolve) => setImmediate(resolve));
};
const card = () => root.children.filter((c) => !c.removed).at(-1);

(async () => {
  publicData = {
    'p-ann': { linked: true, login: 'ann-codes', stars: { total: 7 }, badge: { merged_prs: 2 } },
    'p-bob': { linked: false },
    'p-me': { linked: true, login: '../x', stars: { total: 1 } },
  };
  // Hover waits, then shows the card; no actions, and it says what a click does.
  fire('pointermove', 'p-ann');
  assert.equal(card(), undefined, 'no card before the pointer has rested');
  await flush();
  assert.ok(card(), 'hover shows a card');
  assert.match(textOf(card()), /Ann/);
  assert.match(textOf(card()), /Stars received/);
  assert.match(textOf(card()), /Merged pull requests/);
  assert.doesNotMatch(textOf(card()), /Public repos|Top languages|Discord/, 'sections left off stay off');
  assert.match(textOf(card()), /Click for options/);
  assert.equal(buttons(card()).length, 0, 'hover has no actions');
  same(calls, ['/api/people/p-ann/github']);

  // Leaving and coming back inside five minutes asks nothing new (cached).
  fire('pointermove', null);
  assert.equal(card(), undefined);
  fire('pointermove', 'p-ann');
  await flush();
  assert.equal(calls.length, 1, 'hovering the same person again is cached');
  // Someone else is another request, and someone with nothing public gets a plain note.
  fire('pointermove', 'p-bob');
  await flush();
  assert.equal(calls.length, 2);
  assert.match(textOf(card()), /Nothing shared/);

  // Left press and right press open the same card with the same actions.
  const left = fire('click', 'p-ann');
  await flush();
  assert.ok(left.defaultPrevented && left.stopped, 'a press on a person is not also a press on what is behind');
  const leftLabels = buttons(card()).map((b) => textOf(b));
  same(leftLabels, ['Mention in chat', 'View on GitHub']);
  const leftText = textOf(card());
  const right = fire('contextmenu', 'p-ann');
  await flush();
  assert.ok(right.defaultPrevented, 'right-click replaces the browser menu on a person');
  same(buttons(card()).map((b) => textOf(b)), leftLabels);
  assert.equal(textOf(card()), leftText);
  assert.equal(calls.length, 2, 'pressing reuses the cache too');
  // The actions do what they say.
  buttons(card())[0].props.onclick();
  same(mentioned, ['Ann']);
  fire('click', 'p-ann');
  await flush();
  const link = buttons(card())[1];
  assert.equal(link.tag, 'a');
  assert.equal(link.props.href, 'https://github.com/ann-codes');
  assert.equal(link.props.rel, 'noopener noreferrer');

  // Your own card: no mention of yourself, and an unsafe login never becomes a link.
  fire('click', you);
  await flush();
  same(buttons(card()).map((b) => textOf(b)), []);
  assert.match(textOf(card()), /Nothing else to do/);

  // A failed lookup is a card with nothing on it, never an error or a made-up count.
  publicData['p-bob'] = 'fail';
  cards.close();
  const failListeners = {};
  const failing = context.personCards({
    room,
    stage: { classList: { toggle() {} }, addEventListener: (kind, fn) => { (failListeners[kind] ||= []).push(fn); } },
    root,
    locked: () => false,
    mention() {},
    view: () => ({ you: { person_id: you }, people: [{ id: 'p-bob', name: 'Bob' }] }),
  });
  failListeners.click[0]({
    clientX: 1,
    clientY: 1,
    target: { closest: (sel) => (sel === 'button, a' ? null : { dataset: { person: 'p-bob' } }) },
    preventDefault() {},
    stopPropagation() {},
  });
  await flush();
  assert.match(textOf(card()), /Nothing shared/);
  failing.close();

  // Build mode and a thing in hand leave people alone.
  cards.close();
  locked = true;
  const before = calls.length;
  const ignored = fire('click', 'p-ann');
  fire('pointermove', 'p-ann');
  await flush();
  assert.equal(ignored.defaultPrevented, false);
  assert.equal(card(), undefined);
  assert.equal(calls.length, before);
  console.log('Person cards: only chosen sections; hover cached; left and right press agree; no Discord; locked in build mode.');
})().catch((error) => { console.error(error); process.exitCode = 1; });
