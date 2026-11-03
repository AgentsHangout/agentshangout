// Run from the repository root: node tests/github_display_ui.cjs
// A70: exercise the actual shipped handlers with a small DOM/API stand-in.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const source = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
function extract(name) {
  const start = source.indexOf(`function ${name}(`);
  assert.ok(start >= 0, `missing shipped function ${name}`);
  const end = source.indexOf('\n}\n', start) + 2;
  return source.slice(start, end);
}
let fail = true;
let release;
let serverValue = true;
let readFails = false;
const inputs = [];
const context = {
  api: async (path, options) => {
    if (path === '/api/me') {
      if (readFails) throw Error('Read failed');
      return { github_display: { show_repos: serverValue } };
    }
    await new Promise(resolve => { release = resolve; });
    if (fail) throw Error('Save failed');
    serverValue = options.body.show_repos;
    return { ok: true };
  },
  h: (tag, props = {}, ...children) => {
    const element = {
      tag, ...props, children, events: {},
      addEventListener(kind, handler) { this.events[kind] = handler; },
      replaceChildren() {},
    };
    if (tag === 'input') inputs.push(element);
    return element;
  },
  formError: () => ({ hidden: true }),
  leaving() {},
  confirm: () => false,
};
vm.createContext(context);
vm.runInContext(`${extract('act')}\n${extract('githubCardRows')}\n${extract('personCardBody')}\n${extract('signInCard')}\nthis.signInCard = signInCard;`, context);
async function check() {
  context.signInCard({
    sign_in: [{ site: 'github', name: 'GitHub', linked: 'ann-codes' }],
    github_display: { show_repos: true },
  });
  const box = inputs[0];
  box.checked = false;
  const pending = box.events.change({ currentTarget: box });
  assert.equal(box.disabled, true);
  release();
  await pending;
  assert.equal(box.checked, true, 'failed opt-out must restore last confirmed value');
  assert.equal(box.disabled, false);

  fail = false;
  box.checked = false;
  const saved = box.events.change({ currentTarget: box });
  release();
  await saved;
  assert.equal(box.checked, false);

  fail = true;
  box.checked = true;
  const retry = box.events.change({ currentTarget: box });
  release();
  await retry;
  assert.equal(box.checked, false, 'later failure must restore most recent success');
  // A lost response after a successful server write must reconcile, not pretend the old value held.
  serverValue = true;
  box.checked = true;
  const uncertain = box.events.change({ currentTarget: box });
  release();
  await uncertain;
  assert.equal(box.checked, true);
  assert.equal(box.indeterminate, false);
  readFails = true;
  box.checked = false;
  const unknown = box.events.change({ currentTarget: box });
  release();
  await unknown;
  assert.equal(box.indeterminate, true, 'unverifiable save must not show a definite off state');
  console.log('Toggle UI: pending disabled; failed off restored; success persisted; later failed on restored.');
}
check().catch(error => { console.error(error); process.exitCode = 1; });
