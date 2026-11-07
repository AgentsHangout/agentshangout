// Run with node tests/mayor_sash_ui.cjs. Runs the shipped sash code (MY10, #714): drawSash paints a
// gold band from one shoulder to the other hip, mirrored from behind, and only the mayor wears it.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const room = fs.readFileSync('web/room.js', 'utf8').replace(/\r\n/g, '\n');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');

const take = (start, end) => {
  const from = room.indexOf(start);
  assert.ok(from >= 0, `missing ${start}`);
  return room.slice(from, room.indexOf(end, from) + end.length);
};
const rHelper = take('const r = (g, color, x, y, w, h) => {', '\n};\n');
const sash = take("const SASH = '#e8b923';", '\n}\n').replace('export function', 'function');
const c = {};
vm.createContext(c);
vm.runInContext(`${rHelper}\n${sash}\nthis.drawSash = drawSash; this.SASH = SASH; this.SASH_EDGE = SASH_EDGE;`, c);

// A canvas that remembers which pixels got which color.
const canvas = () => {
  const px = new Map();
  return {
    px,
    fillStyle: '',
    fillRect(x, y, w, h) {
      for (let i = 0; i < w; i++) for (let j = 0; j < h; j++) px.set(`${x + i},${y + j}`, this.fillStyle);
    },
  };
};
const front = canvas();
c.drawSash(front, 10, 20, 8, 6);
// Left shoulder at the top, right hip at the bottom, gold all the way.
assert.equal(front.px.get('10,20'), c.SASH, 'starts at the left shoulder');
assert.equal(front.px.get('11,20'), c.SASH);
assert.equal(front.px.get('16,25'), c.SASH, 'ends at the right hip');
assert.equal(front.px.get('17,25'), c.SASH);
for (let row = 20; row < 26; row++) {
  const gold = [...front.px].filter(([k, v]) => k.endsWith(`,${row}`) && v === c.SASH).length;
  assert.equal(gold, 2, `two gold pixels on row ${row}`);
}
assert.ok([...front.px.values()].includes(c.SASH_EDGE), 'it has an edge');
// Seen from behind it runs the other way.
const back = canvas();
c.drawSash(back, 10, 20, 8, 6, true);
assert.equal(back.px.get('16,20'), c.SASH, 'from behind it starts at the right shoulder');
assert.equal(back.px.get('10,25'), c.SASH, 'and ends at the left hip');
console.log('Sash: a gold band shoulder to hip, mirrored from behind.');

// Only the mayor wears it: both figures draw it behind their own flag, and the page sets the flag
// only for the person or bot the server names.
assert.ok(room.includes("if (person.mayor) drawSash(g, x - 4, torso, 8, 6, facing === 'back');"));
assert.ok(room.includes("if (bot.mayor) drawSash(g, x - 4, base - 6, 8, 5, bot.facing === 'back');"));
assert.ok(room.includes('person.mayor = Boolean(data.mayor);'));
assert.ok(room.includes('bot.mayor = Boolean(data.mayor);'));
const wears = app.slice(app.indexOf('const wearsSash = '), app.indexOf(';\n', app.indexOf('const wearsSash = ')) + 1);
assert.ok(wears.length > 20, 'missing wearsSash');
const w = { snapshot: { mayor: { kind: 'bot', id: 'eagle' } } };
vm.createContext(w);
vm.runInContext(`${wears}\nthis.wearsSash = wearsSash;`, w);
assert.equal(w.wearsSash('bot', 'eagle'), true);
assert.equal(w.wearsSash('person', 'eagle'), false, 'a person with the same id is not the mayor');
assert.equal(w.wearsSash('bot', 'scout'), false);
w.snapshot.mayor = null;
assert.equal(w.wearsSash('bot', 'eagle'), false, 'no mayor here, no sash');
assert.ok(app.includes("mayor: wearsSash('bot', agent.id),"));
assert.ok(app.includes("mayor: wearsSash('person', p.id) })));"));
console.log('Sash: worn only by the mayor the server names, person or bot.');
