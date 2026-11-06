// A shared, opt-in updater for the browser and the Android/iOS web views. Installing a worker
// never reloads a page: only the tab where someone confirms Refresh app reloads after activation.
export function createClientUpdates({
  navigator: nav = globalThis.navigator,
  window: win = globalThis.window ?? globalThis,
  document: doc = globalThis.document,
  confirm: ask = (message) => globalThis.confirm(message),
  reload: reloadPage = () => globalThis.location.reload(),
  now = () => Date.now(),
} = {}) {
  const listeners = new Set();
  let registration = null;
  let signalsStarted = false;
  let starting = null;
  let lastCheck = -Infinity;
  let checking = null;
  let activationRequested = false;
  let controllerListener = null;

  const supported = Boolean(nav?.serviceWorker?.register);
  const snapshot = () => ({ supported, ready: Boolean(registration?.waiting) });
  const notify = () => {
    const current = snapshot();
    for (const listener of listeners) {
      try { listener(current); } catch { /* A detached panel cannot break update checks. */ }
    }
  };
  const observeWorker = (worker) => {
    worker?.addEventListener?.('statechange', () => {
      notify();
      if (worker.state === 'installed') queueMicrotask(notify);
    });
  };
  const observeRegistration = (reg) => {
    reg.addEventListener?.('updatefound', () => {
      observeWorker(reg.installing);
      notify();
    });
    observeWorker(reg.installing);
    notify();
  };
  const isCheckable = () => doc?.visibilityState !== 'hidden' && nav?.onLine !== false;

  async function checkForUpdate() {
    if (!registration || !isCheckable() || now() - lastCheck < 5 * 60_000) return false;
    if (checking) return checking;
    lastCheck = now();
    checking = (async () => {
      try {
        await registration.update();
      } catch {
        // A network or browser update error must not interfere with the running app.
      }
      notify();
      return true;
    })();
    try { return await checking; } finally { checking = null; }
  }

  function onVisibilityChange() {
    if (doc?.visibilityState === 'visible') {
      if (registration) void checkForUpdate();
      else void start();
    }
  }

  function onOnline() {
    if (registration) void checkForUpdate();
    else void start();
  }

  function onControllerChange() {
    notify();
    if (!activationRequested) return;
    activationRequested = false;
    if (controllerListener) nav.serviceWorker.removeEventListener?.('controllerchange', controllerListener);
    controllerListener = null;
    reloadPage();
  }

  async function start() {
    if (!supported || registration) return snapshot();
    if (starting) return starting;
    if (!signalsStarted) {
      signalsStarted = true;
      nav.serviceWorker.addEventListener?.('controllerchange', onControllerChange);
      doc?.addEventListener?.('visibilitychange', onVisibilityChange);
      win?.addEventListener?.('online', onOnline);
    }
    starting = (async () => {
      try {
        registration = await nav.serviceWorker.register('/client-worker.js', {
          scope: '/',
          updateViaCache: 'none',
        });
        observeRegistration(registration);
        await checkForUpdate();
      } catch {
        // Worker registration is optional; later visible or online events retry it.
        registration = null;
      } finally {
        starting = null;
        notify();
      }
      return snapshot();
    })();
    return starting;
  }

  function subscribe(listener) {
    listeners.add(listener);
    listener(snapshot());
    return () => listeners.delete(listener);
  }

  async function activate() {
    let accepted = false;
    try {
      accepted = ask('Refresh the app now? Save any unsent drafts first; refreshing reloads this page.');
    } catch {
      accepted = false;
    }
    if (!accepted) return false;
    const waiting = registration?.waiting;
    if (!waiting) {
      reloadPage();
      return true;
    }
    activationRequested = true;
    controllerListener = onControllerChange;
    try {
      nav.serviceWorker.addEventListener?.('controllerchange', controllerListener);
      waiting.postMessage({ type: 'activate-update' });
      return true;
    } catch {
      activationRequested = false;
      nav.serviceWorker.removeEventListener?.('controllerchange', controllerListener);
      controllerListener = null;
      return false;
    }
  }

  function reload() {
    reloadPage();
  }

  return { start, subscribe, checkForUpdate, activate, reload, snapshot };
}
