/* CLIENT_MANIFEST is embedded by the server from this binary's public assets. */
const CACHE_PREFIX = 'agentshangout-client-';
const CACHE_NAME = CACHE_PREFIX + CLIENT_MANIFEST.version;
const assetPath = /^\/client\/([a-f0-9]{64})\/([a-z][a-z0-9-]*\.(?:js|css|svg|png|jpg|ico))$/;
const versionCache = name => /^agentshangout-client-[a-f0-9]{64}$/.test(name);
const digest = async bytes => Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256', bytes)), b => b.toString(16).padStart(2, '0')).join('');

async function verifiedAsset(file, previous) {
  // Reuse an unchanged file across bundle versions, but verify its bytes again.
  const name = new URL(file.url, self.location.origin).pathname.split('/').pop();
  let response;
  for (const cacheName of previous) {
    const cache = await caches.open(cacheName);
    const old = await cache.match(`/client/${cacheName.slice(CACHE_PREFIX.length)}/${name}`);
    if (old?.headers.get('X-Client-SHA256') === file.sha256) { response = old; break; }
  }
  response ||= await fetch(new Request(new URL(file.url, self.location.origin), { credentials: 'omit', cache: 'no-cache', redirect: 'error' }));
  if (!response.ok) throw new Error('Client asset unavailable');
  const bytes = await response.arrayBuffer();
  if (bytes.byteLength > 8 * 1024 * 1024 || await digest(bytes) !== file.sha256) throw new Error('Client asset failed verification');
  const headers = new Headers(response.headers);
  headers.set('X-Client-SHA256', file.sha256);
  return [file.url, new Response(bytes, { status: 200, headers })];
}

self.addEventListener('install', event => event.waitUntil((async () => {
  const previous = (await caches.keys()).filter(versionCache);
  // Verify every response before writing. A rejected download cannot leave a partial version.
  const files = await Promise.all(CLIENT_MANIFEST.files.map(file => verifiedAsset(file, previous)));
  const exists = previous.includes(CACHE_NAME);
  try {
    const cache = await caches.open(CACHE_NAME);
    for (const [url, response] of files) await cache.put(url, response);
  } catch (error) {
    if (!exists) await caches.delete(CACHE_NAME);
    throw error;
  }
  // Normal worker waiting protects open documents. Only the person's refresh opts out.
})()));

self.addEventListener('activate', event => event.waitUntil((async () => {
  // A manual activation can leave another tab on an older module graph. Keep its assets.
  const windows = await self.clients.matchAll({ type: 'window', includeUncontrolled: true });
  if (!windows.length) {
    const previous = (await caches.keys()).filter(versionCache).filter(name => name !== CACHE_NAME);
    await Promise.all(previous.slice(0, -2).map(name => caches.delete(name)));
  }
  await self.clients.claim();
})()));

self.addEventListener('message', event => {
  if (event.data?.type === 'activate-update') event.waitUntil(self.skipWaiting());
});

self.addEventListener('fetch', event => {
  const request = event.request;
  const url = new URL(request.url);
  const match = url.pathname.match(assetPath);
  // No documents, cookies, API responses, authentication callbacks or event streams in CacheStorage.
  if (request.method !== 'GET' || url.origin !== self.location.origin || url.search || !match) return;
  event.respondWith((async () => {
    const cacheName = CACHE_PREFIX + match[1];
    if ((await caches.keys()).includes(cacheName)) {
      const response = await (await caches.open(cacheName)).match(request);
      if (response) return response;
    }
    // Never substitute the current version for a missing older asset.
    return fetch(request);
  })());
});
