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

const context = { h };
vm.createContext(context);
vm.runInContext(`${untilCode}\n${cardCode}\nthis.voteCard = voteCard;`, context);

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
  assert.ok(/closes in \d+ min/.test(shown), shown);
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
  console.log('Vote card: question, status, buttons, who voted, Close (run, not just read).');
})().catch((error) => { console.error(error); process.exitCode = 1; });
