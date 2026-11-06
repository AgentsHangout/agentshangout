// Run with node tests/emote_ui.cjs. Runs the shipped emote code (I143, I144, I146, #638): the
// room's bubbles from a list, each hiding at its own until, and the Emote submenu on your own card.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const room = fs.readFileSync('web/room.js', 'utf8').replace(/\r\n/g, '\n');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');

// EMOTE_GLYPHS and setEmotes from web/room.js.
const glyphStart = room.indexOf('export const EMOTE_GLYPHS = {');
assert.ok(glyphStart >= 0, 'missing EMOTE_GLYPHS');
const glyphs = room.slice(glyphStart, room.indexOf('};', glyphStart) + 2).replace('export const', 'const');
const methodStart = room.indexOf('  setEmotes(list = []) {');
assert.ok(methodStart >= 0, 'missing setEmotes');
const method = room.slice(methodStart, room.indexOf('\n  }\n', methodStart) + 4);

let now = 1_000_000;
const timers = new Map();
let next = 1;
const c = {
  Date: { now: () => now },
  setTimeout: (fn, ms) => { const id = next++; timers.set(id, { fn, at: now + ms }); return id; },
  clearTimeout: (id) => timers.delete(id),
};
vm.createContext(c);
vm.runInContext(`${glyphs}\nclass Room {\n${method}\n}\nthis.Room = Room; this.EMOTE_GLYPHS = EMOTE_GLYPHS;`, c);
const runTimers = () => {
  for (const [id, timer] of [...timers]) {
    if (timer.at <= now) { timers.delete(id); timer.fn(); }
  }
};
const el = () => ({ hidden: true, textContent: '', title: '', attrs: {}, setAttribute(k, v) { this.attrs[k] = v; } });
const r = new c.Room();
const ann = { id: 'ann', emote: el() };
const scout = { id: 'scout', emote: el() };
const bo = { id: 'bo', emote: el() };
r.people = new Map([['ann', ann], ['bo', bo]]);
r.bots = new Map([['scout', scout]]);

const unix = Math.floor(now / 1000);
r.setEmotes([
  { id: 'ann', kind: 'person', name: 'Ann', emote: 'wave', until: unix + 10 },
  { id: 'scout', kind: 'bot', name: 'Scout', emote: 'heart', until: unix + 4 },
  { id: 'gone', kind: 'person', name: 'Gone', emote: 'clap', until: unix + 10 },
]);
assert.equal(ann.emote.hidden, false);
assert.equal(ann.emote.textContent, c.EMOTE_GLYPHS.wave);
assert.equal(ann.emote.attrs['aria-label'], 'Ann emotes: wave', 'screen readers hear it');
assert.equal(scout.emote.hidden, false, 'bots show emotes too');
assert.equal(bo.emote.hidden, true, 'someone not in the list shows none');
// Each hides at its own until.
now += 4000; runTimers();
assert.equal(scout.emote.hidden, true);
assert.equal(ann.emote.hidden, false);
now += 6000; runTimers();
assert.equal(ann.emote.hidden, true);
// An expired or unknown emote never shows.
r.setEmotes([{ id: 'bo', name: 'Bo', emote: 'wave', until: Math.floor(now / 1000) - 1 }, { id: 'ann', name: 'Ann', emote: 'moon', until: Math.floor(now / 1000) + 9 }]);
assert.equal(bo.emote.hidden, true);
assert.equal(ann.emote.hidden, true);
// A newer list replaces what's showing.
r.setEmotes([{ id: 'bo', name: 'Bo', emote: 'dance', until: Math.floor(now / 1000) + 10 }]);
r.setEmotes([]);
assert.equal(bo.emote.hidden, true, 'an empty list clears it');
console.log('Emote bubbles: shown per person and bot, hidden at their own until, unknown ones skipped.');

// The Emote submenu, from web/app.js.
const menuStart = app.indexOf("const EMOTE_WORDS = [");
assert.ok(menuStart >= 0, 'missing EMOTE_WORDS');
const menu = app.slice(menuStart, app.indexOf('\n}\n', app.indexOf('function emoteMenu(', menuStart)) + 3);
const m = { EMOTE_GLYPHS: c.EMOTE_GLYPHS, wordName: (w) => w[0].toUpperCase() + w.slice(1) };
vm.createContext(m);
vm.runInContext(`${menu}\nthis.emoteMenu = emoteMenu; this.EMOTE_WORDS = EMOTE_WORDS;`, m);
assert.deepEqual([...m.EMOTE_WORDS].sort(), Object.keys(c.EMOTE_GLYPHS).sort(), 'every emote has a glyph');
const picked = [];
const sub = m.emoteMenu((word) => picked.push(word));
assert.equal(sub.label, 'Emote');
assert.equal(sub.items.length, 8);
assert.equal(sub.items[0].label, `${c.EMOTE_GLYPHS.wave} Wave`);
sub.items[7].run();
assert.deepEqual(picked, ['dance']);
// The server's list is the same eight, in the same order.
const server = fs.readFileSync('src/emotes.rs', 'utf8');
const words = server.match(/pub const EMOTES: \[&str; 8\] =\s*\[([^\]]+)\]/)[1].match(/"(\w+)"/g).map((w) => w.slice(1, -1));
assert.deepEqual(words, [...m.EMOTE_WORDS]);
// Your own card offers it; someone else's doesn't.
assert.ok(app.includes('you && emote ? emoteMenu(emote) : null,'));
assert.ok(app.includes("stream.addEventListener('emotes', (event) => room.setEmotes(JSON.parse(event.data)));"));
console.log('Emote menu: eight emotes matching the server, on your own card only.');
