// Run from the repository root: node tests/easy_sign_up_ui.cjs
// A82 to A87 and E22: the shipped sign-up handlers from web/app.js, against a small DOM and a
// controlled server.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const source = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
function extract(name) {
  const found = source.match(new RegExp(`\\n((?:async )?function ${name}\\()`));
  assert.ok(found, `missing shipped function ${name}`);
  const start = found.index + 1;
  return source.slice(start, source.indexOf('\n}\n', start) + 2);
}
function constant(name) {
  const found = source.match(new RegExp(`\\nconst ${name} = .*;\\n`));
  assert.ok(found, `missing shipped constant ${name}`);
  return found[0].trim();
}

class Node {
  constructor(tag) {
    Object.assign(this, { tag, children: [], attrs: {}, listeners: {}, parent: null });
    Object.assign(this, { hidden: false, value: '', type: '', name: '', open: false, disabled: false, href: '', className: '', returnValue: '' });
  }
  get textContent() { return text(this); }
  set textContent(value) { this.children = [String(value)]; }
  append(...children) {
    for (const child of children) {
      if (child instanceof Node) child.parent = this;
      this.children.push(child);
    }
  }
  replaceChildren(...children) { this.children = []; this.append(...children); }
  remove() { if (this.parent) this.parent.children = this.parent.children.filter((c) => c !== this); this.parent = null; }
  addEventListener(kind, fn, options) { (this.listeners[kind] ||= []).push({ fn, once: options?.once }); }
  dispatch(kind, extra = {}) {
    const list = this.listeners[kind] || [];
    this.listeners[kind] = list.filter((l) => !l.once);
    return Promise.all(list.map((l) => l.fn({ target: this, currentTarget: this, preventDefault() {}, ...extra })));
  }
  setAttribute(key, value) { this.attrs[key] = String(value); }
  querySelector(selector) {
    if (selector === 'button[type=submit]') return all(this).find((n) => n.tag === 'button' && n.type === 'submit');
    return undefined;
  }
  focus() {}
  showModal() { this.open = true; }
  close(value) { if (value !== undefined) this.returnValue = value; this.open = false; this.dispatch('close'); }
  reportValidity() { return true; }
}
const all = (node) => (node instanceof Node ? [node, ...node.children.flatMap(all)] : []);
const text = (node) => (node instanceof Node ? node.children.map(text).join('') : String(node));
const find = (node, test) => all(node).find(test);
const button = (node, label) => find(node, (n) => (n.tag === 'button' || n.tag === 'a') && text(n) === label);
const labels = (node) => all(node).filter((n) => n !== node && (n.tag === 'button' || n.tag === 'a')).map(text);
const input = (node, name) => find(node, (n) => n.tag === 'input' && n.name === name);
// The owner key dialog shows the key in a read-only box.
const shownKey = (dialog) => find(dialog, (n) => n.tag === 'input').value;
const click = (node) => node.dispatch('click');
const submit = (form) => form.dispatch('submit');
// Objects from the page's realm compare by what they hold.
const same = (actual, expected, message) => assert.deepEqual(JSON.parse(JSON.stringify(actual)), expected, message);
const tick = () => new Promise((resolve) => setImmediate(resolve));
const body = new Node('body');
const dialogs = () => body.children.filter((n) => n.tag === 'dialog' && n.open);

class FormData {
  constructor(form) {
    this.list = all(form).filter((n) => n.tag === 'input' && n.name).map((n) => [n.name, n.value]);
  }
  [Symbol.iterator]() { return this.list[Symbol.iterator](); }
}

let sites = [];
let calls = [];
let browserAsked = 0;
let routed = 0;
let confirmed = true;
let storage = new Map();
let storageWorks = true;
let doorOpen = true;
const fail = (status, message = `HTTP ${status}`) => Object.assign(new Error(message), { status });
const context = {
  console,
  setTimeout,
  FormData,
  document: { createElement: (tag) => new Node(tag), body, title: '' },
  location: { pathname: '/h/f1', origin: 'https://example.test' },
  navigator: { clipboard: { writeText: async () => {} } },
  sessionStorage: { setItem: (k, v) => calls.push(['session', k, v]) },
  localStorage: {
    getItem: (k) => { if (!storageWorks) throw Error('blocked'); return storage.get(k) ?? null; },
    setItem: (k, v) => { if (!storageWorks) throw Error('blocked'); storage.set(k, v); },
  },
  confirm: () => confirmed,
  route: async () => { routed += 1; },
  passkeyStatus: async () => ({ usable: true }),
  registerPasskey: async (options) => { browserAsked += 1; calls.push(['register', options]); return { person: { id: 'p1', name: options.name }, owner_key: 'passkey-owner-key' }; },
  signInWithPasskey: async () => { browserAsked += 1; return { id: 'p2', name: 'Cy' }; },
  api: async (path, options = {}) => {
    calls.push(['api', path, options.method || 'GET', options.body]);
    if (path === '/api/sign-in') return { sites };
    if (path === '/api/people') return { person: { id: 'p3', name: options.body.your_name }, owner_key: 'name-owner-key' };
    if (path === '/api/key/rotate') return { owner_key: 'rotated-key' };
    if (path === '/api/session') return {};
    if (path === '/api/floors/f1/door') { if (!doorOpen) throw fail(404, 'Not found'); return { id: 'f1', name: 'Porch', space_name: 'Cafe' }; }
    if (path === '/api/floors/f1/walk-in') {
      if (options.body?.your_name) return { hangout_id: 'f1', until: 99, person: { id: 'p4' }, owner_key: 'door-owner-key' };
      return { hangout_id: 'f1', until: 99 };
    }
    throw Error(`Unexpected ${path}`);
  },
};
vm.createContext(context);
const names = ['h', 'modal', 'formError', 'submitter', 'field', 'act', 'copyButton', 'secretRow', 'showOwnerKey',
  'afterNewAccount', 'signUpByName', 'ownerKeyForm', 'signInChoices', 'accessCard', 'signInElsewhere',
  'onlyOwnerKeyNudge', 'ownerKeyControl', 'openRoom', 'doorView'];
vm.runInContext([
  ...['WAY_BACK', 'leaving', 'NEW_PERSON', 'NUDGE_SEEN', 'PASSKEY_EXPLAINER'].map(constant),
  ...names.map(extract),
  `Object.assign(globalThis, { ${names.join(', ')} });`,
].join('\n'), context);

const cardFor = (status, extra = {}) => context.accessCard({
  title: 'New here?',
  status,
  nameText: 'Make my account',
  byName: async (name) => calls.push(['byName', name]),
  onSignedIn: async (person) => calls.push(['signedIn', person]),
  onCreated: async (made) => calls.push(['created', made]),
  ...extra,
});

(async () => {
  // A82: the ways in are equal choices, and nothing asks the browser for a passkey until one is picked.
  sites = [{ site: 'discord', name: 'Discord' }, { site: 'github', name: 'GitHub' }];
  let card = cardFor({ usable: true });
  await tick();
  const ways = find(card, (n) => n.attrs['aria-label'] === 'Ways in');
  same(labels(ways), ['Sign up with just a name', 'Sign up with a passkey', 'Sign up with Discord', 'Sign up with GitHub']);
  assert.equal(button(ways, 'Sign up with Discord').href, '/api/sign-in/discord?new=true');
  assert.equal(browserAsked, 0, 'the card asked the browser for a passkey on its own');
  const form = find(card, (n) => n.tag === 'form' && n.className === 'create-account');
  assert.equal(form.hidden, true, 'a name field shows before anything is picked');
  const signBackIn = find(card, (n) => n.tag === 'details');
  assert.equal(signBackIn.open, false, 'signing back in isn\'t folded away');
  assert.ok(text(signBackIn).includes('Already have an account? Sign in'));
  assert.ok(button(signBackIn, 'Sign in with a passkey'));
  assert.ok(button(signBackIn, 'Sign in with GitHub'));
  assert.ok(input(signBackIn, 'owner_key'), 'no owner key sign-in');
  await click(button(card, 'Sign up with a passkey'));
  assert.equal(form.hidden, false);
  assert.equal(button(ways, 'Sign up with a passkey').attrs['aria-pressed'], 'true');
  assert.equal(button(ways, 'Sign up with just a name').attrs['aria-pressed'], 'false');
  assert.equal(browserAsked, 0, 'picking the passkey asks for the name first');
  input(form, 'your_name').value = 'Ann';
  await submit(form);
  same(calls.find((c) => c[0] === 'register'), ['register', { name: 'Ann' }]);
  assert.equal(calls.find((c) => c[0] === 'created')[1].owner_key, 'passkey-owner-key');
  // Nothing on the card asks for a passkey sign-in until that button is pressed.
  assert.equal(browserAsked, 1);
  await click(button(signBackIn, 'Sign in with a passkey'));
  assert.equal(browserAsked, 2);
  same(calls.at(-1), ['signedIn', { id: 'p2', name: 'Cy' }]);

  // A82: without passkeys there's no passkey choice and no passkey sign-in. The rest stays.
  sites = [];
  card = cardFor({ usable: false, reason: 'browser' });
  await tick();
  same(labels(find(card, (n) => n.attrs['aria-label'] === 'Ways in')), ['Sign up with just a name']);
  assert.equal(button(card, 'Sign in with a passkey'), undefined);
  assert.ok(input(card, 'owner_key'));
  // `signInOpen` unfolds signing back in.
  assert.equal(find(cardFor({ usable: true }, { signInOpen: true }), (n) => n.tag === 'details').open, true);

  // A83: just a name is the name and one button, and goes to whatever the page makes accounts with.
  calls = [];
  browserAsked = 0;
  card = cardFor({ usable: true });
  await click(button(card, 'Sign up with just a name'));
  const nameForm = find(card, (n) => n.tag === 'form' && n.className === 'create-account');
  assert.equal(text(button(nameForm, 'Make my account')), 'Make my account');
  input(nameForm, 'your_name').value = 'Bo';
  await submit(nameForm);
  same(calls.filter((c) => c[0] === 'byName'), [['byName', 'Bo']]);
  assert.equal(browserAsked, 0, 'just a name asked for a passkey');
  // A failure shows next to the form.
  const failing = cardFor({ usable: true }, { byName: async () => { throw fail(429, 'Too many new people from here.'); } });
  await click(button(failing, 'Sign up with just a name'));
  const failingForm = find(failing, (n) => n.tag === 'form' && n.className === 'create-account');
  await submit(failingForm);
  assert.ok(find(failingForm, (n) => n.className === 'error' && !n.hidden && text(n) === 'Too many new people from here.'));
  // On the map, it's POST /api/people.
  calls = [];
  let entered = 0;
  await context.signUpByName('Di', () => { entered += 1; });
  same(calls[0], ['api', '/api/people', 'POST', { your_name: 'Di' }]);

  // A84: the key, then I saved it or Skip for now, both only closing. No passkey offer follows.
  let dialog = dialogs().at(-1);
  assert.equal(shownKey(dialog), 'name-owner-key');
  assert.ok(button(dialog, 'I saved it'));
  await click(button(dialog, 'Skip for now'));
  await tick();
  assert.equal(entered, 1, 'Skip for now didn\'t carry on');
  assert.equal(dialogs().length, 0, 'something opened after the owner key');
  assert.equal(browserAsked, 0);
  context.afterNewAccount('key-two', () => { entered += 1; }, { site: 'Discord' });
  dialog = dialogs().at(-1);
  assert.ok(button(dialog, 'Skip for now'));
  await click(button(dialog, 'I saved it'));
  await tick();
  assert.equal(entered, 2);
  assert.equal(dialogs().length, 0);
  assert.ok(!source.includes('offerPasskey'), 'the passkey offer after a new account is back');
  // A replaced key isn't a new account, so it has no Skip.
  context.showOwnerKey('key-three', () => {});
  assert.equal(button(dialogs().at(-1), 'Skip for now'), undefined);
  dialogs().at(-1).close();

  // A85: only the owner key gets a note in Account, naming what can be added, until Not now.
  const linkable = [{ site: 'discord', name: 'Discord', linked: null }, { site: 'github', name: 'GitHub', linked: null }];
  let nudge = context.onlyOwnerKeyNudge({ passkeys: 0, sign_in: linkable }, { usable: true });
  assert.equal(text(nudge.children[0]), 'Add a passkey or link Discord or GitHub so you can sign in on your other devices.');
  assert.equal(nudge.tag, 'div', 'the note is a dialog');
  assert.equal(text(context.onlyOwnerKeyNudge({ passkeys: 0, sign_in: linkable }, { usable: false }).children[0]),
    'Link Discord or GitHub so you can sign in on your other devices.');
  assert.equal(text(context.onlyOwnerKeyNudge({ passkeys: 0, sign_in: [] }, { usable: true }).children[0]),
    'Add a passkey so you can sign in on your other devices.');
  assert.equal(context.onlyOwnerKeyNudge({ passkeys: 0, sign_in: [] }, { usable: false }), null, 'nothing to add, but a note');
  assert.equal(context.onlyOwnerKeyNudge({ passkeys: 1, sign_in: linkable }, { usable: true }), null);
  assert.equal(context.onlyOwnerKeyNudge({ passkeys: 0, sign_in: [{ ...linkable[0], linked: 'ann' }] }, { usable: true }), null);
  const holder = new Node('div');
  holder.append(nudge);
  await click(button(nudge, 'Not now'));
  assert.equal(holder.children.length, 0, 'Not now left it there');
  assert.equal(context.onlyOwnerKeyNudge({ passkeys: 0, sign_in: linkable }, { usable: true }), null, 'Not now didn\'t stick');
  // Storage that throws shows the note every time, and Not now still hides it.
  storageWorks = false;
  nudge = context.onlyOwnerKeyNudge({ passkeys: 0, sign_in: linkable }, { usable: true });
  holder.append(nudge);
  await click(button(nudge, 'Not now'));
  assert.equal(holder.children.length, 0);
  storageWorks = true;

  // A87: one control replaces the key, asking first. A site's session isn't offered it (A54).
  calls = [];
  let control = context.ownerKeyControl({ passkeys: 0, sign_in_session: false });
  confirmed = false;
  await click(button(control, 'Replace owner key'));
  assert.equal(calls.length, 0, 'it replaced the key without asking');
  confirmed = true;
  await click(button(control, 'Replace owner key'));
  same(calls[0], ['api', '/api/key/rotate', 'POST', null]);
  assert.equal(shownKey(dialogs().at(-1)), 'rotated-key');
  dialogs().at(-1).close();
  control = context.ownerKeyControl({ passkeys: 0, sign_in_session: true });
  assert.equal(button(control, 'Replace owner key'), undefined);
  assert.ok(text(control).includes('sign in with a passkey or your owner key'));
  assert.equal(source.split("'Replace owner key'").length - 1, 1, 'Replace owner key is built in more than one place');
  const parts = source.slice(source.indexOf('async function homeParts('));
  assert.ok(parts.slice(0, parts.indexOf('\n}\n')).includes('ownerKeyControl(me)'), 'Account lost the control');
  assert.ok(source.includes('const account = accountPanel(panels, me, parts);'), 'rooms lost the Account panel');

  // A86: signed out at a public floor's link, the door, then a name walks them in.
  calls = [];
  routed = 0;
  const signedOut = () => Promise.reject(fail(401, 'Sign in first'));
  let opened = await context.openRoom('f1', signedOut);
  const door = opened.door;
  assert.ok(door, 'no door for a public floor');
  assert.equal(text(find(door, (n) => n.tag === 'h1')), 'Cafe: Porch');
  const doorForm = find(door, (n) => n.tag === 'form');
  assert.ok(button(doorForm, 'Walk in'));
  input(doorForm, 'your_name').value = 'Eve';
  await submit(doorForm);
  same(calls.find((c) => c[1] === '/api/floors/f1/walk-in'), ['api', '/api/floors/f1/walk-in', 'POST', { your_name: 'Eve' }]);
  dialog = dialogs().at(-1);
  assert.equal(shownKey(dialog), 'door-owner-key');
  assert.equal(routed, 0, 'it went in before the owner key');
  await click(button(dialog, 'Skip for now'));
  await tick();
  assert.equal(routed, 1, 'the owner key didn\'t lead into the room');
  // Already have an account: the sign-in choices are there, and they come back to the same room.
  const signInPart = find(door, (n) => n.className === 'quiet');
  assert.equal(signInPart.hidden, true);
  await click(button(door, 'Sign in'));
  assert.equal(signInPart.hidden, false);
  const keyForm = find(signInPart, (n) => n.tag === 'form');
  input(keyForm, 'owner_key').value = 'old-key';
  await submit(keyForm);
  same(calls.find((c) => c[1] === '/api/session'), ['api', '/api/session', 'POST', { owner_key: 'old-key' }]);
  assert.equal(routed, 2, 'signing in with the owner key didn\'t come back to the room');
  await click(button(signInPart, 'Sign in with a passkey'));
  assert.equal(routed, 3, 'signing in with a passkey didn\'t come back to the room');
  // A door that isn't there leaves the 401, which takes them to the map as before.
  doorOpen = false;
  await assert.rejects(context.openRoom('f1', signedOut), (error) => error.status === 401);
  doorOpen = true;

  // E22: someone signed in walks straight in, with no warning about bots first.
  calls = [];
  const before = body.children.length;
  let loads = 0;
  opened = await context.openRoom('f1', async () => {
    loads += 1;
    if (loads === 1) throw fail(404, 'Not found');
    return { hangout: { name: 'Porch' } };
  });
  same(opened.snapshot, { hangout: { name: 'Porch' } });
  same(calls, [['api', '/api/floors/f1/walk-in', 'POST', null]]);
  assert.equal(body.children.length, before, 'walking in opened a dialog');
  assert.ok(!source.includes('Before you enter this public room'), 'the bot warning is back on walking in');
  // The warning is where a bot is sent or brought into a public room.
  for (const name of ['sendBot', 'bringBot']) {
    const start = source.indexOf(`  async function ${name}(`);
    const fn = source.slice(start, source.indexOf('\n  }\n', start));
    assert.ok(fn.includes('can cost money'), `${name} lost the warning`);
  }

  console.log('A82 to A87 and E22 sign-up handlers passed');
})().catch((error) => { console.error(error); process.exitCode = 1; });
