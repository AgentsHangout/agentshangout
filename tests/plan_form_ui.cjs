// node tests/plan_form_ui.cjs: F56, proposing a plan says it's held here, asks for an off-site place
// only if needed, and ends on inviting people with the plan's link.
const fs = require('node:fs'), assert = require('node:assert/strict');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
const start = app.indexOf('  function proposeHangout() {');
assert.ok(start >= 0);
const form = app.slice(start, app.indexOf('\n  }\n', start));
// Where: held here by default, with one optional line for somewhere else.
assert.ok(form.includes("h('span', { class: 'muted' }, `Here, in ${snapshot.hangout.name}. `)"), 'it says the plan is held here');
assert.ok(form.includes("'Somewhere else?'"), 'with an optional Somewhere else');
assert.ok(form.includes("placeholder: 'An address or a link', hidden: true"), 'which starts hidden');
assert.ok(form.includes("location: values.location || '',"), 'empty means here');
assert.ok(!form.includes("field('Where', h('input', { name: 'location', maxLength: 200 }))"), 'no bare text box any more');
// Then the invite step, for a person, with the plan the server just made.
assert.ok(form.includes('const created = await api(`/api/hangouts/${id}/events`'));
assert.ok(form.includes("if (created?.id && !snapshot.you.agent_id) sharePlan(created, { fresh: true })"));
const share = app.slice(app.indexOf('async function sharePlan('), app.indexOf('\n}\n', app.indexOf('async function sharePlan(')));
assert.ok(share.includes("modal(fresh ? 'Invite people' : 'Invite a friend',"));
assert.ok(share.includes("'Your plan is up, and everyone in this room can see it and answer.'"));
console.log('Plan form: held here, optional Somewhere else, then Invite people with the link.');
