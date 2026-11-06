const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const { webcrypto, createHash } = require('node:crypto');
const source = fs.readFileSync('web/client-worker.js', 'utf8');
const origin = 'https://example.test';
const stores = new Map();
const key = value => new URL(typeof value === 'string' ? value : value.url, origin).href;
const caches = {
  keys: async () => [...stores.keys()],
  delete: async name => stores.delete(name),
  open: async name => {
    if (!stores.has(name)) stores.set(name, new Map());
    const data = stores.get(name);
    return { match: async request => data.get(key(request))?.clone(),
      put: async (request, response) => data.set(key(request), response.clone()) };
  },
};
let requests = [], offline = false, corrupt = false, claims = 0, activations = 0;
function boot(version, body) {
  const url = `/client/${version}/app.js`;
  const listeners = {};
  const manifest = { version, files: [{ url, sha256: createHash('sha256').update(body).digest('hex') }] };
  const self = { location: { origin }, clients: { matchAll: async () => [{ id: 'open-page' }], claim: async () => claims++ },
    skipWaiting: async () => activations++, addEventListener: (name, fn) => listeners[name] = fn };
  vm.runInNewContext(source, { CLIENT_MANIFEST: manifest, self, caches, crypto: webcrypto,
    URL, Request, Response, Headers, fetch: async request => {
      requests.push(key(request));
      if (offline) throw Error('offline');
      return new Response(corrupt ? 'corrupt' : body, { headers: { 'content-type': 'text/javascript' } });
    } });
  return { url, listeners, run: async name => {
    let promise; listeners[name]({ waitUntil: p => promise = p }); await promise;
  } };
}
(async () => {
  const first = boot('a'.repeat(64), 'first');
  await first.run('install');
  assert.equal(activations, 0, 'install never replaces an active worker');
  await first.run('activate');
  offline = true;
  let response;
  first.listeners.fetch({ request: new Request(origin + first.url), respondWith: p => response = p });
  assert.equal(await (await response).text(), 'first');
  for (const url of ['/api/me', '/h/private', '/client-manifest.json', '/app.js', 'https://other.test/client/' + 'a'.repeat(64) + '/app.js']) {
    let intercepted = false;
    first.listeners.fetch({ request: new Request(url.startsWith('https') ? url : origin + url), respondWith: () => intercepted = true });
    assert.equal(intercepted, false, url);
  }
  offline = false;
  requests = [];
  const same = boot('b'.repeat(64), 'first');
  await same.run('install');
  assert.equal(requests.length, 0, 'unchanged verified assets are reused locally');
  corrupt = true;
  const broken = boot('c'.repeat(64), 'new');
  await assert.rejects(broken.run('install'));
  assert.equal((await caches.keys()).some(k => k.includes('c'.repeat(64))), false);
  corrupt = false;
  const next = boot('d'.repeat(64), 'new');
  await next.run('install'); await next.run('activate');
  offline = true;
  next.listeners.fetch({ request: new Request(origin + first.url), respondWith: p => response = p });
  assert.equal(await (await response).text(), 'first', 'new worker still serves a running old document exactly');
  next.listeners.message({ data: { type: 'other' } });
  assert.equal(activations, 0);
  next.listeners.message({ data: { type: 'activate-update' }, waitUntil: p => response = p });
  await response;
  assert.equal(activations, 1);
  console.log('M7: complete verified caching, unchanged reuse, offline assets, failed updates, session isolation and explicit activation pass.');
})().catch(error => { console.error(error); process.exitCode = 1; });
