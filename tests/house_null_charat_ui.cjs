// Run with node tests/house_null_charat_ui.cjs
// Issue #831: Guard null .charAt in web/app.js when opening a house/room where an item has does: null
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');

const appSource = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');

// Extract `sentence` function definition from web/app.js
const sentenceMatch = appSource.match(/const sentence = [^;\n]+;/);
assert.ok(sentenceMatch, 'sentence function must exist in web/app.js');

const context = {
  console,
  STATE_TEXT: { idle: 'Idle', active: 'Active' },
  DOING_TEXT: { coding: 'Coding' },
  doingText: (a) => a.kind,
  liveThinking: () => null,
};
vm.createContext(context);
vm.runInContext(`${sentenceMatch[0]}; this.sentence = sentence;`, context);

// 1. Verify sentence(null) and sentence(undefined) do not throw
assert.doesNotThrow(() => {
  assert.equal(context.sentence(null), '');
  assert.equal(context.sentence(undefined), '');
  assert.equal(context.sentence(''), '');
  assert.equal(context.sentence('reading notes'), 'Reading notes');
}, 'sentence must safely handle null and undefined without throwing');

// 2. Verify doingOf(agent) when usedBy returns an item with does: null
const doingOfMatch = appSource.match(/function doingOf\(agent\) \{[\s\S]*?\n  \}/);
assert.ok(doingOfMatch, 'doingOf function must exist in web/app.js');

context.snapshot = {
  items: [
    { id: 'item-1', kind: 'whiteboard', does: null, used_by: ['bot-1'] },
    { id: 'item-2', kind: 'shared_notes_station', does: 'reading shared notes', used_by: ['bot-2'] },
  ],
};
context.usedBy = (agent) => context.snapshot.items.find((item) => item.used_by?.includes(agent.id));

vm.runInContext(`${doingOfMatch[0]}; this.doingOf = doingOf;`, context);

// Bot 1 is using an item with does: null (e.g. whiteboard)
const bot1 = { id: 'bot-1', state: 'idle', status: null, activity: null, thinking: null, away: false };
assert.doesNotThrow(() => {
  const result = context.doingOf(bot1);
  assert.ok(result, 'doingOf must return a valid string for bot using item with does: null');
  assert.notEqual(result, '', 'doingOf should return description or fallback');
}, 'doingOf must not crash with Cannot read properties of null (reading charAt) when item.does is null');

// Bot 2 is using an item with does: 'reading shared notes'
const bot2 = { id: 'bot-2', state: 'idle', status: null, activity: null, thinking: null, away: false };
assert.equal(context.doingOf(bot2), 'Reading shared notes');

console.log('tests/house_null_charat_ui.cjs passed cleanly');
