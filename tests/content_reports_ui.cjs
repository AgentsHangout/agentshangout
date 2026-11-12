// RE21: run the shipped reporting dialog and the actual message/note controls without a server.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const source = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
class Node {
  constructor(tag, props = {}, children = []) {
    this.tag = tag; this.children = []; this.style = {}; this.value = '';
    Object.assign(this, props); this.append(...children);
  }
  append(...children) { this.children.push(...children.flat(Infinity).filter(x => x != null && x !== false)); }
  replaceChildren(...children) { this.children = []; this.append(...children); }
  set textContent(value) { this.replaceChildren(String(value)); }
  get textContent() { return this.children.map(text).join(''); }
  set innerHTML(_) { throw Error('Report text must never be parsed as HTML'); }
  close() { this.closed = true; }
  focus() { this.focused = true; }
}
const h = (tag, props, ...children) => new Node(tag, props, children);
const all = node => node instanceof Node ? [node, ...node.children.flatMap(all)] : [];
const text = node => node instanceof Node ? node.textContent : String(node);
function fn(name, indent = '') {
  const start = source.indexOf(`function ${name}(`);
  assert.ok(start >= 0, `web/app.js has ${name}`);
  const end = source.indexOf(`\n${indent}}\n`, start);
  assert.ok(end > start, `${name} ends`);
  return source.slice(start, end + indent.length + 3);
}
function setup() {
  const calls = [], dialogs = [];
  let failure = null, wait = null;
  const context = {
    h,
    api: async (path, options) => {
      calls.push({ path, ...JSON.parse(JSON.stringify(options)) });
      if (wait) await wait;
      if (failure) throw failure;
      return { report_id: 41, status: 'received', blocked_person: true, block_note: 'Blocked the bot\'s owner.' };
    },
    modal: (title, ...children) => {
      const dialog = h('dialog', { title }, ...children); dialogs.push(dialog); return dialog;
    },
    snapshot: { you: { person_id: 'me' }, keyless_message_ids: [] },
    person: () => ({ name: 'Bo' }), colorOf: () => '#123456', shownById: new Map(),
    timeFormat: { format: () => 'now' }, fromUnix: value => value,
    chatText: value => [value], messageRefs: value => value, refLink: () => null,
    whenText: () => 'now',
  };
  vm.createContext(context);
  vm.runInContext([
    fn('formError'), fn('field'), fn('contentReportDialog'), fn('contentReportButton'),
    fn('messageRow', '  '), fn('noteRow', '  '),
    'this.report = (kind, id, label, options) => contentReportButton(kind, id, label, { enabled: true, ...options });',
    'this.contentReportButton = contentReportButton; this.messageRow = messageRow; this.noteRow = noteRow;',
  ].join('\n'), context);
  return { context, calls, dialogs, fail: error => { failure = error; }, wait: promise => { wait = promise; } };
}
const field = (dialog, name) => all(dialog).find(node => node.name === name);
const form = dialog => all(dialog).find(node => node.tag === 'form');
const send = dialog => form(dialog).onsubmit({ preventDefault() {}, target: form(dialog) });
const reportButton = node => all(node).find(node => node.tag === 'button' && text(node) === 'Report');

(async () => {
  let ui = setup();
  const button = ui.context.contentReportButton('bot', 'b1', '<img src=x onerror=bad()>', { enabled: true });
  assert.ok(button.class.includes('wd-btn'));
  assert.match(button['aria-label'], /^Report /);
  assert.equal(ui.calls.length, 0, 'rendering a button sends nothing');
  button.onclick();
  const cancelled = ui.dialogs.at(-1);
  assert.ok(text(cancelled).includes('<img src=x onerror=bad()>'), 'the target label stays text');
  cancelled.close();
  assert.equal(ui.calls.length, 0, 'opening and cancelling send nothing');
  assert.equal(ui.context.contentReportButton('bot', 'b1', 'Own bot', { enabled: true, own: true }), null);
  assert.equal(ui.context.contentReportButton('bot', 'b1', 'Bot viewer', { enabled: false }), null);

  button.onclick();
  const dialog = ui.dialogs.at(-1);
  const reason = field(dialog, 'reason'), details = field(dialog, 'details'), block = field(dialog, 'block');
  assert.equal(reason.required, true);
  assert.deepEqual(all(reason).filter(n => n.tag === 'option' && n.value).map(n => n.value),
    ['harassment', 'hate', 'sexual', 'minor_safety', 'spam', 'impersonation', 'other']);
  assert.equal(details.maxLength, 1000);
  assert.ok(text(dialog).includes('owner'), 'blocking a bot names its owner');
  assert.ok(text(dialog).includes('illegal material'));
  reason.value = 'minor_safety'; details.value = 'Please review this message.'; block.checked = true;
  let finish;
  ui.wait(new Promise(resolve => { finish = resolve; }));
  const pending = send(dialog);
  await send(dialog);
  assert.equal(ui.calls.length, 1, 'a pending report cannot be sent twice');
  assert.equal(all(dialog).find(n => n.type === 'submit').disabled, true);
  assert.equal(JSON.stringify(ui.calls[0]), JSON.stringify({ path: '/api/reports', method: 'POST', body: {
    target_kind: 'bot', target_id: 'b1', reason: 'minor_safety', details: 'Please review this message.', block: true,
  } }));
  finish(); await pending;
  assert.match(text(dialog), /41/);
  assert.ok(text(dialog).includes('Blocked the bot\'s owner.'), 'the exact server block note is shown');
  assert.ok(all(dialog).some(n => n.role === 'status'), 'success is announced');
  assert.ok(!field(dialog, 'details'), 'a received report has no resubmit control');
  assert.ok(all(dialog).some(n => n.tag === 'button' && text(n) === 'Done' && n.focused), 'focus moves to Done');

  ui = setup();
  ui.context.contentReportButton('place', 'room1', 'this room', { enabled: true }).onclick();
  const place = ui.dialogs.at(-1);
  assert.equal(field(place, 'block'), undefined, 'a place has nobody to block');
  field(place, 'reason').value = 'spam'; field(place, 'details').value = 'Keep my draft';
  ui.fail(Object.assign(new Error('Daily report limit. Try again tomorrow.'), { status: 429 }));
  await send(place);
  assert.equal(field(place, 'details').value, 'Keep my draft');
  assert.ok(text(place).includes('Daily report limit. Try again tomorrow.'));
  assert.equal(ui.calls.length, 1, 'a refusal is never retried automatically');
  assert.equal(all(place).find(n => n.type === 'submit').disabled, false, 'manual retry remains possible');
  assert.equal('block' in ui.calls[0].body, false);
  ui.fail(null); await send(place);
  assert.equal(ui.calls.length, 2);

  // Exercise the actual room renderers: they must report the selected record, never a pasted id.
  ui = setup();
  const message = { id: 17, kind: 'message', person_id: 'other', speaker: 'Bo', body: 'Hi', created_at: 1 };
  reportButton(ui.context.messageRow(message)).onclick();
  const messageDialog = ui.dialogs.at(-1);
  field(messageDialog, 'reason').value = 'minor_safety';
  await send(messageDialog);
  assert.equal(ui.calls[0].body.target_kind, 'message');
  assert.equal(ui.calls[0].body.target_id, '17');
  assert.equal(reportButton(ui.context.messageRow({ ...message, person_id: 'me' })), undefined);
  assert.equal(reportButton(ui.context.messageRow({ ...message, kind: 'system' })), undefined);
  reportButton(ui.context.noteRow({ id: 'note1', text: 'A note', written_by: 'Bo', created_at: 1 })).onclick();
  const noteDialog = ui.dialogs.at(-1);
  field(noteDialog, 'reason').value = 'other'; await send(noteDialog);
  assert.equal(ui.calls[1].body.target_kind, 'note');
  assert.equal(ui.calls[1].body.target_id, 'note1');
  console.log('content reports UI ok (RE21)');
})().catch(error => { console.error(error); process.exit(1); });
