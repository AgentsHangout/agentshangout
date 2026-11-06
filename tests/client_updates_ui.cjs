// Client refresh stays opt-in, checks updates on a throttle, and is harmless in unsupported web views.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const serviceSource = fs.readFileSync('web/client-updates.js', 'utf8').replace(/\r\n/g, '\n').replaceAll('export ', '');
const appSource = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
const uiStart = appSource.indexOf('function clientUpdateControl() {');
assert.ok(uiStart >= 0, 'production clientUpdateControl exists');
const uiHelper = appSource.slice(uiStart, appSource.indexOf('\n}\n', uiStart) + 2);

class Events {
  constructor() { this.listeners = new Map(); }
  addEventListener(name, listener) {
    const all = this.listeners.get(name) || new Set();
    all.add(listener);
    this.listeners.set(name, all);
  }
  removeEventListener(name, listener) { this.listeners.get(name)?.delete(listener); }
  fire(name) { for (const listener of this.listeners.get(name) || []) listener({ type: name }); }
}

function worker(state = 'installed') {
  const events = new Events();
  return { state, messages: [], addEventListener: events.addEventListener.bind(events), fire: events.fire.bind(events), postMessage(message) { this.messages.push(message); } };
}

function makeHarness({ waiting = null, registerError = null, updateError = null } = {}) {
  const serviceWorker = new Events();
  const document = new Events();
  const window = new Events();
  document.visibilityState = 'visible';
  let now = 1_000_000;
  let updates = 0;
  let reloads = 0;
  let accepted = true;
  let registrations = 0;
  let holdUpdate = false;
  let finishUpdate = null;
  const registration = {
    waiting,
    installing: null,
    addEventListener() {},
    async update() {
      updates++;
      if (holdUpdate) await new Promise(resolve => { finishUpdate = resolve; });
      if (updateError) throw new Error('offline');
    },
  };
  serviceWorker.register = async (...args) => {
    registrations++;
    serviceWorker.registerArgs = args;
    if (registerError) throw new Error('unsupported');
    return registration;
  };
  const context = vm.createContext({
    globalThis: {},
    Date: { now: () => now },
  });
  context.globalThis = context;
  vm.runInContext(serviceSource, context);
  const client = context.createClientUpdates({
    navigator: { serviceWorker, onLine: true },
    window,
    document,
    confirm(message) { context.confirmMessage = message; return accepted; },
    reload() { reloads++; },
    now: () => now,
  });
  return {
    client, serviceWorker, document, window, registration, context,
    setNow(value) { now = value; },
    setAccepted(value) { accepted = value; },
    setUpdateError(value) { updateError = value; },
    setRegisterError(value) { registerError = value; },
    beginSlowUpdate() { holdUpdate = true; },
    finishSlowUpdate() { holdUpdate = false; finishUpdate?.(); finishUpdate = null; },
    get registrations() { return registrations; },
    get updates() { return updates; },
    get reloads() { return reloads; },
  };
}

async function run() {
  const waiting = worker('installed');
  const test = makeHarness({ waiting });
  const states = [];
  test.client.subscribe(state => states.push(state));
  await test.client.start();
  assert.equal(JSON.stringify(test.serviceWorker.registerArgs), JSON.stringify(['/client-worker.js', { scope: '/', updateViaCache: 'none' }]));
  assert.equal(test.updates, 1, 'registration performs the initial update check');
  assert.equal(test.client.snapshot().ready, true, 'waiting worker is shown as ready');
  assert.equal(states.at(-1).ready, true);

  test.document.visibilityState = 'hidden';
  test.document.fire('visibilitychange');
  test.window.fire('online');
  assert.equal(test.updates, 1, 'hidden tabs do not check updates');
  test.document.visibilityState = 'visible';
  test.document.fire('visibilitychange');
  test.window.fire('online');
  assert.equal(test.updates, 1, 'visibility and online events share the five minute throttle');
  test.setNow(1_299_999);
  test.window.fire('online');
  assert.equal(test.updates, 1, 'checks inside five minutes are skipped');
  test.setNow(1_300_000);
  test.window.fire('online');
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(test.updates, 2, 'the throttle permits a check after five minutes');
  test.setNow(1_600_000);
  test.setUpdateError(true);
  test.window.fire('online');
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(test.updates, 3, 'update errors are swallowed');
  test.setUpdateError(false);
  test.setNow(1_900_000);
  test.beginSlowUpdate();
  test.window.fire('online');
  assert.equal(test.updates, 4);
  test.setNow(2_200_000);
  test.document.fire('visibilitychange');
  test.window.fire('online');
  assert.equal(test.updates, 4, 'slow update checks never overlap after the throttle expires');
  test.finishSlowUpdate();
  await new Promise(resolve => setImmediate(resolve));

  test.serviceWorker.fire('controllerchange');
  assert.equal(test.reloads, 0, 'controllerchange never reloads an unprompted tab');
  test.setAccepted(false);
  assert.equal(await test.client.activate(), false, 'cancel keeps the current app running');
  assert.equal(waiting.messages.length, 0);
  test.setAccepted(true);
  assert.equal(await test.client.activate(), true);
  assert.match(test.context.confirmMessage, /unsent drafts/i);
  assert.equal(JSON.stringify(waiting.messages), JSON.stringify([{ type: 'activate-update' }]));
  assert.equal(test.reloads, 0, 'the asking tab waits for activation');
  test.serviceWorker.fire('controllerchange');
  assert.equal(test.reloads, 1, 'only the explicitly refreshed tab reloads after activation');
  test.serviceWorker.fire('controllerchange');
  assert.equal(test.reloads, 1, 'activation reload happens once');
  test.client.reload();
  assert.equal(test.reloads, 2, 'route failure reload control uses the same reload action');

  const registrationFailure = makeHarness({ registerError: true });
  await assert.doesNotReject(registrationFailure.client.start(), 'registration failures are harmless');
  assert.equal(registrationFailure.client.snapshot().ready, false);
  registrationFailure.setRegisterError(false);
  registrationFailure.document.fire('visibilitychange');
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(registrationFailure.registrations, 2, 'a later visible event retries failed registration');
  assert.equal(registrationFailure.updates, 1, 'recovered registration checks for updates');
  const noWaiting = makeHarness();
  await noWaiting.client.start();
  assert.equal(await noWaiting.client.activate(), true);
  assert.equal(noWaiting.reloads, 1, 'Refresh app reloads normally when there is no waiting worker');
  const unsupportedContext = vm.createContext({ globalThis: {} });
  unsupportedContext.globalThis = unsupportedContext;
  vm.runInContext(serviceSource, unsupportedContext);
  let unsupportedReloads = 0;
  const unsupported = unsupportedContext.createClientUpdates({
    navigator: {}, document: null, confirm: () => true, reload: () => { unsupportedReloads++; },
  });
  assert.equal((await unsupported.start()).supported, false);
  assert.equal(await unsupported.activate(), true, 'Refresh app remains useful without service workers');
  assert.equal(unsupportedReloads, 1);

  let stateListener;
  let unsubscribed = false;
  let activations = 0;
  const node = (tag, props = {}, ...children) => ({ tag, props, children: children.flat().filter(Boolean), replaceChildren(...next) { this.children = next; } });
  const uiContext = vm.createContext({
    h: node,
    clientUpdates: {
      subscribe(listener) { stateListener = listener; listener({ ready: false }); return () => { unsubscribed = true; }; },
      activate() { activations++; },
    },
  });
  vm.runInContext(uiHelper, uiContext);
  const control = uiContext.clientUpdateControl();
  assert.ok(control.element.children.some(child => child.tag === 'button'), 'Refresh app is always available in Account');
  assert.equal(control.element.children.some(child => child.children?.includes('An app update is ready.')), false);
  stateListener({ ready: true });
  const refresh = control.element.children.find(child => child.tag === 'button');
  assert.ok(refresh && refresh.children.includes('Refresh app'));
  assert.equal(control.element.children.some(child => child.children?.includes('An app update is ready.')), true);
  refresh.props.onclick();
  assert.equal(activations, 1);
  stateListener({ ready: false });
  assert.equal(control.element.children.some(child => child.children?.includes('An app update is ready.')), false, 'the prompt clears after activation');
  control.stop();
  assert.equal(unsubscribed, true, 'leaving the account panel releases its update listener');
  assert.ok(appSource.includes("}, 'Reload app'))"), 'route failure offers an explicit reload');
  console.log('Client updates: registration, polling, activation, account prompt, and reload behavior pass.');
}

run().catch(error => { console.error(error); process.exitCode = 1; });
