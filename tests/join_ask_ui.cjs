// PJ19, PJ20, PJ24: the shipped Ask to join button, in its three states, and where it posts.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const source = fs.readFileSync('web/app.js', 'utf8');
const start = source.indexOf('  function joinAskButton(');
assert.ok(start >= 0);
const code = source.slice(start, source.indexOf('\n  }\n', start) + 5);
const calls = [];
let reloads = 0;
let opened = null;
const h = (tag, props = {}, ...children) => ({tag, props, children: children.flat(Infinity).filter(x => x != null)});
const context = {
  h, id: 'floor 1', snapshot: null,
  formError: () => h('p'),
  modal: (title, ...content) => (opened = {title, content, closed: false, close() { this.closed = true; }}),
  submitter: (_error, action) => (values) => action(values),
  api: async (path, options) => { calls.push({path, ...options}); },
  reload: async () => { reloads += 1; },
};
vm.createContext(context);
vm.runInContext(code, context);
const all = node => node && typeof node === 'object' ? [node, ...(node.children || []).flatMap(all)] : [];
const text = node => typeof node === 'object' ? node.children.map(text).join(' ') : String(node);
const guest = (extra = {}) => ({guest: {since: 1, until: 2}, can_ask_to_join: true, you: {person_id: 'p1'}, floor: {floor: {access: 'public'}}, ...extra});
(async () => {
  // A member, or a bot's page: no button.
  context.snapshot = {...guest(), guest: null, can_ask_to_join: false};
  assert.equal(context.joinAskButton(), null);
  context.snapshot = guest({you: {person_id: 'p1', agent_id: 'a1'}});
  assert.equal(context.joinAskButton(), null);
  // An owner watching a bot they sent in is a guest, but didn't walk in, so can't ask.
  context.snapshot = guest({can_ask_to_join: false});
  assert.equal(context.joinAskButton(), null);

  // A guest with no request can ask, with a note of at most 200 characters.
  context.snapshot = guest();
  const ask = context.joinAskButton();
  assert.equal(text(ask), 'Ask to join');
  assert.ok(!ask.props.disabled);
  ask.props.onclick();
  assert.equal(opened.title, 'Ask to join');
  const form = opened.content.flatMap(all).find(n => n.tag === 'form');
  const note = all(form).find(n => n.tag === 'input');
  assert.equal(note.props.maxLength, 200);
  assert.ok(!note.props.required, 'the note is optional');
  await form.props.onsubmit({note: 'I fix docs'});
  assert.equal(calls.at(-1).path, '/api/floors/floor%201/join_requests');
  assert.equal(calls.at(-1).method, 'POST');
  assert.equal(calls.at(-1).body.note, 'I fix docs');
  assert.ok(opened.closed);
  assert.equal(reloads, 1);

  // PJ20: while it waits, the button says so and can't be pressed.
  context.snapshot = guest({join_request: {status: 'waiting', asked_at: 5}});
  const waiting = context.joinAskButton();
  assert.equal(text(waiting), 'Asked to join');
  assert.equal(waiting.props.disabled, true);
  assert.equal(waiting.props.onclick, undefined);

  // PJ24: after a no the asker is told, and there is nothing to press.
  context.snapshot = guest({join_request: {status: 'declined', asked_at: 5}});
  const declined = context.joinAskButton();
  assert.equal(declined.tag, 'small');
  assert.ok(text(declined).includes('declined'));
  assert.equal(all(declined).find(n => n.tag === 'button'), undefined);

  // PJ24: after 7 days without an answer the request expires. The asker is told, and may ask again.
  context.snapshot = guest({join_request: {status: 'expired', asked_at: 5}});
  const expired = context.joinAskButton();
  assert.ok(text(expired).includes('expired'));
  const expiredAsk = all(expired).find(n => n.tag === 'button');
  assert.ok(expiredAsk);
  assert.equal(text(expiredAsk), 'Ask to join');
  assert.ok(!expiredAsk.props.disabled);
  console.log('Ask to join button passed');
})().catch(error => { console.error(error); process.exitCode = 1; });
