// node tests/vote_card_ui.cjs: V12, a vote as a card: its question and status, a button per option
// for someone who may vote with their choice marked, who chose what (visitors marked), and Close
// for whoever opened it. Runs the shipped voteCard against stubs, not just its text.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
const slice = (from, end) => {
  const start = app.indexOf(from);
  assert.ok(start >= 0, from);
  return app.slice(start, app.indexOf(end, start) + end.length);
};
const clockCode = slice('function voteClock(closesAt, {', '\n}\n');
const cardCode = slice('function voteCard(vote,', '\n}\n');
const untilCode = slice('function untilText(unix) {', '\n}\n');

class Node {
  constructor(tag, props = {}, children = []) {
    this.tag = tag; this.props = props; this.children = [];
    Object.assign(this, props); this.append(...children);
  }
  append(...c) { this.children.push(...c.flat(Infinity).filter((x) => x != null && x !== false)); }
}
const h = (tag, props, ...children) => new Node(tag, props || {}, children);
const all = (n) => (n instanceof Node ? [n, ...n.children.flatMap(all)] : []);
const text = (n) => (n instanceof Node ? (n.textContent || '') + n.children.map(text).join('') : String(n));

// #814: the countdown runs on a fake timer, so a test can tick it without waiting.
const ticks = [];
const cleared = [];
const context = {
  h,
  whenText: () => 'Sat, 4:00 PM',
  setInterval: (fn) => { ticks.push(fn); return ticks.length; },
  clearInterval: (id) => { cleared.push(id); },
};
vm.createContext(context);
vm.runInContext(`${untilCode}\n${clockCode}\n${cardCode}\nthis.voteCard = voteCard;`, context);

const soon = new Date(Date.now() + 20 * 60 * 1000).toISOString();
const vote = {
  id: 'v1', question: 'Lunch?', opened_by: 'Ann', open: true, closes_at: soon, voted: 3,
  you_voted: 'Pizza', you_opened: true, note: "A room poll, not an owner's approval.",
  options: [
    { option: 'Tacos', votes: 2, voters: [{ bot: 'Scout', owner: 'Ann' }, { person: 'Eve', visitor: true }] },
    { option: 'Pizza', votes: 1, voters: [{ person: 'Ann' }] },
  ],
};

(async () => {
  const cast = [];
  let closed = 0;
  const card = context.voteCard(vote, { cast: async (option) => { cast.push(option); }, close: async () => { closed += 1; } });
  const shown = text(card);
  assert.ok(shown.includes('Lunch?') && shown.includes("Ann's vote") && shown.includes('3 voted'), shown);
  assert.ok(/closes in \d+:\d\d/.test(shown), shown);
  const timerEl = all(card).find((n) => n.props.role === 'timer');
  assert.ok(timerEl && timerEl.props['aria-label'] === 'Closes Sat, 4:00 PM', 'compact live timer with a one-time label');
  assert.ok(!all(card).some((n) => n.props['aria-live'] && n.props['aria-live'] !== 'off'), 'no live region spams the screen reader');
  assert.ok(shown.includes("Scout (Ann's bot)") && shown.includes('Eve, visiting'), 'who chose what, visitors marked');
  const buttons = all(card).filter((n) => n.tag === 'button');
  const tacos = buttons.find((b) => text(b) === 'Tacos');
  const pizza = buttons.find((b) => text(b) === 'Pizza');
  assert.equal(pizza.props.class, 'chosen', 'their choice is marked');
  assert.equal(tacos.props.class, 'ghost');
  await tacos.props.onclick();
  assert.equal(JSON.stringify(cast), '["Tacos"]');
  await buttons.find((b) => text(b) === 'Close it now').props.onclick();
  assert.equal(closed, 1);

  // The countdown ticks once a second; a past closes_at reads closed, and its timer stops.
  assert.ok(ticks.length >= 1, 'a one-second tick is set');
  const clockNow = all(card).find((n) => n.props.role === 'timer');
  ticks[0]();
  assert.ok(/^closes in/.test(clockNow.textContent), clockNow.textContent);
  const past = new Date(Date.now() - 5000).toISOString();
  const expired = context.voteCard({ ...vote, closes_at: past }, { cast: async () => {}, close: async () => {} });
  const expiredClock = all(expired).find((n) => n.props.role === 'timer');
  assert.equal(text(expiredClock), 'closed', 'a past closes_at shows closed');
  const before = cleared.length;
  ticks[ticks.length - 1]();
  assert.equal(cleared.length, before + 1, 'the timer stops once closed');

  // At zero, a card that was open asks the page to re-read the votes, once; a card that starts past zero doesn't.
  const realDate = vm.runInContext('Date', context);
  const fake = { now: Date.now() };
  context.Date = { parse: Date.parse, now: () => fake.now };
  let expiredCalls = 0;
  context.voteCard({ ...vote, closes_at: new Date(fake.now + 2000).toISOString() }, { onExpire: () => { expiredCalls += 1; } });
  const liveTick = ticks[ticks.length - 1];
  fake.now += 1000;
  liveTick();
  assert.equal(expiredCalls, 0, 'no re-read while time is left');
  fake.now += 2000;
  liveTick();
  assert.equal(expiredCalls, 1, 'at zero the page re-reads the votes');
  liveTick();
  assert.equal(expiredCalls, 1, 'only once');
  context.voteCard({ ...vote, closes_at: new Date(fake.now - 1000).toISOString() }, { onExpire: () => { expiredCalls += 1; } });
  ticks[ticks.length - 1]();
  assert.equal(expiredCalls, 1, 'a card that starts past zero does not loop the re-read');
  context.Date = realDate;

  // A refusal shows on the card.
  const refused = context.voteCard(vote, { cast: async () => { throw new Error('guests can\'t vote'); } });
  const status = all(refused).find((n) => n.props.role === 'status');
  await all(refused).find((n) => n.tag === 'button' && text(n) === 'Tacos').props.onclick();
  assert.equal(status.textContent, 'guests can\'t vote');

  // Closed, or for a bot's page: counts only, no buttons; not the opener: no Close.
  const done = context.voteCard({ ...vote, open: false }, { cast: async () => {}, close: async () => {} });
  assert.equal(all(done).filter((n) => n.tag === 'button').length, 0);
  assert.ok(text(done).includes('closed'));
  const notMine = context.voteCard({ ...vote, you_opened: false }, { cast: async () => {}, close: async () => {} });
  assert.ok(!all(notMine).some((n) => n.tag === 'button' && text(n) === 'Close it now'));
  // V13: a walk-in's card has counts and says only members see who voted.
  const hidden = context.voteCard({ ...vote, voters_hidden: true, options: vote.options.map((o) => ({ ...o, voters: [] })) }, {});
  assert.ok(text(hidden).includes("Only the room's members see who voted."), text(hidden));
  assert.ok(!text(hidden).includes('Scout'));
  const readOnly = context.voteCard(vote, {});
  assert.equal(all(readOnly).filter((n) => n.tag === 'button').length, 0);

  // V12 (#777): above chat, an open vote defaults to a compact expandable banner so long options
  // and voter lists never crowd out the messages or composer.
  const longOption = 'Keep the compact poll summary banner above chat and scroll long ballots inside';
  const manyVoters = Array.from({ length: 18 }, (_, i) => ({ bot: `Bot ${i + 1}`, owner: `Owner ${i + 1}` }));
  const longVote = {
    ...vote,
    id: 'v-long',
    question: 'How should a long room poll with many options and voters sit above the chat on a small phone screen?',
    options: [
      { option: longOption, votes: manyVoters.length, voters: manyVoters },
      { option: 'Pizza', votes: 1, voters: [{ person: 'Ann' }] },
    ],
  };
  const toggled = [];
  const banner = context.voteCard(longVote, {
    cast: async (option) => { cast.push(option); },
    close: async () => { closed += 1; },
    collapsible: true,
    onToggle: (openState) => { toggled.push(openState); },
  });
  assert.ok(banner.props.class.includes('vote-banner') && banner.props.class.includes('collapsed'), banner.props.class);
  const body = all(banner).find((n) => (n.props.class || '').includes('vote-banner-body'));
  assert.ok(body && body.hidden === true, 'poll details start collapsed');
  const toggle = all(banner).find((n) => n.tag === 'button' && (n.props.class || '').includes('vote-toggle'));
  assert.ok(toggle, 'explicit expand control');
  assert.equal(text(toggle), 'Show poll');
  assert.equal(toggle.props['aria-expanded'], 'false');

  toggle.props.onclick();
  assert.deepEqual(toggled, [true]);
  assert.equal(body.hidden, false, 'expanding shows the options and voter list');
  assert.ok(banner.props.class.includes('expanded') && !banner.props.class.includes('collapsed'), banner.props.class);
  assert.equal(text(toggle), 'Hide poll');
  assert.equal(toggle.props['aria-expanded'], 'true');
  assert.ok(text(body).includes(longOption) && text(body).includes("Bot 18 (Owner 18's bot)"), 'long options and voter list are inside the expandable body');
  const longBtn = all(body).find((n) => n.tag === 'button' && text(n) === longOption);
  await longBtn.props.onclick();
  assert.equal(cast.at(-1), longOption, 'voting works from the expanded banner');
  await all(body).find((n) => n.tag === 'button' && text(n) === 'Close it now').props.onclick();
  assert.equal(closed, 2, 'closing works from the expanded banner');

  toggle.props.onclick();
  assert.deepEqual(toggled, [true, false]);
  assert.equal(body.hidden, true, 'collapsing hides the ballot again');
  assert.equal(text(toggle), 'Show poll');
  assert.equal(toggle.props['aria-expanded'], 'false');

  const css = fs.readFileSync('web/style.css', 'utf8');
  assert.match(css, /\.vote-banner-body\s*\{[^}]*max-height:/, 'expanded poll body has bounded height');
  assert.match(css, /\.vote-banner-body\s*\{[^}]*overflow-y:\s*auto/, 'expanded poll body scrolls independently');
  console.log('Vote card: question, status, buttons, who voted, Close, and compact expandable banner (#777).');
})().catch((error) => { console.error(error); process.exitCode = 1; });
