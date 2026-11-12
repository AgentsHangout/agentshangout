// node tests/funds_ui.cjs: #1056 1b, a floor's Funds panel. Runs the shipped fundsPanel from
// web/app.js against a fake API: the rows, put in, cancel, start, and no Start for a non-member.
const fs = require('node:fs'), vm = require('node:vm');
process.chdir(require('node:path').join(__dirname, '..'));
(async () => {

const app = fs.readFileSync('web/app.js', 'utf8');
const start = app.indexOf('// HC59 to HC64 (#1056)');
const end = app.indexOf('// HC17: packs that cost credits can be bought here', start);
const src = app.slice(start, end);
const grabLine = (s) => { const i = app.indexOf(s); return app.slice(i, app.indexOf('\n', i) + 1); };
const helpers = [grabLine('const count = (n, one, many)'), grabLine('const sentence = (text)')].join('');
const actSrc = app.slice(app.indexOf('function act(errorEl, run) {'), app.indexOf('\n}\n', app.indexOf('function act(errorEl, run) {')) + 3);
const h = (tag, attrs, ...kids) => ({ tag, attrs: attrs || {}, kids: kids.flat().filter((k) => k != null && k !== false), replaceChildren(...k) { this.kids = k.filter((x) => x != null); } });
const calls = [];
const state = { funds: [
  { id: 'f1', item: 'winter', target: 15, total: 6, yours: 2, contributors: 2, started_by_you: true, status: 'open', ends_at: 1800000000 },
  { id: 'f2', item: 'piece:koi_pond', target: 150, total: 150, yours: 0, started_by_you: false, status: 'complete', ends_at: 1800000000 },
  { id: 'f3', item: 'piece:grand_piano', target: 250, total: 40, yours: 0, contributors: 3, started_by_you: false, status: 'refunded', ends_at: 1800000000 },
], can_start: true };
let completeNext = false;
const api = async (path, opts) => { if (path.endsWith('/put') && completeNext) { completeNext = false; return { put: 9, fund: { status: 'complete' } }; } calls.push([path, opts?.method || 'GET', JSON.stringify(opts?.body || null)]); if (path === '/api/credits') return { grand: [{ kind: 'koi_pond', label: 'Koi pond', cost: 150 }], packs: [{ pack: 'beach', cost: 25, over: false, sold_alone: [{ kind: 'crab', label: 'Crab', cost: 2 }] }, { pack: 'halloween', cost: 1, over: true }] }; if (path.endsWith('/funds') && !opts) return state; return {}; };
const events = [];
const ctx = { h, api, confirm: () => true, alert: () => {}, Date, Math, Number, String, JSON, CustomEvent: class { constructor(type, init) { this.type = type; this.detail = init?.detail; } }, dispatchEvent: (event) => events.push(event) };
vm.createContext(ctx);
vm.runInContext(helpers + actSrc + src + '\nthis.panel = fundsPanel("room1"); this.fundName = fundName;', ctx);
await ctx.panel.load();
const text = JSON.stringify(ctx.panel.element);
for (const want of ['Winter pack', 'Koi pond', '6 of 15 credits', 'you put in 2', 'Bought for this floor', 'Put in', 'Cancel', 'Start a fund', 'Beach pack · 25 credits', 'Crab · 2 credits', 'Koi pond (grand) · 150 credits']) if (!text.includes(want)) throw new Error(`missing ${want}`);
if (text.includes('Halloween')) throw new Error('an over pack is offered');
const list = ctx.panel.element.kids[0];
// #1097 (b): the bar says how many people put in, never who; a refunded fund says so as the panel does.
if (!text.includes('from 2 people')) throw new Error('no contributor count');
if (!text.includes('from 3 people')) throw new Error('refunded fund has no contributor count');
if (!text.includes('Ended: everyone got their credits back')) throw new Error('refunded state not shown');
const rows = list.kids[1].kids;
const actions = rows[0].kids[3];
const amount = actions.kids[0];
amount.value = '4';
await actions.kids[1].attrs.onclick({ currentTarget: {} });
const put = calls.find((c) => c[0] === '/api/funds/f1/put');
if (!put || put[2] !== '{"amount":4}') throw new Error(JSON.stringify(calls));
if (events.length) throw new Error('an unfinished put announced an unlock');
// #1102: a put that completes the fund tells an open build menu on this floor.
completeNext = true;
amount.value = '9';
await actions.kids[1].attrs.onclick({ currentTarget: {} });
if (events.length !== 1 || events[0].type !== 'ah-floor-unlocks' || events[0].detail.floor !== 'room1') throw new Error(JSON.stringify(events));
await actions.kids[2].attrs.onclick({ currentTarget: {} });
if (!calls.find((c) => c[0] === '/api/funds/f1/cancel')) throw new Error('no cancel');
const startRow = list.kids[2];
startRow.kids[0].value = 'piece:crab';
await startRow.kids[1].attrs.onclick({ currentTarget: {} });
const started = calls.find((c) => c[0] === '/api/floors/room1/funds' && c[1] === 'POST');
if (!started || started[2] !== '{"item":"piece:crab"}') throw new Error(JSON.stringify(calls));
state.can_start = false;
await ctx.panel.load();
if (JSON.stringify(ctx.panel.element).includes('Start a fund')) throw new Error('a non-member is offered start');
console.log("ok");
})().catch((error) => { console.error(error); process.exit(1); });
