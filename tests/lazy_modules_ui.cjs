// node tests/lazy_modules_ui.cjs: #913, what the map doesn't draw loads when someone opens it. The
// map's first load follows only static imports, and none of these five is on it. Then app.js's own
// loaders run here: each import()s the real module from web/, as the page does from its folder, and
// is used the way the page uses it. A wrong path or name, or a module that throws, fails here.
// tutorial.js stays static on purpose: its Get started button and arrive() run on every page load.
const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const fs = require('node:fs'), path = require('node:path'), vm = require('node:vm');

const web = path.resolve(__dirname, '..', 'web');
const read = (name) => fs.readFileSync(path.join(web, name), 'utf8').replace(/\r\n/g, '\n');
const app = read('app.js');
const LAZY = ['passkey.js', 'plan-invite.js', 'bot-setup.js', 'trust.js', 'outside.js'];

// app.js is a module that parses, so no name is both imported and declared.
const parsed = spawnSync(process.execPath, ['--input-type=module', '--check'], { input: app, encoding: 'utf8' });
assert.equal(parsed.status, 0, `app.js doesn't parse as a module:\n${parsed.stderr}`);
assert.ok(!/^await /m.test(app), 'top-level await in app.js');

// The map's first load: every module app.js reaches through static imports, one module to the next.
const STATIC = /^(?:import(?!\s*\()|export)\b[^;'"]*?['"]\.\/([\w-]+\.js)['"]/gm;
const firstLoad = new Set();
for (const todo = ['app.js']; todo.length;) {
  const name = todo.pop();
  if (firstLoad.has(name)) continue;
  firstLoad.add(name);
  for (const [, found] of read(name).matchAll(STATIC)) todo.push(found);
}
for (const name of ['room.js', 'world.js', 'outside-building.js', 'tutorial.js', 'bug-report.js']) {
  assert.ok(firstLoad.has(name), `${name} is on the first load, or this misreads imports`);
}
for (const name of LAZY) {
  assert.ok(!firstLoad.has(name), `${name} is on the map's first load`);
  // The literal form tests/app_imports_served.rs follows, so each is served from the page's folder.
  assert.ok(app.includes(`import('./${name}')`), `${name} is never loaded with import()`);
}

// The loaders as app.js has them, with import() resolved from a file in `dir` as the page resolves it
// from /client/<version>/app.js.
const start = app.indexOf('// #913:');
const end = app.indexOf("\nconst app = document.getElementById('app');");
assert.ok(start >= 0 && end > start, 'the #913 loaders open app.js');
assert.ok(vm.constants?.USE_MAIN_CONTEXT_DEFAULT_LOADER, `import() from a script needs Node 20.12 or newer, not ${process.version}`);
const loaders = (dir) => new vm.Script(
  `(() => {\n${app.slice(start, end)}\nreturn { onDemand, passkeyStatus, registerPasskey, signInWithPasskey, planInviteView, botSetupView, trustPanel, outsideBuilder };\n})()`,
  { filename: path.join(dir, 'app.js'), importModuleDynamically: vm.constants.USE_MAIN_CONTEXT_DEFAULT_LOADER },
).runInThisContext();

// Just enough page for what these modules do before they draw anything.
class El {
  constructor(tag, props = {}, children = []) {
    Object.assign(this, { tag, props: { ...props }, children: [], parentElement: null, style: {}, hidden: Boolean(props.hidden), textContent: '' });
    const classes = new Set(String(props.class || '').split(' ').filter(Boolean));
    this.classList = {
      add: (...names) => names.forEach((name) => classes.add(name)),
      remove: (...names) => names.forEach((name) => classes.delete(name)),
      toggle: (name, on = !classes.has(name)) => { if (on) classes.add(name); else classes.delete(name); return on; },
      contains: (name) => classes.has(name),
    };
    this.append(...children);
  }
  append(...kids) {
    for (const kid of kids.flat(Infinity)) {
      if (kid === null || kid === undefined || kid === false) continue;
      if (kid instanceof El) { kid.remove(); kid.parentElement = this; }
      this.children.push(kid);
    }
  }
  prepend(...kids) { const after = this.children; this.children = []; this.append(...kids); this.children.push(...after); }
  replaceChildren(...kids) { this.children = []; this.append(...kids); }
  remove() {
    if (this.parentElement) this.parentElement.children = this.parentElement.children.filter((kid) => kid !== this);
    this.parentElement = null;
  }
  setAttribute(name, value) { this.props[name] = String(value); }
  addEventListener() {}
  removeEventListener() {}
  focus() {}
  getBoundingClientRect() { return { left: 0, top: 0, width: 0, height: 0 }; }
}
const h = (tag, props, ...children) => new El(tag, props, children);
const text = (node) => (node instanceof El ? node.textContent + node.children.map(text).join('') : String(node));
const failure = (status, message = 'refused') => Object.assign(new Error(message), { status });
const until = async (ready, what) => {
  for (let wait = 0; wait < 1000 && !ready(); wait++) await new Promise((done) => setTimeout(done, 5));
  assert.ok(ready(), `still waiting for ${what}`);
};
const quiet = { matches: false, addEventListener() {}, removeEventListener() {} };
Object.assign(globalThis, {
  window: globalThis,
  document: { title: '', activeElement: null, hidden: false, addEventListener() {}, removeEventListener() {} },
  matchMedia: () => quiet,
  requestAnimationFrame: () => 1,
  cancelAnimationFrame() {},
});

// A map's parts for the outside builder: what it adds, opens, asks for and says is written down.
function mapParts() {
  const added = [], opened = [], said = [], asked = [];
  const root = h('section'), well = h('div'), host = h('div'), actions = h('span');
  well.append(host);
  root.append(well);
  let answer;
  const options = {
    h, modal() {}, formError: () => h('p', { class: 'error', hidden: true }),
    popupMenu: () => ({ isOpen: false, close() {} }),
    selectedBuildCollection() {}, collectionOffers() {}, buildCollectionPicker() {},
    api: (url) => { asked.push(url); return new Promise((resolve, reject) => { answer = { resolve, reject }; }); },
    frame: { root, host, actions },
    world: { sky: undefined, placeLook: () => ({ door: 'L', variant: 0, pal: 0 }), focus() {}, canvas: { focus() {} } },
    panels: { current: null, add: (name) => { added.push(name); return h('div'); }, toggle: (name) => h('button', { name }), open: (name) => opened.push(name), close() {} },
    beside: quiet,
    say: (words) => said.push(words),
    current: () => ({}),
    changed() {},
  };
  return { options, added, opened, said, asked, root, well, answer: () => answer };
}

// An async test that stops on a promise nobody settles would otherwise end with nothing failed.
let finished = false;
process.on('exit', (code) => {
  if (code === 0 && !finished) {
    console.error('lazy modules: stopped before the last check');
    process.exit(1);
  }
});

(async () => {
  const lazy = loaders(web);

  // A failed fetch is tried again the next time, and a module that came is fetched only once.
  let tries = 0;
  const flaky = lazy.onDemand(async () => {
    tries++;
    if (tries === 1) throw new Error('offline');
    return 'module';
  });
  await assert.rejects(flaky(), /offline/);
  assert.equal(await flaky(), 'module');
  assert.equal(await flaky(), 'module');
  assert.equal(tries, 2);

  // Passkeys: the sign-in and sign-up buttons' calls reach passkey.js.
  const posted = [];
  globalThis.fetch = async (url) => {
    posted.push(url);
    return { ok: false, status: url.includes('/login/') ? 401 : 409, json: async () => ({ error: 'that name is taken' }) };
  };
  assert.deepEqual(await lazy.passkeyStatus(), { usable: false, reason: 'browser' });
  await assert.rejects(lazy.registerPasskey({ name: 'Ann' }), /^Error: That name is taken$/);
  await assert.rejects(lazy.signInWithPasskey(), /That didn't work/);
  assert.deepEqual(posted, ['/api/passkeys/register/start', '/api/passkeys/login/start']);

  // A plan's link and a bot setup page, for a code that has gone.
  const gone = async () => { throw failure(404); };
  const plan = await lazy.planInviteView('old', { h, api: gone, field() {}, formError() {}, secretRow() {}, spanText() {}, googleCalendarUrl() {}, startCity() {} });
  assert.match(text(plan), /This link doesn't work any more/);
  const setup = await lazy.botSetupView('old', { h, api: gone, field() {}, formError() {}, link() {} });
  assert.match(text(setup), /This bot setup isn't here/);

  // Trust in My bots: nothing is fetched until My bots opens, then trust.js draws in its host.
  const asked = [], opened = [];
  const host = h('section');
  const trust = lazy.trustPanel({
    h, me: { kind: 'person' }, host, hostPanel: 'bots', figure: () => null, dress() {}, schedule() {}, crew() {},
    api: async (url) => { asked.push(url); throw failure(503, 'Trust is down'); },
    panels: { open: (name) => opened.push(name) },
  });
  assert.equal(trust.toggle, null, 'no button of its own');
  trust.refreshLooks();
  trust.open();
  assert.deepEqual(opened, ['bots'], 'opening it opens My bots');
  await new Promise((done) => setTimeout(done, 20));
  assert.deepEqual(asked, [], 'nothing asked before My bots opens');
  assert.equal(host.classList.contains('trust-body'), false, 'trust.js has not made the panel yet');
  trust.load();
  await until(() => text(host).includes('Try again'), 'trust.js to draw its answer');
  assert.ok(host.classList.contains('trust-body'), 'the panel is trust.js\'s own, in the host');
  assert.deepEqual(asked, ['/api/trust']);
  assert.match(text(host), /Trust is down/);
  trust.refreshLooks();

  // The outside builder: a map left before outside.js comes never builds one.
  const left = mapParts();
  const early = lazy.outsideBuilder(left.options);
  const leaving = early.open({ x: 1, y: 1, kind: 'house' });
  early.destroy();
  await leaving;
  assert.deepEqual([left.added, left.asked], [[], []], 'nothing made for a map that has gone');

  // Before Build outside, nothing is open or in hand; after it, the builder is outside.js's own.
  const map = mapParts();
  const build = lazy.outsideBuilder(map.options);
  assert.deepEqual([build.isOpen, build.row(), build.takesEscape()], [false, null, false]);
  build.close();
  assert.deepEqual(map.added, [], 'no Build panel until someone opens it');
  const opening = build.open({ x: 3, y: 4, kind: 'house' });
  await until(() => map.asked.length, 'outside.js to ask for the place');
  assert.deepEqual(map.asked, ['/api/places/3/4/outside']);
  assert.deepEqual(map.added, ['outside']);
  assert.deepEqual(map.opened, ['outside']);
  assert.equal(build.isOpen, true);
  assert.deepEqual(build.row(), [3, 4, {}]);
  assert.equal(build.takesEscape(), false);
  assert.ok(map.well.children.some((kid) => kid.props?.class === 'outside-stage' && !kid.hidden), 'the stage is over the map');
  assert.ok(map.root.children.some((kid) => kid.props?.class?.includes('outside-dock')), 'with its dock');
  assert.equal(map.options.frame.actions.children[0]?.props.name, 'outside', 'and its Build button first in the bar');
  map.answer().reject(failure(403, 'Only the owner and approved people can edit this house.'));
  await opening;
  assert.deepEqual(map.said, ['Only the owner and approved people can edit this house.']);
  assert.deepEqual([build.isOpen, build.row()], [false, null]);
  build.destroy();
  assert.ok(!map.well.children.some((kid) => kid.props?.class === 'outside-stage'), 'leaving takes the stage away');
  assert.ok(!map.root.children.some((kid) => kid.props?.class?.includes('outside-dock')), 'and the dock');

  // A module that doesn't come: each says so where it opens, and doesn't throw.
  const offline = loaders(path.join(web, 'missing'));
  const away = mapParts();
  await offline.outsideBuilder(away.options).open({ x: 3, y: 4, kind: 'house' });
  assert.deepEqual(away.said, ['Build outside didn\'t load. Check your connection, then reload the page.']);
  const emptyHost = h('section');
  offline.trustPanel({ h, host: emptyHost, hostPanel: 'bots', panels: {} }).load();
  await until(() => text(emptyHost).length, 'the trust host to say it didn\'t load');
  assert.match(text(emptyHost), /didn't load/);

  finished = true;
  console.log('lazy modules ok: none on the first load, and each one loads and runs when it opens');
})().catch((error) => { console.error(error); process.exit(1); });
