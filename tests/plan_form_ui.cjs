// node tests/plan_form_ui.cjs: F56, proposing a plan says it's held here, asks for an off-site place
// only if needed, and ends on inviting people with the plan's link. It runs the shipped proposeHangout
// and sharePlan against stubs, not just their text.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
const slice = (from, end) => { const start = app.indexOf(from); assert.ok(start >= 0, from); return app.slice(start, app.indexOf(end, start) + end.length); };
const proposeCode = slice('  function proposeHangout() {', '\n  }\n');
const shareCode = slice('async function sharePlan(', '\n}\n');

class Node {
  constructor(tag, props = {}, children = []) {
    this.tag = tag; this.props = props; this.children = []; this.hidden = Boolean(props.hidden); this.value = props.value || '';
    Object.assign(this, props); this.append(...children);
  }
  append(...c) { this.children.push(...c.flat(Infinity).filter((x) => x != null && x !== false)); }
  replaceChildren(...c) { this.children = []; this.append(...c); }
  focus() {}
}
const all = (n) => (n instanceof Node ? [n, ...n.children.flatMap(all)] : []);
const text = (n) => (n instanceof Node ? n.children.map(text).join('') : String(n));
const h = (tag, props, ...children) => new Node(tag, props || {}, children);

function run({ agent = false } = {}) {
  const calls = [];
  const dialogs = [];
  const context = {
    h, id: 'room1',
    snapshot: { hangout: { name: "Ann's Cafe" }, you: { agent_id: agent ? 'b1' : null } },
    formError: () => h('p', { class: 'error', hidden: true }),
    field: (label, input) => h('label', {}, label, input),
    modal: (title, ...content) => { const d = { title, content, closed: false, close() { this.closed = true; } }; dialogs.push(d); return d; },
    submitter: (_err, action) => action,
    secretRow: (url) => h('code', {}, url),
    act: (_err, fn) => fn,
    reload: async () => {},
    say: () => {},
    api: async (path, options = {}) => {
      calls.push({ path, body: options.body, method: options.method });
      if (path === '/api/hangouts/room1/events') return { id: 'plan9', title: options.body.title };
      if (path === '/api/events/plan9/invite') return { invite_url: 'https://x/p/abc' };
      return {};
    },
  };
  vm.createContext(context);
  vm.runInContext(`${shareCode}\n${proposeCode}\nthis.proposeHangout = proposeHangout;`, context);
  context.proposeHangout();
  return { context, calls, dialogs };
}

(async () => {
  // The form says where it's held, and Somewhere else starts hidden behind a link.
  const { calls, dialogs } = run();
  const form = dialogs[0].content.find((c) => c instanceof Node && c.tag === 'form');
  assert.equal(dialogs[0].title, 'Propose a hangout');
  assert.ok(text(form).includes("Here, in Ann's Cafe."), text(form));
  const box = all(form).find((n) => n.props.name === 'location');
  const link = all(form).find((n) => n.tag === 'button' && text(n) === 'Somewhere else?');
  assert.equal(box.hidden, true, 'Somewhere else starts hidden');
  link.props.onclick();
  assert.equal(box.hidden, false, 'the link opens it');
  assert.equal(link.hidden, true, 'and goes away');

  // Submitting with it empty: no location of its own, which means here; then Invite people opens.
  await form.props.onsubmit({ title: 'Tacos', starts_at: '2026-11-07T18:00', duration_minutes: '120', location: '', notes: '' });
  const made = calls.find((c) => c.path === '/api/hangouts/room1/events');
  assert.equal(made.body.location, '', 'empty means here');
  assert.equal(dialogs[0].closed, true);
  await new Promise((r) => setImmediate(r));
  assert.ok(calls.some((c) => c.path === '/api/events/plan9/invite' && c.method === 'POST'), 'the plan link is made');
  const invite = dialogs[1];
  assert.equal(invite.title, 'Invite people');
  assert.ok(invite.content.some((c) => c instanceof Node && text(c).includes('everyone in this room can see it')));
  assert.ok(invite.content.some((c) => c instanceof Node && text(c).includes('https://x/p/abc')));

  // Somewhere else, filled in, becomes the location.
  const second = run();
  const form2 = second.dialogs[0].content.find((c) => c instanceof Node && c.tag === 'form');
  await form2.props.onsubmit({ title: 'Hike', starts_at: '2026-11-08T09:00', duration_minutes: '180', location: 'Trailhead lot', notes: '' });
  assert.equal(second.calls[0].body.location, 'Trailhead lot');

  // A bot proposes, and gets no link (F8).
  const bot = run({ agent: true });
  const form3 = bot.dialogs[0].content.find((c) => c instanceof Node && c.tag === 'form');
  await form3.props.onsubmit({ title: 'Sync', starts_at: '2026-11-09T10:00', duration_minutes: '60', location: '', notes: '' });
  await new Promise((r) => setImmediate(r));
  assert.ok(!bot.calls.some((c) => c.path.endsWith('/invite')), 'no link for a bot');
  assert.equal(bot.dialogs.length, 1);

  // The old way still works: from a plan's card it's "Invite a friend".
  await second.context.sharePlan({ id: 'plan9' });
  assert.equal(second.dialogs.at(-1).title, 'Invite a friend');
  console.log('Plan form: held here, optional Somewhere else, then Invite people with the link (run, not just read).');
})().catch((error) => { console.error(error); process.exitCode = 1; });
