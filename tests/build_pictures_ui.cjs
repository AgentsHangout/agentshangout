// node tests/build_pictures_ui.cjs: #529, the build catalog draws each picture once and copies it after.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const source = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
function extract(name) { const start = source.indexOf(`function ${name}(`); assert.ok(start >= 0, name); return source.slice(start, source.indexOf('\n}\n', start) + 2); }
const constant = (name) => { const start = source.indexOf(`const ${name} =`); assert.ok(start >= 0, name); return source.slice(start, source.indexOf(';\n', start) + 2); };
let drawnCopies = 0;
const canvas = () => ({ width: 0, height: 0, getContext: () => ({ drawImage: () => { drawnCopies += 1; } }) });
const context = { document: { createElement: (tag) => { assert.equal(tag, 'canvas'); return canvas(); } } };
vm.createContext(context);
vm.runInContext([constant('BUILD_PICTURES'), constant('MAX_BUILD_PICTURES'), extract('buildPicture'), 'this.buildPicture = buildPicture; this.BUILD_PICTURES = BUILD_PICTURES;'].join('\n'), context);

let painted = 0;
const paint = () => { painted += 1; return { width: 40, height: 30 }; };
const first = context.buildPicture('room:a', paint);
const second = context.buildPicture('room:a', paint);
assert.equal(painted, 1, 'a picture is painted once, however many times the catalog opens');
assert.notEqual(first, second, 'each tile gets its own canvas, since one element can only be in one place');
assert.deepEqual([second.width, second.height], [40, 30], 'the copy is the same size');
assert.equal(drawnCopies, 2, 'each copy is one drawImage');
context.buildPicture('room:b', paint);
assert.equal(painted, 2, 'a different picture is painted');
// A picture that can't be drawn gives no canvas and isn't kept, so it's tried again next time.
let tried = 0;
assert.equal(context.buildPicture('item:unknown', () => { tried += 1; return null; }), null);
context.buildPicture('item:unknown', () => { tried += 1; return null; });
assert.equal(tried, 2);
assert.equal(context.BUILD_PICTURES.has('item:unknown'), false);
assert.equal(context.buildPicture('item:blank', () => ({ width: 0, height: 0 })).width, 0, 'an empty canvas is handed back as it is');
assert.equal(context.BUILD_PICTURES.has('item:blank'), false);
// The cache can't grow without end.
for (let n = 0; n < 1000; n++) context.buildPicture(`many:${n}`, paint);
assert.ok(context.BUILD_PICTURES.size <= 600, `held ${context.BUILD_PICTURES.size}`);
// Every picture the catalog draws goes through it, and reopening an unchanged catalog keeps its tiles.
for (const call of ['buildPicture(`item:', 'buildPicture(`room:', 'buildPicture(`wall:', 'buildPicture(`floor:', 'buildPicture(`furniture:']) {
  assert.ok(source.includes(call), `${call} is cached`);
}
assert.ok(source.includes("const same = catalog && JSON.stringify(fresh) === JSON.stringify(catalog);"));

// #748: toggling See the room / Catalog within one build session skips redundant /api/items fetches
// unless ah-credits spent changed; ending build mode or spending credits fetches again.
function extractInner(name) {
  const start = source.indexOf(`  async function ${name}(`) >= 0
    ? source.indexOf(`  async function ${name}(`)
    : source.indexOf(`  function ${name}(`);
  assert.ok(start >= 0, name);
  const end = source.indexOf('\n  }\n', start);
  assert.ok(end >= 0, `${name} end`);
  return source.slice(start, end + 4);
}
const onCreditsStart = source.indexOf('  function onCredits(');
assert.ok(onCreditsStart >= 0, 'onCredits is defined in buildMode');
const onCreditsCode = extractInner('onCredits');
const openCatalogCode = extractInner('openCatalog');
const onFloorUnlocksCode = extractInner('onFloorUnlocks');

(async () => {
  let fetches = 0;
  let renders = 0;
  let catalogPayload = { items: [{ kind: 'stool' }], look: { packs_bought: [] } };
  const catalogCtx = {
    catalog: null,
    catalogFresh: false,
    catalogSpent: null,
     catalogKey: 'k1',
    placedKey: 'p1',
    buildError: { hidden: true, textContent: '' },
    panels: { current: null },
    id: 'room1',
    api: async (path) => {
      // #1102: the catalog is read for the floor build mode is on.
      assert.equal(path, '/api/items?floor=room1');
      fetches += 1;
      return JSON.parse(JSON.stringify(catalogPayload));
    },
    render: () => { renders += 1; },
  };
  vm.createContext(catalogCtx);
  vm.runInContext(`${onCreditsCode}\n${onFloorUnlocksCode}\n${openCatalogCode}\nthis.onCredits = onCredits; this.onFloorUnlocks = onFloorUnlocks; this.openCatalog = openCatalog;`, catalogCtx);

  // 1. Opening Build the first time fetches /api/items once.
  await catalogCtx.openCatalog();
  assert.equal(fetches, 1);
  assert.equal(renders, 1);
  assert.equal(catalogCtx.catalogFresh, true);

  // 2. Toggling See the room / Catalog within the same build session (no spend) makes no fetch.
  await catalogCtx.openCatalog();
  assert.equal(fetches, 1, 'toggle with no spend makes no /api/items fetch');
  assert.equal(renders, 2, 'toggle still renders the catalog panel');

  // 3. Initial ah-credits with spent: 0 (or unchanged spent) does not invalidate the session catalog.
  catalogCtx.onCredits({ detail: { spent: 0 } });
  await catalogCtx.openCatalog();
  assert.equal(fetches, 1, 'unchanged spent makes no fetch');

  // 4. Spending credits invalidates catalogFresh so the next Catalog toggle fetches /api/items.
  catalogPayload = { items: [{ kind: 'stool' }], look: { packs_bought: ['stargazer'] } };
  catalogCtx.onCredits({ detail: { spent: 10 } });
  assert.equal(catalogCtx.catalogFresh, false);
  await catalogCtx.openCatalog();
  assert.equal(fetches, 2, 'spending credits triggers a fresh /api/items fetch');
  assert.equal(catalogCtx.catalogKey, null, 'changed catalog clears catalogKey so tiles redraw');

  // 5. Spending credits while the Build panel is open refreshes openCatalog immediately.
  catalogCtx.panels.current = 'build';
  catalogCtx.onCredits({ detail: { spent: 15 } });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(fetches, 3, 'spending while Build is open refreshes the catalog');

  // 6. #1102: a fund completing on another floor changes nothing here; on this floor, the open
  // Build panel reads the catalog again, so what the fund bought shows unlocked.
  catalogCtx.onFloorUnlocks({ detail: { floor: 'elsewhere' } });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(fetches, 3, 'another floor\'s fund makes no fetch');
  catalogCtx.onFloorUnlocks({ detail: { floor: 'room1' } });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(fetches, 4, 'this floor\'s completed fund refreshes the open catalog');

  console.log('Build pictures: painted once, copied per tile, bounded, and openCatalog skips redundant session fetches unless credits are spent.');
})().catch((err) => {
  console.error(err);
  process.exit(1);
});
