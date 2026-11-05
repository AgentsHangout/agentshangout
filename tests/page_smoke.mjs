// #575: load the real pages in Node and fail on any error they throw while they start. It runs the
// shipped web/*.js against a small stand-in DOM and a real test server, for the path given, so a
// mistake that only shows when the code runs (like #558's `const done` before its use) fails CI
// instead of the live site.
//
//   node tests/page_smoke.mjs <base url> <bearer key> <path>
//
// The stand-in DOM does just enough for a page to build itself: elements keep their children and
// attributes, and everything a browser would draw or measure is a no-op. Canvas drawing is a
// do-anything stub. Nothing here checks how a page looks, only that building it doesn't throw.
import { mkdtempSync, readdirSync, copyFileSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL, fileURLToPath } from 'node:url';

const [base, key, ...paths] = process.argv.slice(2);
if (!base || !key || paths.length !== 1) {
  console.error('usage: page_smoke.mjs <base> <key> <path>');
  process.exit(2);
}

const failures = [];
const fail = (where, error) => failures.push(`${where}: ${error?.stack || error}`);
process.on('uncaughtException', (error) => fail('uncaught', error));
process.on('unhandledRejection', (error) => fail('unhandled rejection', error));

// A canvas context, gradient or anything else a page draws with: every property is callable and
// returns another of the same, so drawing code runs without a real canvas.
const anything = () => {
  const fn = function () { return proxy; };
  const proxy = new Proxy(fn, {
    get(_t, prop) {
      if (prop === Symbol.toPrimitive) return () => 0;
      if (prop === 'then') return undefined;
      if (prop === 'width' || prop === 'height') return 1;
      if (prop === 'data') return new Uint8ClampedArray(4);
      return proxy;
    },
    set() { return true; },
    apply() { return proxy; },
  });
  return proxy;
};

class FakeEvent { constructor(type, init = {}) { this.type = type; Object.assign(this, init); } preventDefault() {} stopPropagation() {} stopImmediatePropagation() {} }

class FakeNode {
  constructor(tag = '#node') {
    this.tagName = String(tag).toUpperCase();
    this.nodeName = this.tagName;
    this.children = [];
    this.childNodes = this.children;
    this.attributes = {};
    this.dataset = {};
    this.style = new Proxy({ setProperty() {}, removeProperty() {}, getPropertyValue() { return ''; } }, { get: (t, p) => (p in t ? t[p] : ''), set: () => true });
    this.parentNode = null;
    this.hidden = false;
    this.value = '';
    this.checked = false;
    this.disabled = false;
    this.className = '';
    this.id = '';
    this.width = 0;
    this.height = 0;
    this._text = '';
    const self = this;
    this.classList = {
      add: (...n) => { const s = new Set(self.className.split(' ').filter(Boolean)); n.forEach((x) => s.add(x)); self.className = [...s].join(' '); },
      remove: (...n) => { self.className = self.className.split(' ').filter((x) => x && !n.includes(x)).join(' '); },
      toggle: (n, force) => { const has = self.className.split(' ').includes(n); const on = force === undefined ? !has : force; if (on) self.classList.add(n); else self.classList.remove(n); return on; },
      contains: (n) => self.className.split(' ').includes(n),
    };
  }
  get firstChild() { return this.children[0] || null; }
  get lastChild() { return this.children.at(-1) || null; }
  get firstElementChild() { return this.firstChild; }
  get parentElement() { return this.parentNode; }
  get nextSibling() { const s = this.parentNode?.children; return s ? s[s.indexOf(this) + 1] || null : null; }
  get previousSibling() { const s = this.parentNode?.children; return s ? s[s.indexOf(this) - 1] || null : null; }
  get nextElementSibling() { return this.nextSibling; }
  get previousElementSibling() { return this.previousSibling; }
  get lastElementChild() { return this.lastChild; }
  get childElementCount() { return this.children.length; }
  get isConnected() { return true; }
  get textContent() { return this._text + this.children.map((c) => c.textContent).join(''); }
  set textContent(v) { this.children.length = 0; this._text = String(v ?? ''); }
  get innerText() { return this.textContent; }
  set innerText(v) { this.textContent = v; }
  set innerHTML(v) { this.textContent = ''; }
  get innerHTML() { return ''; }
  get offsetParent() { return this.parentNode; }
  get offsetWidth() { return 0; }
  get offsetHeight() { return 0; }
  get clientWidth() { return 800; }
  get clientHeight() { return 600; }
  get scrollHeight() { return 0; }
  append(...nodes) {
    for (const n of nodes.flat()) {
      if (n == null || n === false) continue;
      const child = n instanceof FakeNode ? n : new FakeText(String(n));
      child.parentNode = this;
      this.children.push(child);
    }
  }
  appendChild(n) { this.append(n); return n; }
  prepend(...nodes) { const old = this.children.splice(0); this.append(...nodes); this.children.push(...old); }
  before() {} after() {}
  insertBefore(n) { this.append(n); return n; }
  removeChild(n) { const i = this.children.indexOf(n); if (i >= 0) this.children.splice(i, 1); return n; }
  replaceChildren(...nodes) { this.children.length = 0; this._text = ''; this.append(...nodes); }
  replaceWith() {}
  remove() { this.parentNode?.removeChild(this); }
  cloneNode() { return new FakeNode(this.tagName); }
  setAttribute(k, v) {
    this.attributes[k] = String(v);
    if (k === 'class') this.className = String(v);
    if (k === 'id') this.id = String(v);
  }
  getAttribute(k) { return this.attributes[k] ?? null; }
  hasAttribute(k) { return k in this.attributes; }
  removeAttribute(k) { delete this.attributes[k]; }
  toggleAttribute(k, on) { if (on === false) delete this.attributes[k]; else this.attributes[k] = ''; }
  addEventListener() {} removeEventListener() {} dispatchEvent() { return true; }
  get descendants() { return this.children.flatMap((c) => [c, ...c.descendants]); }
  querySelector(selector) { return this.descendants.find((n) => n.matches(selector)) || null; }
  querySelectorAll(selector) { return this.descendants.filter((n) => n.matches(selector)); }
  getElementsByTagName(tag) { return this.querySelectorAll(tag); }
  getElementsByClassName(name) { return this.querySelectorAll(`.${name}`); }
  closest(selector) { for (let n = this; n; n = n.parentNode) if (n.matches(selector)) return n; return null; }
  // Enough of CSS for pages that look up what they just built: tags, .classes, #ids and [attributes],
  // compounded and comma-separated. With combinators, only the last part has to match.
  matches(selector) {
    if (this instanceof FakeText) return false;
    return String(selector).split(',').some((one) => {
      const last = one.trim().split(/[\s>+~]+/).at(-1) || '';
      const parts = last.match(/[.#]?[\w-]+|\[[^\]]+\]|:[\w-]+(\([^)]*\))?/g) || [];
      return parts.length > 0 && parts.every((part) => {
        if (part.startsWith('.')) return this.className.split(' ').includes(part.slice(1));
        if (part.startsWith('#')) return this.id === part.slice(1);
        if (part.startsWith(':')) return true;
        if (part.startsWith('[')) {
          const [, name, value] = part.match(/^\[([\w-]+)(?:[~|^$*]?=["']?([^"'\]]*)["']?)?\]$/) || [];
          if (!name) return false;
          const actual = name.startsWith('data-') ? this.dataset[name.slice(5).replace(/-(\w)/g, (_m, c) => c.toUpperCase())] ?? this.attributes[name] : this.attributes[name] ?? this[name];
          return value === undefined ? actual != null && actual !== false : String(actual) === value;
        }
        return part === '*' || this.tagName === part.toUpperCase();
      });
    });
  }
  contains(n) { return n === this; }
  focus() {} blur() {} click() {} select() {} scrollIntoView() {} scrollTo() {} showModal() {} close() {} requestFullscreen() {}
  setPointerCapture() {} releasePointerCapture() {} hasPointerCapture() { return false; }
  getBoundingClientRect() { return { x: 0, y: 0, left: 0, top: 0, right: 0, bottom: 0, width: 0, height: 0 }; }
  getAnimations() { return []; }
  animate() { return { finished: Promise.resolve(), cancel() {}, onfinish: null }; }
  attachShadow() { return new FakeNode('shadow'); }
  getContext() { return anything(); }
  toDataURL() { return 'data:image/png;base64,'; }
  toBlob(cb) { cb?.(null); }
  setSelectionRange() {}
  checkValidity() { return true; }
  reportValidity() { return true; }
  reset() {}
  requestSubmit() {}
}
class FakeText extends FakeNode { constructor(text) { super('#text'); this._text = text; } }

const html = new FakeNode('html');
const head = new FakeNode('head');
const body = new FakeNode('body');
const appRoot = new FakeNode('div');
appRoot.id = 'app';
html.append(head, body);
body.append(appRoot);

const storage = () => { const m = new Map(); return { getItem: (k) => (m.has(k) ? m.get(k) : null), setItem: (k, v) => m.set(k, String(v)), removeItem: (k) => m.delete(k), clear: () => m.clear(), key: () => null, get length() { return m.size; } }; };
const noopClass = class { constructor() {} observe() {} unobserve() {} disconnect() {} close() {} addEventListener() {} removeEventListener() {} };

let current = new URL(paths[0], base);
const location = {
  get href() { return current.href; },
  get pathname() { return current.pathname; },
  get search() { return current.search; },
  get hash() { return current.hash; },
  set hash(v) { current.hash = v; },
  get origin() { return current.origin; },
  get host() { return current.host; },
  get hostname() { return current.hostname; },
  get protocol() { return current.protocol; },
  assign(u) { current = new URL(u, current); },
  replace(u) { current = new URL(u, current); },
  reload() {},
};
const history = {
  state: null,
  pushState(_s, _t, u) { if (u) current = new URL(u, current); },
  replaceState(_s, _t, u) { if (u) current = new URL(u, current); },
  back() {}, forward() {},
};
const document = {
  documentElement: html, head, body,
  title: 'Agents Hangout',
  visibilityState: 'visible', hidden: false, readyState: 'complete', cookie: '',
  activeElement: body, fullscreenElement: null,
  fonts: { ready: Promise.resolve(), load: async () => [], add() {} },
  createElement: (tag) => new FakeNode(tag),
  createElementNS: (_ns, tag) => new FakeNode(tag),
  createTextNode: (t) => new FakeText(t),
  createDocumentFragment: () => new FakeNode('#fragment'),
  getElementById: (id) => (id === 'app' ? appRoot : null),
  querySelector: () => null,
  querySelectorAll: () => [],
  addEventListener() {}, removeEventListener() {},
  exitFullscreen: async () => {},
  execCommand: () => false,
};

const globals = {
  window: globalThis, self: globalThis, document, location, history,
  Node: FakeNode, Element: FakeNode, HTMLElement: FakeNode, HTMLCanvasElement: FakeNode, Text: FakeText,
  Event: FakeEvent, CustomEvent: FakeEvent, KeyboardEvent: FakeEvent, PointerEvent: FakeEvent, MouseEvent: FakeEvent,
  localStorage: storage(), sessionStorage: storage(),
  matchMedia: () => ({ matches: false, media: '', addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }),
  getComputedStyle: () => new Proxy({}, { get: (_t, p) => (p === 'getPropertyValue' ? () => '' : '') }),
  requestAnimationFrame: () => 0, cancelAnimationFrame() {},
  requestIdleCallback: () => 0, cancelIdleCallback() {},
  ResizeObserver: noopClass, IntersectionObserver: noopClass, MutationObserver: noopClass,
  EventSource: noopClass, WebSocket: noopClass, BroadcastChannel: noopClass,
  Image: class extends FakeNode { constructor() { super('img'); } decode() { return Promise.resolve(); } },
  OffscreenCanvas: class extends FakeNode { constructor() { super('canvas'); } },
  ImageData: class { constructor(w = 1, h = 1) { this.width = w; this.height = h; this.data = new Uint8ClampedArray(w * h * 4); } },
  Path2D: class { constructor() {} },
  DOMParser: class { parseFromString() { return document; } },
  devicePixelRatio: 1, innerWidth: 1280, innerHeight: 800, screen: { width: 1280, height: 800 },
  scrollTo() {}, scrollBy() {}, alert() {}, confirm: () => false, prompt: () => null, open: () => null, print() {}, focus() {},
  addEventListener() {}, removeEventListener() {}, dispatchEvent() { return true; },
  isSecureContext: true,
  // Repeating timers do nothing. setTimeout stays Node's own: fetch uses it for its timeouts, and the
  // run ends with process.exit, so a long one-off timer never holds it open.
  setInterval: () => 0, clearInterval() {},
};
globalThis.__realSetTimeout = globalThis.setTimeout;
for (const [name, value] of Object.entries(globals)) {
  Object.defineProperty(globalThis, name, { value, writable: true, configurable: true });
}
Object.defineProperty(globalThis, 'navigator', {
  value: { userAgent: 'node-smoke', language: 'en-US', languages: ['en-US'], vibrate() { return false; }, clipboard: { writeText: async () => {} }, onLine: true, maxTouchPoints: 0, credentials: { get: async () => null, create: async () => null }, share: undefined, serviceWorker: undefined },
  configurable: true, writable: true,
});

// The page calls the real test server, signed in with the given key.
const realFetch = globalThis.fetch;
globalThis.fetch = (path, options = {}) => {
  const url = new URL(String(path), base);
  return realFetch(url, { ...options, headers: { ...(options.headers || {}), authorization: `Bearer ${key}` } })
    .catch((error) => { fail(`fetch ${url.pathname}`, error?.cause || error); throw error; });
};

// The pages are ES modules ending in .js, so copy them next to a package.json that says so.
const web = fileURLToPath(new URL('../web/', import.meta.url));
const dir = mkdtempSync(join(tmpdir(), 'page-smoke-'));
writeFileSync(join(dir, 'package.json'), '{"type":"module"}');
for (const file of readdirSync(web)) if (file.endsWith('.js')) copyFileSync(join(web, file), join(dir, file));
// route() catches what a page throws and shows only its message, so the copy also hands the error
// here, for the stack. If route() changes shape this finds nothing, and the shown message still fails.
const appCopy = join(dir, 'app.js');
writeFileSync(appCopy, readFileSync(appCopy, 'utf8').replace(
  /(\n  \} catch \(error\) \{\r?\n)(    if \(error\.status === 401)/,
  '$1    if (!(error.status === 401 || error.status === 404)) globalThis.__pageError?.(error);\n$2',
));
globalThis.__pageError = (error) => fail('route', error);

const settle = async () => { for (let i = 0; i < 40; i++) await new Promise((r) => globalThis.__realSetTimeout(r, 25)); };

try {
  // Importing app.js routes to the first path straight away.
  await import(pathToFileURL(join(dir, 'app.js')).href);
} catch (error) {
  fail(`loading ${paths[0]}`, error);
}
await settle();
const check = (path) => {
  const shown = appRoot.textContent;
  if (shown.includes("That didn't load")) fail(path, `the page showed "That didn't load": ${shown.slice(0, 300)}`);
  if (!appRoot.children.length) fail(path, 'the page drew nothing');
};
check(paths[0]);

rmSync(dir, { recursive: true, force: true });
if (failures.length) {
  console.error(failures.join('\n\n'));
  process.exit(1);
}
console.log(`page smoke: ${paths[0]} loaded with no errors`);
process.exit(0);
