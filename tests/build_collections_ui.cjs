// node tests/build_collections_ui.cjs: run the shipped filter and picker helpers.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const source = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
function extract(name) { const start = source.indexOf(`function ${name}(`); assert.ok(start >= 0); return source.slice(start, source.indexOf('\n}\n', start) + 2); }
const h = (tag, props = {}, ...children) => ({ tag, ...props, children: children.flat().filter(Boolean) });
const context = { h, field: (label, control) => h('label', {}, label, control) };
vm.createContext(context);
vm.runInContext([extract('selectedBuildCollection'), extract('collectionOffers'), extract('buildCollectionPicker')].join('\n'), context);
const catalog = { items: [{ kind: 'microscope', category: 'work', unlocked: true }, { kind: 'telescope', category: 'electronics', unlocked: false }, { kind: 'chair', category: 'seating', unlocked: true }], collections: [{ id: 'discovery', label: 'Discovery & evidence', description: 'Observe and compare.', items: ['microscope', 'telescope'], parts: ['rooftop'] }, { id: 'welcome', label: 'Welcome & recharge', items: ['chair'] }] };
const before = JSON.stringify(catalog);
const filtered = id => { const collection = context.selectedBuildCollection(catalog, id); return catalog.items.filter(item => context.collectionOffers(collection, 'items', item.kind)); };
assert.equal(filtered('').length, catalog.items.length, 'All restores every item');
assert.deepEqual(filtered('discovery').map(i => i.kind), ['microscope', 'telescope'], 'collections cross existing categories');
assert.equal(filtered('discovery')[1].unlocked, false, 'filtering never unlocks a locked item');
assert.deepEqual(filtered('welcome').map(i => i.kind), ['chair']);
assert.equal(filtered('unknown').length, catalog.items.length, 'a removed collection falls back to All');
const collection = context.selectedBuildCollection(catalog, 'discovery');
assert.equal(context.collectionOffers(collection, 'parts', 'rooftop'), true);
assert.equal(context.collectionOffers(collection, 'parts', 'pets'), false);
assert.equal(context.collectionOffers(null, 'parts', 'pets'), true, 'All keeps unrelated existing groups available');
let chosen;
const picker = context.buildCollectionPicker(catalog, collection, id => { chosen = id; });
const select = picker.children[0].children[1];
assert.equal(select.children[0].children[0], 'All');
select.onchange({ target: { value: '' } });
assert.equal(chosen, '');
assert.equal(JSON.stringify(catalog), before, 'switching collections changes no catalog or decoration document');
const yard = fs.readFileSync('web/outside-yard.js', 'utf8').replace(/\r\n/g, '\n');
const start = yard.indexOf('function frame(');
vm.runInContext(yard.slice(start, yard.indexOf('\n}\n', start) + 2), context);
const points = new Set();
for (let facing = 0; facing < 4; facing++) { const frame = context.frame({ u0: 0, v0: 0, u1: 2, v1: 2, facing }, facing % 2 === 0); points.add(frame.m(0.25, 0.5).join(',')); }
assert.equal(points.size, 4, 'square science objects draw in all four orientations');
console.log('Build collections: All, cross-category filtering, locked items, parts, no mutations and four orientations verified.');
