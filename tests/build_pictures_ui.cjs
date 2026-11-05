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
// A picture that can't be drawn isn't tried again, and gives no canvas.
let tried = 0;
assert.equal(context.buildPicture('item:unknown', () => { tried += 1; return null; }), null);
context.buildPicture('item:unknown', () => { tried += 1; return null; });
assert.equal(tried, 1);
// The cache can't grow without end.
for (let n = 0; n < 1000; n++) context.buildPicture(`many:${n}`, paint);
assert.ok(context.BUILD_PICTURES.size <= 600, `held ${context.BUILD_PICTURES.size}`);
// Every picture the catalog draws goes through it, and reopening an unchanged catalog keeps its tiles.
for (const call of ['buildPicture(`item:', 'buildPicture(`room:', 'buildPicture(`wall:', 'buildPicture(`floor:', 'buildPicture(`furniture:']) {
  assert.ok(source.includes(call), `${call} is cached`);
}
assert.ok(source.includes("const same = catalog && JSON.stringify(fresh) === JSON.stringify(catalog);"));
console.log('Build pictures: painted once, copied per tile, bounded, and an unchanged catalog keeps its tiles.');
