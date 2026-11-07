// Run with node tests/off_map_ui.cjs. Runs the shipped My bots row (S86, S89): a bot in a room with
// no spot on the map says so, and a bot the map can center on keeps its Center button.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');

const from = app.indexOf('const offMapText = (bot) =>');
assert.ok(from >= 0, 'missing offMapText');
const to = app.indexOf('\n}\n', app.indexOf('function yourBotRow(', from)) + 3;
const code = app.slice(from, to);

// A tiny h() that keeps tags, classes and text, so the row can be searched.
const h = (tag, attrs, ...kids) => ({ tag, attrs: attrs || {}, kids: kids.flat().filter((k) => k !== null && k !== undefined && k !== false) });
const text = (node) => (typeof node === 'string' ? node : node.kids.map(text).join(' '));
const find = (node, pred) => {
  if (typeof node === 'string') return [];
  return [...(pred(node) ? [node] : []), ...node.kids.flatMap((kid) => find(kid, pred))];
};
const c = {
  h,
  zoomed: () => h('canvas', {}),
  figurePicture: () => ({}),
  LOOK_ZOOM_SMALL: 1,
  statusText: () => 'Idle',
  where: (bot) => (bot.room ? `In ${bot.room.name}` : 'Not in any room right now'),
  DOING: {},
  agoText: () => 'now',
  untilText: () => 'soon',
  NEXT: {},
  link: (href, label) => h('a', { href }, label),
  routineDialog: async () => {},
  deleteBotDialog: () => {},
  alert: () => {},
};
vm.createContext(c);
vm.runInContext(`${code}\nthis.yourBotRow = yourBotRow; this.offMapText = offMapText;`, c);

const lab = { name: 'axiom', room: { id: 'r1', name: 'Innovation Lab' }, status: { state: 'idle' }, doing: 'chat' };
const row = c.yourBotRow(lab, { offMap: true, reload: () => {} });
const off = find(row, (n) => String(n.attrs.class || '').includes('off-map'));
assert.equal(off.length, 1, 'the row says it is off the map');
assert.equal(text(off[0]), "Innovation Lab isn't on the map, so there's nowhere to center on it");
assert.equal(find(row, (n) => n.tag === 'button' && n.kids.includes('Center')).length, 0, 'no Center');

// A bot the map can center on keeps Center and says nothing about the map.
const onMap = c.yourBotRow({ ...lab, room: { id: 'r2', name: 'Fairground' } }, { focusOn: () => {}, reload: () => {} });
assert.equal(find(onMap, (n) => String(n.attrs.class || '').includes('off-map')).length, 0);
assert.equal(find(onMap, (n) => n.tag === 'button' && n.kids.includes('Center')).length, 1);
// Outside the map (a room's My bots), there's no such line either.
const inRoom = c.yourBotRow(lab, { reload: () => {} });
assert.equal(find(inRoom, (n) => String(n.attrs.class || '').includes('off-map')).length, 0);
assert.equal(c.offMapText({}), "Its room isn't on the map, so there's nowhere to center on it");

// The map's list marks a bot off the map only when it's in a room and has no tile.
assert.ok(app.includes('const offMap = !tile && Boolean(bot.room);'));
assert.ok(app.includes('return yourBotRow(bot, { focusOn, offMap, reload:'));
console.log('My bots: a bot in a room off the map says so; one on the map keeps Center.');
