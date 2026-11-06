// node tests/lock_reason_ui.cjs: I128, I19. A Build tile says why it can't be placed. Runs the
// shipped lockReason, friendsText and wordName from web/app.js, not just their text.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
const slice = (from, end) => {
  const start = app.indexOf(from);
  assert.ok(start >= 0, from);
  return app.slice(start, app.indexOf(end, start) + end.length);
};
const code = [
  slice('const friendsText = ', ';\n'),
  slice('const wordName = ', ';\n'),
  slice('function lockReason(', '\n}\n'),
].join('\n');
const context = {};
vm.createContext(context);
vm.runInContext(`${code}\nthis.lockReason = lockReason;`, context);
const { lockReason } = context;
const here = { isPublic: false, full: false };

// I128: a Lific board for someone without god mode says so, never "needs null friends".
const lific = { kind: 'lific_board', unlocked: false, god_needed: true, friends_needed: null };
assert.equal(lockReason(lific, here), 'needs god mode');
// I19: friends still say how many.
assert.equal(lockReason({ unlocked: false, friends_needed: 3 }, here), 'needs 3 friends');
assert.equal(lockReason({ unlocked: false, friends_needed: 1 }, here), 'needs 1 friend');
// A pack comes first, and an unlocked tile on a full floor says so.
assert.match(lockReason({ unlocked: false, pack_needed: 'fall', god_needed: true }, here), /^in the fall pack/);
assert.equal(lockReason({ unlocked: true }, { isPublic: false, full: true }), 'this floor is full');
assert.equal(lockReason({ unlocked: true, mood: 'flirty' }, { isPublic: true, full: false }), 'private floors only');
assert.equal(lockReason({ unlocked: true }, here), null);
console.log('Build tiles say why they are locked, god mode included (run, not just read).');
