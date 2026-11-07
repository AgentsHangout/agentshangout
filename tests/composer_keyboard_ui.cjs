// node tests/composer_keyboard_ui.cjs: M12 (#835). On a phone, focusing the chat box hides the bottom
// tabs and docks the open sheet on the keyboard; leaving it puts everything back. Runs the shipped
// dockComposer against a stub page and keyboard, not just its text.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
const start = app.indexOf('function dockComposer(root, view = window.visualViewport) {');
assert.ok(start >= 0, 'dockComposer is in web/app.js');
const code = app.slice(start, app.indexOf('\n}\n', start) + 3);

const listeners = (target) => { target.on = {}; target.addEventListener = (name, fn) => { (target.on[name] ||= []).push(fn); }; return target; };
const classes = new Set(['apple-hig']);
const root = { classList: { contains: (c) => classes.has(c), toggle: (c, on) => (on ? classes.add(c) : classes.delete(c)) }, style: { vars: {}, setProperty(k, v) { this.vars[k] = v; } } };
const inChat = { closest: (sel) => (sel === '.chat-form' ? {} : null) };
const elsewhere = { closest: () => null };
const document = listeners({ activeElement: elsewhere });
const view = listeners({ height: 852, offsetTop: 0 });
const timers = [];
const context = { document, window: { innerHeight: 852, visualViewport: view }, setTimeout: (fn) => timers.push(fn) };
vm.createContext(context);
vm.runInContext(`${code}\nthis.dockComposer = dockComposer;`, context);
context.dockComposer(root, view);
assert.ok(!classes.has('typing'));
assert.equal(root.style.vars['--keyboard'], '0px');

// The keyboard opens over the chat box: only the visual viewport shrinks (Android's default).
document.activeElement = inChat;
view.height = 512;
document.on.focusin.forEach((fn) => fn());
view.on.resize.forEach((fn) => fn());
assert.ok(classes.has('typing'), 'typing hides the tabs');
assert.equal(root.style.vars['--keyboard'], '340px', 'the sheet ends where the keyboard starts');

// With the page itself shrunk for the keyboard there's nothing to lift.
context.window.innerHeight = 512;
view.on.resize.forEach((fn) => fn());
assert.equal(root.style.vars['--keyboard'], '0px');

// Leaving the box puts the tabs back, with no gap left over.
document.activeElement = elsewhere;
context.window.innerHeight = 852; view.height = 852;
document.on.focusout.forEach((fn) => fn());
timers.splice(0).forEach((fn) => fn());
assert.ok(!classes.has('typing'));
assert.equal(root.style.vars['--keyboard'], '0px');

// On a desktop nothing changes, even in a chat box.
classes.delete('apple-hig');
document.activeElement = inChat;
document.on.focusin.forEach((fn) => fn());
assert.ok(!classes.has('typing'));
console.log('Composer keyboard: the chat box docks on a phone keyboard (run, not just read).');
