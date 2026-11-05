// node tests/my_bots_view_ui.cjs: S86, one My bots button and full-screen view for Trust, Bots and Crew.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
const css = fs.readFileSync('web/trust.css', 'utf8').replace(/\r\n/g, '\n');

// The map: one bolder My bots button, a My bots panel that holds the list and Trust, no Trust launcher.
assert.ok(app.includes("panels.toggle('bots', 'My bots', { short: 'Bots', className: 'my-bots-button' })"));
assert.ok(app.includes("panels.add('bots', 'My bots', { onOpen: () => { loadBots(true); trust?.load(); } }, botsBody, trustHost)"));
assert.ok(app.includes("host: trustHost, hostPanel: 'bots'"));
assert.ok(!app.includes('trust-launcher') && !css.includes('trust-launcher'), 'no separate Trust launcher');
assert.ok(!app.includes("moreRow('bots'"), 'My bots never folds into More, so More has no row for it');
// The phone tab bar: one My bots tab in place of Trust and Crew.
assert.ok(app.includes("{ id: 'bots', label: 'My bots', icon: '👥'"));
assert.ok(!app.includes("{ id: 'trust', label: 'Trust'") && !app.includes("{ id: 'crew', label: 'Crew'"));
// A room: the same button, opening on who's in this room, then your own bots (not for a guest).
assert.ok(app.includes("panels.toggle('crew', 'My bots', { short: 'Bots', className: 'my-bots-button' })"));
assert.ok(app.includes("panels.add('crew', 'My bots', { onOpen: () => roomTrust?.load() },\n    h('h3', { class: 'my-bots-here' }, 'In this room'),"));
assert.ok(app.includes("me?.kind === 'person' && !snapshot.guest"));
// Full screen, on both.
assert.ok(css.includes('.wd-game > .game-panel-bots, .wd-game > .game-panel-crew { width: calc(100% - 24px)'));

// Trust inside a host: draws into it, has no panel or button of its own, and opens the host panel.
const h = (tag, props = {}, ...children) => ({ tag, props, children, classList: { add() {} }, append() {}, replaceChildren() {} });
const added = [], toggled = [], opened = [];
const panels = { add: (name) => { added.push(name); return h('div'); }, toggle: (name) => { toggled.push(name); return h('button'); }, open: (name) => opened.push(name) };
const context = { h, api: async () => ({}), panels, document: { createElement: () => ({}) }, console };
vm.createContext(context);
vm.runInContext(fs.readFileSync('web/trust.js', 'utf8').replace('export function trustPanel', 'function trustPanel') + '\nthis.trustPanel = trustPanel;', context);
const host = h('section');
const hosted = context.trustPanel({ h, api: context.api, panels, me: { kind: 'person' }, host, hostPanel: 'bots' });
assert.deepEqual([added, toggled], [[], []], 'no panel and no top-bar button of its own');
assert.equal(hosted.toggle, null);
hosted.open();
assert.deepEqual(opened, ['bots'], 'opening it opens the My bots view');
assert.equal(typeof hosted.load, 'function');
// Without a host it is the panel it always was.
const alone = context.trustPanel({ h, api: context.api, panels, me: { kind: 'person' } });
assert.deepEqual([added, toggled], [['trust'], ['trust']]);
alone.open();
assert.deepEqual(opened, ['bots', 'trust']);
console.log('My bots view: one button and view on the map and in a room, Trust hosted inside it, full screen.');
