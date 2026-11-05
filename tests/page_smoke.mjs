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
import { appRoot, failures, fail, goTo } from './dom_stub.mjs';

const [base, key, ...paths] = process.argv.slice(2);
if (!base || !key || paths.length !== 1) {
  console.error('usage: page_smoke.mjs <base> <key> <path>');
  process.exit(2);
}

goTo(new URL(paths[0], base).href);

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
