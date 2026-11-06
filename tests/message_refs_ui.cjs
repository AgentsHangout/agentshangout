// node tests/message_refs_ui.cjs: I131 (#640). A line's `re 7068`, `re: 7068` or `#7068` is a link
// when 7068 is a message already shown in the room, and nothing else is. Runs the shipped
// messageRefs from web/app.js, not just its text.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
const start = app.indexOf('function messageRefs(');
assert.ok(start >= 0, 'messageRefs is in app.js');
const code = app.slice(start, app.indexOf('\n}\n', start) + 3);
const context = {};
vm.createContext(context);
vm.runInContext(`${code}\nthis.messageRefs = messageRefs;`, context);
const { messageRefs } = context;

const shown = new Set([7068, 7032, 641]);
const known = (id) => shown.has(id);
const refs = (text) => JSON.parse(JSON.stringify(messageRefs(text, known)));

// Each form, for a message in the room.
assert.deepEqual(refs('re 7068 sounds good'), [{ text: 're 7068', ref: 7068 }, { text: ' sounds good' }]);
assert.deepEqual(refs('Re: 7032, done'), [{ text: 'Re: 7032', ref: 7032 }, { text: ', done' }]);
assert.deepEqual(refs('see #7068 and #7032'),
  [{ text: 'see ' }, { text: '#7068', ref: 7068 }, { text: ' and ' }, { text: '#7032', ref: 7032 }]);

// Numbers that aren't a message shown here stay text: an older message, a GitHub issue, a port.
assert.deepEqual(refs('re 6000, #636 and 127.0.0.1:4510'), [{ text: 're 6000, #636 and 127.0.0.1:4510' }]);
// "are 7068" isn't a ref: "re" has to start a word.
assert.deepEqual(refs('there are 7068 lines'), [{ text: 'there are 7068 lines' }]);
// A bare number isn't one either, only re and #.
assert.deepEqual(refs('7068 was right'), [{ text: '7068 was right' }]);
// The words come back as they were, only split.
for (const text of ['re 7068', 'x #7032 y', 'plain', '', 're 641!']) {
  assert.equal(refs(text).map((part) => part.text).join(''), text, text);
}
// The room view turns the pieces into links: it renders through messageRefs.
assert.ok(app.includes('messageRefs(message.body, (id) => shownById.has(id))'));
console.log('Message refs link only to messages shown in the room (run, not just read).');
