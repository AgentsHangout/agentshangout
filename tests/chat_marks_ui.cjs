// node tests/chat_marks_ui.cjs: I148 (#711). A room line shows `code`, **bold**, __bold__, *italic*
// and _italic_ as elements, and everything else as the text it was. Runs the shipped h, messageRefs,
// chatMarks and chatText from web/app.js against a small fake DOM, not just their text.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
const take = (marker, end = '\n}\n') => {
  const start = app.indexOf(marker);
  assert.ok(start >= 0, `${marker} is in app.js`);
  return app.slice(start, app.indexOf(end, start) + end.length);
};
const code = [
  take('function h('),
  take('function messageRefs('),
  take('const CHAT_MARKS =', ';\n'),
  take('function chatMarks('),
  take('function chatText('),
].join('\n');

// Just enough DOM for h: elements keep their children, strings stay strings, and HTML is refused.
class FakeElement {
  constructor(tag) { this.tagName = tag.toUpperCase(); this.children = []; }
  setAttribute(key, value) { this[key] = value; }
  addEventListener() {}
  append(...nodes) { this.children.push(...nodes); }
  set innerHTML(_) { throw new Error('innerHTML is never used for a chat line'); }
}
const context = { document: { createElement: (tag) => new FakeElement(tag) } };
vm.createContext(context);
vm.runInContext(`${code}\nthis.chatMarks = chatMarks; this.chatText = chatText; this.messageRefs = messageRefs;`, context);
const { chatMarks, chatText, messageRefs } = context;

const shown = new Set([7068]);
const link = ({ text, ref }) => ({ link: ref, text });
// A line as the room draws it, written back out: <tag>text</tag> for elements, text as it is.
const draw = (text) => chatText(messageRefs(text, (id) => shown.has(id)), link).map((node) => {
  if (typeof node === 'string') return node;
  if (node.link) return `[${node.text}]`;
  assert.ok(node.children.every((child) => typeof child === 'string'), 'nothing nests');
  return `<${node.tagName.toLowerCase()}>${node.children.join('')}</${node.tagName.toLowerCase()}>`;
}).join('');

// Bold both ways, italic both ways, and code.
assert.equal(draw('this is **important** ok'), 'this is <strong>important</strong> ok');
assert.equal(draw('__also bold__, yes'), '<strong>also bold</strong>, yes');
assert.equal(draw('a *quiet* word and an _other_ one'), 'a <em>quiet</em> word and an <em>other</em> one');
assert.equal(draw('run `cargo test --locked` first'), 'run <code>cargo test --locked</code> first');
assert.equal(draw('**one** and **two**'), '<strong>one</strong> and <strong>two</strong>');
assert.equal(draw('(**done**!) *ok*.'), '(<strong>done</strong>!) <em>ok</em>.');
// Code is taken as written, markers inside it included.
assert.equal(draw('`a_b_c **x**`'), '<code>a_b_c **x**</code>');

// Bullets stay bullets, with bold inside them.
assert.equal(draw('* **Fix** the door\n* tidy up\n- *maybe* later'),
  '* <strong>Fix</strong> the door\n* tidy up\n- <em>maybe</em> later');
assert.equal(draw('* one\n* two'), '* one\n* two');

// Unbalanced or unclosed markers stay as they were, and so do markers with a space inside.
for (const text of ['**not closed', 'not opened**', '*half', 'a ** b ** c', 'a * b * c', '`open code',
  '** spaced **', '_ spaced _', '**bold*', 'x\n**y\nz**']) {
  assert.equal(draw(text), text, text);
}

// snake_case, maths and paths stay as written.
for (const text of ['snake_case_name', 'my_var and other_var', '_private_var', '2*3*4', 'file_name_v2.txt',
  'https://example.com/a_b_c', 'C:\\some_dir\\a_b']) {
  assert.equal(draw(text), text, text);
}

// Only asterisks, or only underscores, is just that.
for (const text of ['*', '**', '***', '****', '******', '__', '____', '``']) {
  assert.equal(draw(text), text, text);
}

// HTML is text: the pieces are strings, which h appends as text nodes.
assert.equal(draw('<b>hi</b> <script>alert(1)</script>'), '<b>hi</b> <script>alert(1)</script>');
assert.deepEqual(JSON.parse(JSON.stringify(chatMarks('<b>x</b>'))), [{ text: '<b>x</b>' }]);
assert.equal(draw('**<img src=x onerror=alert(1)>**'), '<strong><img src=x onerror=alert(1)></strong>');
const strong = chatText([{ text: '**<script>x</script>**' }], link)[0];
assert.equal(strong.tagName, 'STRONG');
assert.deepEqual(strong.children, ['<script>x</script>']);

// Next to a link to an earlier message and a mention: the link stays a link, a mention stays text,
// and no marker reaches across the link.
assert.equal(draw('**re 7068** is done'), '**[re 7068]** is done');
assert.equal(draw('**see** re 7068 *now*'), '<strong>see</strong> [re 7068] <em>now</em>');
assert.equal(draw('@Grok Bot **look** at #7068'), '@Grok Bot <strong>look</strong> at [#7068]');
assert.equal(draw('**@Grok Bot** hi'), '<strong>@Grok Bot</strong> hi');
assert.equal(draw('@grok_bot said hi'), '@grok_bot said hi');
// A number that isn't a message shown here is still text, and markers around it still work.
assert.equal(draw('**#636** is open'), '<strong>#636</strong> is open');

// The room view draws its lines through chatText, and never as HTML.
assert.ok(app.includes("h('p', {}, ...chatText(messageRefs(message.body, (id) => shownById.has(id)), refLink))"));
assert.ok(!/innerHTML[^\n]*message\.body/.test(app), 'no line is set as HTML');
console.log('Chat lines show bold, italic and code, and everything else as text (run, not just read).');
