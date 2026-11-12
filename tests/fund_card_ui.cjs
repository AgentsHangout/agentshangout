// node tests/fund_card_ui.cjs: #1097 (c), the fund card on the room floor. Runs the shipped
// fundCardsFor, fundCardText and roomFundCards from web/app.js against a fake room and API:
// open, funded, ended (refunded), cancelled, an old fund (nothing), and a grand piece not placed yet.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const path = require('node:path');
process.chdir(path.join(__dirname, '..'));
(async () => {

const app = fs.readFileSync('web/app.js', 'utf8');
// The same slice funds_ui.cjs runs: the fund helpers, then the card code just before HC17.
const start = app.indexOf('// HC59 to HC64 (#1056)');
const end = app.indexOf('// HC17: packs that cost credits can be bought here', start);
const src = app.slice(start, end);
const grabLine = (s) => { const i = app.indexOf(s); return app.slice(i, app.indexOf('\n', i) + 1); };
const helpers = [grabLine('const count = (n, one, many)'), grabLine('const sentence = (text)')].join('');
const h = (tag, attrs, ...kids) => ({ tag, attrs: attrs || {}, kids: kids.flat().filter((k) => k != null && k !== false) });
const api = async (path) => { calls.push(path); return { funds: state.funds }; };
const calls = [];
const state = { funds: [] };
let fakeNowMs = 1800000000 * 1000;
const ctx = { h, api, Date: { now: () => fakeNowMs }, Math, Number, String, JSON, Intl, Infinity, Promise, console, W: 480, H: 252 };
vm.createContext(ctx);
vm.runInContext(helpers + src + `
this.fundCardsFor = fundCardsFor; this.fundCardText = fundCardText; this.roomFundCards = roomFundCards;`, ctx);

// Objects from the vm context have their own prototypes, so compare them as JSON.
const same = (a, b) => assert.equal(JSON.stringify(a), JSON.stringify(b));
const NOW = 1800000000;
const DAY = 86400;
const fund = (over) => ({ id: 'f', item: 'piece:grand_piano', target: 250, total: 40, yours: 0, contributors: 3,
  started_by_you: false, status: 'open', ends_at: NOW + 20 * DAY, created_at: NOW - 10 * DAY, ...over });
// A grand piano on the floor, as room.items has it after standing(): canvas pixels.
const piano = { kind: 'grand_piano', id: 'p1', item: 'grand_piano', left: 200, top: 120, wide: 72, deep: 48 };

// Open: the goal, the bar, the count of people, and no names or amounts put in.
let cards = ctx.fundCardsFor([fund({ id: 'open' })], [piano], NOW);
assert.equal(cards.length, 1);
assert.equal(cards[0].title, 'Grand piano');
same(ctx.fundCardText(cards[0]), { line: '40 of 250 credits · from 3 people', bar: { max: 250, value: 40 } });
assert.equal(cards[0].x, 200 + 72 + 6, 'the card sits to the right of the piano');
assert.equal(cards[0].y, 120);
const openJson = JSON.stringify(cards[0]);
for (const secret of ['yours', 'started_by_you', 'put_in', 'names']) assert.ok(!openJson.includes(secret), secret);

// Open, one person only: still the count, and "1 person".
cards = ctx.fundCardsFor([fund({ contributors: 1, total: 10 })], [piano], NOW);
assert.equal(ctx.fundCardText(cards[0]).line, '10 of 250 credits · from 1 person');

// Open and a card on the right has no room: it goes to the left of the piano.
const edge = { ...piano, left: 400, wide: 72 };
cards = ctx.fundCardsFor([fund()], [edge], NOW);
assert.equal(cards[0].x, 400 - 6 - 132);

// Funded: "Funded", no bar.
cards = ctx.fundCardsFor([fund({ status: 'complete', total: 250, contributors: 5, ends_at: NOW + 5 * DAY })], [piano], NOW);
same(ctx.fundCardText(cards[0]), { line: 'Funded', bar: null });

// Ended: refunded, and cancelled, both say everyone got their credits back.
for (const status of ['refunded', 'cancelled']) {
  cards = ctx.fundCardsFor([fund({ status, ends_at: NOW - 2 * DAY })], [piano], NOW);
  assert.equal(cards.length, 1, status);
  same(ctx.fundCardText(cards[0]), { line: 'Ended: everyone got their credits back', bar: null });
}

// An old fund, ended more than a week ago, shows nothing; so does a room with no fund.
const old = fund({ status: 'refunded', ends_at: NOW - 30 * DAY, created_at: NOW - 60 * DAY });
same(ctx.fundCardsFor([old], [piano], NOW), []);
same(ctx.fundCardsFor([], [piano], NOW), []);
// A pack fund (not a grand piece) has no card here.
same(ctx.fundCardsFor([fund({ item: 'winter' })], [piano], NOW), []);

// The open fund wins over a newer ended one for the same piece.
cards = ctx.fundCardsFor([fund({ status: 'refunded', ends_at: NOW - DAY, created_at: NOW - 2 * DAY, id: 'old' }), fund({ id: 'open' })], [piano], NOW);
assert.equal(cards.length, 1);
assert.equal(cards[0].id, 'open');

// A funded piece not placed on the floor yet: its card goes in the top-left corner.
cards = ctx.fundCardsFor([fund({ item: 'piece:koi_pond', status: 'complete', target: 150, total: 150 })], [piano], NOW);
assert.equal(cards[0].title, 'Koi pond');
assert.equal(cards[0].x, 0);
assert.equal(cards[0].y, 0);

// roomFundCards: one fetch, a card on the overlay with a fixed size class, and the position as a share of the stage.
const overlay = { children: [], append(el) { this.children.push(el); }, };
const room = { overlay };
ctx.h = (tag, attrs, ...kids) => ({ tag, attrs: attrs || {}, kids: kids.flat().filter((k) => k != null && k !== false), style: {}, remove() { overlay.children = overlay.children.filter((c) => c !== this); } });
vm.runInContext('this.h = h;', ctx);
state.funds = [fund({ id: 'open' })];
const cardsUi = ctx.roomFundCards(room, 'room1');
cardsUi.update([piano]);
await new Promise((resolve) => setImmediate(resolve));
assert.equal(calls.filter((c) => c === '/api/floors/room1/funds').length, 1);
assert.equal(overlay.children.length, 1);
const el = overlay.children[0];
assert.equal(el.attrs.class, 'fund-card');
assert.equal(el.style.left, `${((200 + 72 + 6) / 480) * 100}%`);
// A second update in the same 30 seconds doesn't fetch again, and the card is replaced, not doubled.
fakeNowMs += 10000;
cardsUi.update([piano]);
await new Promise((resolve) => setImmediate(resolve));
assert.equal(calls.filter((c) => c === '/api/floors/room1/funds').length, 1);
assert.equal(overlay.children.length, 1);
// After 30 seconds the list is fetched again: the fund has gone, so the room shows nothing.
state.funds = [];
fakeNowMs += 31000;
cardsUi.update([piano]);
await new Promise((resolve) => setImmediate(resolve));
assert.equal(calls.filter((c) => c === '/api/floors/room1/funds').length, 2);
assert.equal(overlay.children.length, 0);

console.log('ok');
})().catch((error) => { console.error(error); process.exit(1); });
