// node tests/my_friends_ui.cjs: #796, My friends in the bar and More, reading /api/friends, with
// a friend shown where they are only on a public floor.
const fs = require('node:fs');
const assert = require('node:assert/strict');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');

// The bar's button, for a person only, and the same entry under More.
assert.ok(app.includes("me?.kind === 'person' ? panels.toggle('friends', 'My friends', { short: 'Friends', className: 'friends-button' }) : null"));
assert.ok(app.includes("me?.kind === 'person' ? moreRow('friends', 'My friends') : null"));
// Its panel reads the list when it opens, and says only what the server gave.
assert.ok(app.includes("panels.add('friends', 'My friends', { onOpen: loadFriends }"));
assert.ok(app.includes("await api('/api/friends')"));
assert.ok(app.includes("`on ${friend.here.name}`"));
assert.ok(app.includes("'not on a public floor right now'"));
// A failed read says so, rather than looking like no friends.
assert.ok(app.includes("`Couldn't load your friends: ${error.message}`"));
console.log('my friends ui: ok');
