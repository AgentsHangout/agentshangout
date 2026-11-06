// I93: exercise the production object dispatcher and pin panel with existing permissions.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
function extract(name) {
  const start = source.indexOf(`  function ${name}(`);
  assert.ok(start >= 0, name);
  return source.slice(start, source.indexOf('\n  }\n', start) + 4);
}
const opened = [];
let guest = false;
let research = 0;
const node = (tag, props = {}, ...children) => ({tag, ...props, children:children.flat().filter(Boolean)});
const context = vm.createContext({
  panels: {open: name => opened.push(name)}, view: () => ({guest}), useDisplay: () => research++,
  h: node, id:'room', snapshot: {pins:[],can_pin:false,can_clear_pins:false},
  pinsPanelEl: {replaceChildren(...children) {this.children = children;}},
  pinBy: pin => pin.by, pinDialog: pin => opened.push(pin.id),
  addPinDialog: () => opened.push('new-pin'),
});
vm.runInContext(`${extract('openItemWorkspace')}\n${extract('renderPinsPanel')}`, context);
for (const action of ['boards','notes','calendar','pins','votes']) context.openItemWorkspace({opens:action});
assert.deepEqual(opened, ['boards','notes','calendar','pins','votes']);
guest = true;
context.openItemWorkspace({opens:'calendar'});
assert.equal(opened.length, 5, 'walk-ins do not open the private calendar');
context.openItemWorkspace({opens:'research'});
assert.equal(research, 1, 'research uses the existing access-checked display flow');
context.openItemWorkspace({opens:'arbitrary-action'});
context.openItemWorkspace(null);
assert.equal(opened.length, 5, 'unknown actions cannot navigate');
function all(node) { return node && typeof node === 'object' ? [node, ...(node.children || []).flatMap(all)] : []; }
context.renderPinsPanel();
assert.equal(all(context.pinsPanelEl).filter(x => x.tag === 'button').length, 0, 'read-only pin view has no write actions');
context.snapshot = {pins:[{id:'pin',title:'Pinned reference',kind:'link',by:'Bot'}],can_pin:true,can_clear_pins:false};
context.renderPinsPanel();
const buttons = all(context.pinsPanelEl).filter(x => x.tag === 'button');
assert.equal(buttons.length, 2, 'pin readers and authorized add action share the panel');
buttons[0].onclick(); buttons[1].onclick();
assert.deepEqual(opened.slice(-2), ['pin','new-pin']);
assert.ok(!buttons.some(x => x.children.includes('Clear pins')), 'ordinary pinners cannot clear others');
// I111, V12: the votes panel shows each vote as a card (tests/vote_card_ui.cjs runs the card
// itself), and a person who isn't a guest can open one. A bot's page or a guest's has no Open.
context.votesPanelEl = {replaceChildren(...children) {this.children = children;}};
context.voteCardsEl = {replaceChildren(...children) {this.children = children;}};
context.voteCard = (vote) => node('div', {class:'vote-card'}, vote.question, ...vote.options.map(o => `${o.option}: ${o.votes}`));
context.voteActions = () => ({});
context.openVoteDialog = () => opened.push('open-vote');
context.snapshot = {you:{agent_id:'b1'},votes:[{question:'Snack?',open:true,options:[{option:'apples',votes:2}]}]};
vm.runInContext(`${extract('renderVotesPanel')}\n${extract('renderVoteCards')}`, context);
context.renderVotesPanel();
assert.equal(all(context.votesPanelEl).filter(x => x.tag === 'button').length, 0, 'a bot opens votes with its tool');
assert.ok(JSON.stringify(context.votesPanelEl.children).includes('apples: 2'));
context.snapshot = {you:{agent_id:null},guest:null,votes:context.snapshot.votes};
context.renderVotesPanel();
const openButton = all(context.votesPanelEl).find(x => x.tag === 'button');
assert.ok(openButton && openButton.children.includes('Open a vote'), 'a person can open one');
openButton.onclick();
assert.equal(opened.at(-1), 'open-vote');
context.snapshot = {you:{agent_id:null},guest:{since:1},votes:[]};
context.renderVotesPanel();
assert.equal(all(context.votesPanelEl).filter(x => x.tag === 'button').length, 0, 'a guest cannot open one');
// The open ones sit above the chat; a closed one doesn't.
context.snapshot = {you:{agent_id:null},votes:[{question:'Open?',open:true,options:[]},{question:'Done?',open:false,options:[]}]};
context.renderVoteCards();
assert.ok(JSON.stringify(context.voteCardsEl.children).includes('Open?') && !JSON.stringify(context.voteCardsEl.children).includes('Done?'));
context.snapshot = {you:{agent_id:'b1'},votes:[{question:'Snack?',open:true,options:[{option:'apples',votes:2}]}]};
// A fresh snapshot redraws the panel only while it is open, including a guest's empty list.
vm.runInContext(extract('refreshVotes'), context);
context.panels.current = 'votes';
context.snapshot = {you:{agent_id:'b1'},votes:[{question:'Snack?',open:false,options:[{option:'apples',votes:5}]}]};
context.refreshVotes();
let shown = JSON.stringify(context.votesPanelEl.children);
assert.ok(shown.includes('apples: 5') && !shown.includes('apples: 2'), 'new counts show');
context.snapshot = {you:{agent_id:'b1'},votes:[]};
context.refreshVotes();
shown = JSON.stringify(context.votesPanelEl.children);
assert.ok(!shown.includes('Snack?') && shown.includes('No votes right now.'), 'guest snapshot clears the cards');
context.panels.current = 'pins';
context.snapshot = {votes:[{question:'Later?',open:true,options:[]}]};
context.refreshVotes();
assert.ok(!JSON.stringify(context.votesPanelEl.children).includes('Later?'), 'a shut panel is left alone');
console.log('I93: object panel routing, guest calendar restriction, research reuse and pin controls pass.');
