// node tests/bot_signups_ui.cjs: A114, #628. The god-mode panel for bot sign-ups shows only while god
// mode is on, says who last set the switch, when, and the door's count for the day, and flipping it
// asks first, then calls the endpoint and shows the server's answer.
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
const tick = () => new Promise((r) => setImmediate(r));
// The shipped code between two markers in web/app.js.
const slice = (from, to) => {
  const start = app.indexOf(from);
  assert.ok(start >= 0, `web/app.js has ${from}`);
  return app.slice(start, app.indexOf(to, start) + to.length);
};

function load(state) {
  const calls = [], asked = [];
  let answer = true;
  const api = async (path, options = {}) => {
    calls.push({ path, method: options.method || 'GET', body: options.body });
    assert.equal(path, '/api/god-mode/bot-signups');
    if (state.refuse) { const e = new Error(state.refuse); e.status = 403; throw e; }
    if (options.method === 'PUT') Object.assign(state, { open: options.body.open, set_by_name: 'Morg', set_at: 1_800_000_600 });
    return JSON.parse(JSON.stringify(state));
  };
  // A stand-in for createPanels: one panel open at a time, and onOpen runs when it opens.
  const panels = {
    current: null, added: {},
    add(name, title, options) { const body = h('div', {}); this.added[name] = { title, options, body }; return body; },
    open(name) { this.current = name; this.added[name].options.onOpen?.(); },
    close() { this.current = null; },
  };
  const context = { h, api, panels, confirm: (q) => { asked.push(q); return answer; } };
  vm.createContext(context);
  vm.runInContext([
    slice('function formError() {', '\n}\n'),
    slice('function act(errorEl, run) {', '\n}\n'),
    slice('const dayFormat = ', ';\n'),
    slice('const timeFormat = ', ';\n'),
    slice('const fromUnix = ', ';\n'),
    slice('const whenText = ', ';\n'),
    slice('function botSignupsPanel(panels) {', '\n}\n'),
    'this.botSignupsPanel = botSignupsPanel; this.whenText = whenText;',
  ].join('\n'), context);
  return { context, calls, asked, panels, say: (yes) => { answer = yes; } };
}
const button = (root, label) => all(root).find((n) => n.tag === 'button' && text(n) === label);

(async () => {
  const state = { open: true, set_by: 'god1', set_by_name: 'Justin', set_at: 1_800_000_000, today: 3, per_day: 50 };
  const { context, calls, asked, panels, say } = load(state);
  const signups = context.botSignupsPanel(panels);
  const body = panels.added.signups.body;
  assert.equal(panels.added.signups.title, 'Bot sign-ups');

  // Hidden until god mode is on, and nothing is asked of the server before it opens.
  assert.equal(signups.row.hidden, true, 'hidden until god mode is on');
  assert.equal(calls.length, 0);
  signups.show(true);
  assert.equal(signups.row.hidden, false, 'shown with god mode on');

  // Opening it reads the switch: open, who set it and when, and the day's count.
  signups.row.onclick();
  await tick();
  assert.equal(panels.current, 'signups');
  assert.deepEqual(calls.map((c) => c.method), ['GET']);
  const said = text(body);
  assert.match(said, /Bot sign-ups are open\./);
  assert.ok(said.includes(`Opened by Justin, ${context.whenText(1_800_000_000)}.`), said);
  assert.match(said, /Accounts made through the door in the last day: 3 of 50\./);
  assert.ok(!said.includes('god1'), 'the raw id is never shown');

  // Saying no to the confirm changes nothing.
  say(false);
  await button(body, 'Close bot sign-ups').onclick({ currentTarget: {} });
  await tick();
  assert.equal(asked.length, 1);
  assert.match(asked[0], /^Close bot sign-ups\?/);
  assert.equal(calls.length, 1, 'no call without a yes');

  // Yes closes it through the endpoint, and the panel shows the server's answer.
  say(true);
  await button(body, 'Close bot sign-ups').onclick({ currentTarget: {} });
  await tick();
  const put = calls[1];
  assert.equal(put.method, 'PUT');
  assert.equal(JSON.stringify(put.body), JSON.stringify({ open: false }));
  assert.match(text(body), /Bot sign-ups are closed\./);
  assert.ok(text(body).includes(`Closed by Morg, ${context.whenText(1_800_000_600)}.`), text(body));
  assert.ok(button(body, 'Open bot sign-ups'), 'and it can be opened again');

  // Opening again asks its own question.
  await button(body, 'Open bot sign-ups').onclick({ currentTarget: {} });
  await tick();
  assert.match(asked[2], /^Open bot sign-ups\?/);
  assert.equal(JSON.stringify(calls[2].body), JSON.stringify({ open: true }));
  assert.match(text(body), /Bot sign-ups are open\./);

  // A refusal is shown in the server's words.
  state.refuse = 'turn god mode on first';
  await button(body, 'Close bot sign-ups').onclick({ currentTarget: {} });
  await tick();
  const error = all(body).find((n) => n.props.class === 'error');
  assert.equal(error.hidden, false);
  assert.equal(error.textContent, 'turn god mode on first');

  // Turning god mode off hides it and closes it if it's open.
  signups.show(false);
  assert.equal(signups.row.hidden, true);
  assert.equal(panels.current, null, 'closed with god mode off');

  // Nobody has set it: it says so.
  const fresh = load({ open: true, set_by: null, set_by_name: null, set_at: null, today: 0, per_day: 50 });
  const blank = fresh.context.botSignupsPanel(fresh.panels);
  blank.show(true);
  blank.row.onclick();
  await tick();
  assert.match(text(fresh.panels.added.signups.body), /Nobody has changed it yet\./);

  // On the map, it's made only for someone the god list names, and god mode decides if it shows.
  assert.ok(app.includes("const signups = typeof me?.god_mode === 'boolean' ? botSignupsPanel(panels) : null;"));
  assert.ok(app.includes('signups?.show(godOn);'));
  console.log('bot sign-ups panel: ok');
})().catch((e) => { console.error(e); process.exit(1); });
