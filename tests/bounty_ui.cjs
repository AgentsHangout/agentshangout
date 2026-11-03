// HC20-HC21: bounty controls use the server's per-caller actions and never update optimistically.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
const start = source.indexOf('  function bountyCardUi(card) {');
assert.ok(start >= 0, 'production bountyCardUi exists');
const end = source.indexOf('\n  }\n', start) + 4;
const helper = source.slice(start, end);
const escrowStart = source.indexOf('function questionEscrowRows(questions, error, afterCancel) {');
assert.ok(escrowStart >= 0, 'production questionEscrowRows exists');
const escrowEnd = source.indexOf('\n}\n', escrowStart) + 2;
const escrowHelper = source.slice(escrowStart, escrowEnd);
const calls = [];
const errors = [];
let refreshes = 0;
let failPath = null;
const h = (tag, props = {}, ...children) => ({ tag, props, children: children.flat().filter(child => child !== null && child !== undefined && child !== false) });
const context = vm.createContext({
  h,
  field: (label, input) => h('label', {}, label, input),
  count: amount => `${amount} credits`,
  submitter: (_error, run) => values => run(values),
  act: (_error, run) => async () => {
    try { await run(); } catch (error) { errors.push(error.message); }
  },
  api: async (path, options) => {
    calls.push([path, options]);
    if (path === failPath) throw new Error('request failed');
  },
  confirm: () => true,
  boardsError: {},
  bountyCredits: 7,
  bountyCreditsAt: 100,
  loadBoards: async () => { refreshes++; },
});
vm.runInContext(helper, context);
vm.runInContext(escrowHelper, context);
const sameCall = (actual, expected) => assert.equal(JSON.stringify(actual), JSON.stringify(expected));
function all(node) {
  return node && typeof node === 'object'
    ? [node, ...(node.children || []).flatMap(all)]
    : [];
}
function makeBounty(overrides = {}) {
  return {
    amount: 4, status: 'open', sponsor_id: 'sponsor', sponsor_name: 'Sam', can_cancel: false,
    submissions: [{ id: 'submission/a', bot_id: 'bot', bot_name: 'Scout', owner_id: 'owner', owner_name: 'Rae', text: 'A useful answer', can_accept: false }],
    ...overrides,
  };
}

async function run() {
// A caller without the server-granted action flags sees no write controls.
assert.equal(context.bountyCardUi({ id: 'private', can_fund: false, bounty: null }), null);
const readOnly = context.bountyCardUi({ id: 'card', title: 'Question', can_fund: false, bounty: makeBounty() });
assert.equal(all(readOnly).filter(node => node.tag === 'form' || node.tag === 'button').length, 0);

// Funding is one form with positive whole-credit constraints and the caller's own current balance.
const funding = context.bountyCardUi({ id: 'card/alpha', title: 'Question', can_fund: true, bounty: null });
const input = all(funding).find(node => node.tag === 'input');
assert.equal(input.props.min, 1);
assert.equal(input.props.step, 1);
assert.equal(input.props.max, 7);
assert.equal(all(funding).filter(node => node.tag === 'form').length, 1);
await funding.props.onsubmit({ amount: '3' });
sameCall(calls.at(-1), ['/api/cards/card%2Falpha/bounty', { method: 'POST', body: { amount: 3 } }]);
assert.equal(refreshes, 1);

// The API's hard cap remains enforced even when the owner's balance is higher or not loaded yet.
context.bountyCredits = 240;
const wealthyFunding = context.bountyCardUi({ id: 'wealthy', title: 'Question', can_fund: true, bounty: null });
assert.equal(all(wealthyFunding).find(node => node.tag === 'input').props.max, 100);
context.bountyCredits = null;
const unknownBalance = context.bountyCardUi({ id: 'unknown', title: 'Question', can_fund: true, bounty: null });
assert.equal(all(unknownBalance).find(node => node.tag === 'input').props.max, 100);
context.bountyCredits = 7;

// A funded card has one bounty display, no second funding form, and no duplicate controls.
const fundedBounty = makeBounty({ can_cancel: true, submissions: [{ ...makeBounty().submissions[0], can_accept: true }] });
const funded = context.bountyCardUi({ id: 'funded', title: 'Question', can_fund: true, bounty: fundedBounty });
assert.equal(all(funded).filter(node => node.tag === 'form').length, 0);
assert.equal(all(funded).filter(node => node.tag === 'button').length, 2);
const buttons = all(funded).filter(node => node.tag === 'button');
await buttons.find(node => node.children.includes('Accept answer and pay')).props.onclick();
sameCall(calls.at(-1), ['/api/cards/funded/bounty/accept', { method: 'POST', body: { submission_id: 'submission/a' } }]);
await buttons.find(node => node.children.includes('Cancel and refund')).props.onclick();
sameCall(calls.at(-1), ['/api/cards/funded/bounty/cancel', { method: 'POST', body: {} }]);
assert.equal(refreshes, 3);

// Paid state identifies only the answer that received the award.
const paid = context.bountyCardUi({ id: 'paid', title: 'Question', can_fund: false, bounty: makeBounty({
  status: 'paid', accepted_submission_id: 'chosen', submissions: [
    { ...makeBounty().submissions[0], id: 'other' },
    { ...makeBounty().submissions[0], id: 'chosen' },
  ],
}) });
assert.equal(all(paid).filter(node => node.children.includes('Accepted answer · reward paid')).length, 1);

// Failed writes leave the displayed bounty and balance alone and never refresh as if they succeeded.
const protectedState = makeBounty({ can_cancel: true, submissions: [] });
const beforeCredits = context.bountyCredits;
const beforeCache = context.bountyCreditsAt;
failPath = '/api/cards/error/bounty/cancel';
const failing = context.bountyCardUi({ id: 'error', title: 'Question', can_fund: false, bounty: protectedState });
await all(failing).find(node => node.tag === 'button').props.onclick();
assert.equal(protectedState.status, 'open');
assert.equal(context.bountyCredits, beforeCredits);
assert.equal(context.bountyCreditsAt, beforeCache);
assert.equal(refreshes, 3);
assert.equal(JSON.stringify(errors), JSON.stringify(['request failed']));

// The account panel exposes only card ids and amounts, and cancellation works outside the room.
let accountRefreshes = 0;
const escrow = context.questionEscrowRows([{ card_id: 'card/closed-room', amount: 6 }], {}, async () => { accountRefreshes++; });
assert.equal(all(escrow).some(node => JSON.stringify(node).includes('secret card text')), false);
assert.equal(all(escrow).some(node => node.children.includes('6 credits')), true);
const refund = all(escrow).find(node => node.tag === 'button');
await refund.props.onclick();
sameCall(calls.at(-1), ['/api/cards/card%2Fclosed-room/bounty/cancel', { method: 'POST', body: {} }]);
assert.equal(accountRefreshes, 1);
failPath = '/api/cards/card%2Fclosed-room/bounty/cancel';
await refund.props.onclick();
assert.equal(accountRefreshes, 1, 'failed refund does not refresh the account as if it succeeded');
assert.equal(JSON.stringify(errors), JSON.stringify(['request failed', 'request failed']));
assert.equal(context.questionEscrowRows([], {}, async () => {}), null);
console.log('HC20-HC21: bounty permissions, amounts, exact routes, account escrow refunds and non-optimistic errors pass.');
}
run().catch(error => { console.error(error); process.exitCode = 1; });
