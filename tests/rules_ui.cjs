// Run with node tests/rules_ui.cjs. Runs the shipped community-rules code on the page (A110):
// a write refused for the rules shows them once, Accept and post sends the same write again, and
// Not now sends nothing.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const source = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
function extract(head) {
  const start = source.indexOf(head);
  assert.ok(start >= 0, `missing ${head}`);
  return source.slice(start, source.indexOf('\n}\n', start) + 3);
}
const apiSource = extract('async function api(');
const promptSource = source.slice(source.indexOf('let rulesPrompt = null;'), source.indexOf('function formError() {'));
assert.ok(promptSource.includes('function acceptRules()'), 'missing acceptRules');

function node(tag, props = {}, ...children) {
  const el = { tag, ...props, hidden: props.hidden || false, children: children.flat().filter(Boolean), listeners: {},
    addEventListener(kind, fn) { this.listeners[kind] = fn; },
    replaceChildren(...next) { this.children = next; } };
  return el;
}
const flush = async () => { for (let n = 0; n < 20; n++) await Promise.resolve(); };

async function run({ choose }) {
  const sent = [];
  let accepted = false;
  let dialog = null;
  const c = {
    h: node,
    formError: () => node('p', { class: 'error', hidden: true }),
    modal: (title, ...content) => {
      dialog = { title, content: content.flat(), close() { this.listeners.close?.(); }, listeners: {},
        addEventListener(kind, fn) { this.listeners[kind] = fn; } };
      return dialog;
    },
    fetch: async (path, options = {}) => {
      sent.push({ path, method: options.method || 'GET', body: options.body });
      const ok = (data) => ({ ok: true, status: 200, json: async () => data, text: async () => '<main></main>' });
      if (path === '/api/rules') return ok({ version: '2026-11', blocks: [{ kind: 'h1', text: 'Community rules' }, { kind: 'h2', text: "What isn't allowed" }, { kind: 'li', text: 'Harassment' }] });
      if (path === '/api/me') return ok({ rules_version: '2026-11' });
      if (path === '/api/rules/accept') { accepted = true; return ok({ rules_accepted: true }); }
      if (!accepted) return { ok: false, status: 403, json: async () => ({ error: 'accept the community rules (/rules) before posting', reason: 'rules_not_accepted' }) };
      return ok({ id: 7 });
    },
  };
  vm.createContext(c);
  vm.runInContext(`${apiSource}\n${promptSource}\nthis.api = api;`, c);
  const posting = c.api('/api/hangouts/r/messages', { method: 'POST', body: { text: 'hi' } });
  await flush();
  assert.ok(dialog, 'the rules dialog opened');
  assert.equal(dialog.title, 'Community rules');
  // The text is drawn from GET /api/rules as text nodes, without the page's own title.
  const texts = [];
  const collect = (n) => { if (typeof n === 'string') texts.push(n); else if (n && n.children) n.children.forEach(collect); };
  dialog.content.forEach(collect);
  assert.ok(texts.includes('Harassment'), 'the rules are shown');
  assert.ok(!source.includes('DOMParser'), 'no markup parsing on the page');
  const buttons = [];
  const walk = (n) => { if (!n || typeof n !== 'object') return; if (n.tag === 'button') buttons.push(n); (n.children || []).forEach(walk); };
  dialog.content.forEach(walk);
  const acceptButton = buttons.find((b) => b.children[0] === 'Accept and post');
  const notNow = buttons.find((b) => b.children[0] === 'Not now');
  assert.ok(acceptButton && notNow);
  if (choose === 'accept') {
    await acceptButton.onclick();
    const result = await posting;
    assert.deepEqual(result, { id: 7 }, 'the same write went through after accepting');
    const writes = sent.filter((s) => s.path === '/api/hangouts/r/messages');
    assert.equal(writes.length, 2, 'refused once, sent once more');
    assert.equal(writes[1].body, JSON.stringify({ text: 'hi' }), 'the same body');
    assert.equal(sent.find((s) => s.path === '/api/rules/accept').body, JSON.stringify({ version: '2026-11' }));
  } else {
    notNow.onclick();
    await assert.rejects(posting, /Nothing was posted/);
    assert.equal(sent.filter((s) => s.path === '/api/hangouts/r/messages').length, 1, 'Not now sends nothing more');
    assert.ok(!sent.some((s) => s.path === '/api/rules/accept'));
  }
}

// A112: the grace-window notice, from app.js.
function noticeCheck() {
  const notice = extract('function showRulesNotice(');
  const appended = [];
  const c = { h: node, acceptRules: async () => true,
    document: { querySelector: () => null, body: { append: (n) => appended.push(n) } } };
  vm.createContext(c);
  vm.runInContext(`${notice}
this.showRulesNotice = showRulesNotice;`, c);
  c.showRulesNotice({ rules_accepted: true });
  assert.equal(appended.length, 0, 'nothing once accepted');
  c.showRulesNotice({ rules_update: { version: '2026-11', until: '2026-11-13T00:00:00Z' } });
  assert.equal(appended.length, 1, 'shown in the grace window');
  assert.ok(source.includes('showRulesNotice(me);'), 'rooms show it');
}

(async () => {
  noticeCheck();
  await run({ choose: 'accept' });
  await run({ choose: 'not now' });
  console.log('Rules dialog: shown on a rules refusal, Accept and post retries once, Not now sends nothing.');
})().catch((error) => { console.error(error); process.exitCode = 1; });
