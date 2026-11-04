// Run with node tests/repeat_ui.cjs. Exercise the shipped plan Repeat control (RP13).
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const source = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
const start = source.indexOf('const REPEAT_WEEKDAYS');
assert.ok(start >= 0, 'missing repeat control');
const code = source.slice(start, source.indexOf('\n// L50: a day plan', start));
function node(tag, props = {}, ...children) {
  return { tag, ...props, children: children.flat().filter(Boolean),
    replaceChildren(...next) { this.children = next.flat().filter(Boolean); } };
}
function all(tree, predicate, out = []) {
  if (!tree || typeof tree !== 'object') return out;
  if (predicate(tree)) out.push(tree);
  (tree.children || []).forEach((child) => all(child, predicate, out));
  return out;
}
const calls = [];
let reads = {};
const context = {
  h: node,
  formError: () => node('p', { class: 'error', hidden: true }),
  api: async (path, opts = {}) => {
    calls.push({ path, method: opts.method || 'GET', body: opts.body });
    const key = `${opts.method || 'GET'} ${path}`;
    if (!(key in reads)) throw new Error('unexpected ' + key);
    return typeof reads[key] === 'function' ? reads[key](opts.body) : reads[key];
  },
};
vm.createContext(context);
vm.runInContext(`${code}\nthis.repeatBody = repeatBody; this.repeatSummary = repeatSummary; this.repeatPanel = repeatPanel; this.repeatPanelFor = repeatPanelFor;`, context);
const tick = () => new Promise((resolve) => setImmediate(resolve));
const named = (tree, name) => all(tree, (n) => n.name === name)[0];
const json = (value) => JSON.stringify(value);
const week = ['mon', 'tue', 'wed', 'thu', 'fri'];

(async () => {
  // The body RP1 takes.
  assert.equal(json(context.repeatBody({ kind: 'daily', times: '09:00, 13:00' })), json({ freq: 'daily', times: ['09:00', '13:00'] }));
  assert.equal(json(context.repeatBody({ kind: 'weekdays', times: '08:30', until: '2031-01-01' })),
    json({ freq: 'weekly', times: ['08:30'], days: week, until: '2031-01-01' }));
  assert.equal(json(context.repeatBody({ kind: 'monthly', times: '10:00', month_day: '15' })),
    json({ freq: 'monthly', times: ['10:00'], month_day: 15 }));
  assert.equal(json(context.repeatBody({ kind: 'hourly', times: '09:00', interval: '3', from: '09:00', to: '17:00' })),
    json({ freq: 'hourly', interval: 3, window: { from: '09:00', to: '17:00' } }));

  // The shape in words.
  assert.equal(context.repeatSummary(null), 'Does not repeat');
  assert.equal(context.repeatSummary({ freq: 'weekly', days: week, times: ['09:00'] }), 'Weekdays at 09:00');
  assert.equal(context.repeatSummary({ freq: 'daily', days: [...week, 'sat', 'sun'], times: ['09:00', '13:00'], paused: true }), 'Every day at 09:00 and 13:00 (paused)');
  assert.equal(context.repeatSummary({ freq: 'monthly', days: [], times: ['10:00'], month_day: 15, until: '2031-01-01' }), 'On day 15 of each month at 10:00, until 2031-01-01');
  assert.equal(context.repeatSummary({ freq: 'hourly', days: week, times: [], interval: 3, window: { from: '09:00', to: '17:00' } }), 'Every 3 hours, 09:00 to 17:00');

  // The owner: sees the shape, saves a new one, stops it.
  const plan = { id: 'ev1', status: 'proposed', proposed_by: 'ann' };
  const bots = [{ id: 'b1', name: 'Rover' }, { id: 'b2', name: 'Pip' }];
  reads = { 'GET /api/events/ev1/cadence': { cadence: null, your_bots: [] } };
  let panel = context.repeatPanel(plan, 'ann', bots);
  await tick();
  assert.ok(!panel.hidden);
  assert.equal(panel.children[0].children[0], 'Repeat: Does not repeat');
  assert.equal(named(panel, 'stop'), undefined, 'nothing to stop yet');
  assert.equal(all(panel, (n) => n.type === 'checkbox').length, 0, 'the owner has no per-bot box');
  named(panel, 'kind').value = 'weekdays';
  reads['PUT /api/events/ev1/cadence'] = (body) => ({ freq: 'weekly', days: week, times: body.times, paused: false });
  await named(panel, 'save').onclick();
  const put = calls.find((c) => c.method === 'PUT');
  assert.equal(json(put.body), json({ freq: 'weekly', times: ['09:00'], days: week }));
  assert.equal(panel.children[0].children[0], 'Repeat: Weekdays at 09:00');
  reads['DELETE /api/events/ev1/cadence'] = { ok: true };
  await named(panel, 'stop').onclick();
  assert.equal(panel.children[0].children[0], 'Repeat: Does not repeat');

  // A refusal is shown, not thrown.
  reads['PUT /api/events/ev1/cadence'] = () => { throw new Error('until is already past'); };
  await named(panel, 'save').onclick();
  const error = all(panel, (n) => n.class === 'error')[0];
  assert.equal(error.textContent, 'until is already past');
  assert.equal(error.hidden, false);

  // Another member: one box per bot of theirs, nothing if the plan doesn't repeat.
  reads = { 'GET /api/events/ev1/cadence': { cadence: null, your_bots: [] } };
  panel = context.repeatPanel(plan, 'bo', bots);
  await tick();
  assert.ok(!panel.hidden, 'shown, so a repeat started later can be found');
  assert.ok(named(panel, 'not-repeating'), 'says it does not repeat yet');
  assert.equal(all(panel, (n) => n.type === 'checkbox').length, 0);
  reads = { 'GET /api/events/ev1/cadence': { cadence: { freq: 'weekly', days: week, times: ['09:00'] }, your_bots: ['b2'] } };
  panel = context.repeatPanel(plan, 'bo', bots);
  await tick();
  const boxes = all(panel, (n) => n.type === 'checkbox');
  assert.deepEqual(boxes.map((b) => b.checked), [false, true]);
  assert.equal(named(panel, 'save'), undefined, 'only the owner sets the shape');
  reads['PUT /api/events/ev1/cue-bots'] = (body) => ({ bots: body.bots });
  boxes[0].checked = true;
  await boxes[0].onchange();
  assert.equal(json(calls.at(-1).body), json({ bots: ['b1', 'b2'] }));
  boxes[1].checked = false;
  await boxes[1].onchange();
  assert.equal(json(calls.at(-1).body), json({ bots: ['b1'] }));

  // Save then Stop while Save is pending: Stop waits, so an old PUT can't undo it.
  reads = { 'GET /api/events/ev1/cadence': { cadence: { freq: 'weekly', days: week, times: ['09:00'] }, your_bots: [] } };
  panel = context.repeatPanel(plan, 'ann', bots);
  await tick();
  let release;
  reads['PUT /api/events/ev1/cadence'] = () => new Promise((resolve) => { release = () => resolve({ freq: 'weekly', days: week, times: ['09:00'] }); });
  reads['DELETE /api/events/ev1/cadence'] = { ok: true };
  const mark = calls.length;
  const saving = named(panel, 'save').onclick();
  assert.equal(named(panel, 'save').disabled, true);
  assert.equal(named(panel, 'stop').disabled, true);
  await named(panel, 'stop').onclick();
  assert.equal(calls.slice(mark).filter((c) => c.method === 'DELETE').length, 0, 'no DELETE while the PUT is pending');
  release();
  await saving;
  assert.ok(!named(panel, 'stop').disabled, 'buttons come back when it settles');
  // A failure re-enables them too.
  reads['PUT /api/events/ev1/cadence'] = () => { throw new Error('no'); };
  await named(panel, 'save').onclick();
  assert.ok(!named(panel, 'save').disabled);

  // Quick opt-outs: the last write matches the boxes, whatever order the answers come in.
  reads = { 'GET /api/events/ev1/cadence': { cadence: { freq: 'weekly', days: week, times: ['09:00'] }, your_bots: ['b1', 'b2'] } };
  panel = context.repeatPanel(plan, 'bo', bots);
  await tick();
  const pair = all(panel, (n) => n.type === 'checkbox');
  assert.deepEqual(pair.map((b) => b.checked), [true, true]);
  const waiting = [];
  reads['PUT /api/events/ev1/cue-bots'] = (body) => new Promise((resolve) => waiting.push(() => resolve({ bots: body.bots })));
  const before = calls.length;
  pair[0].checked = false;
  const first = pair[0].onchange();
  pair[1].checked = false;
  const second = pair[1].onchange();
  await tick();
  assert.equal(calls.length - before, 1, 'one write in flight at a time');
  waiting.shift()();
  await tick();
  waiting.shift()();
  await Promise.all([first, second]);
  const puts = calls.slice(before);
  assert.equal(puts.length, 2);
  assert.equal(json(puts.at(-1).body), json({ bots: [] }), 'the last write matches the boxes');

  // Save keeps a paused repeat paused unless Resume is ticked.
  const pausedRead = { freq: 'daily', days: [...week, 'sat', 'sun'], times: ['09:00'], paused: true };
  reads = { 'GET /api/events/ev1/cadence': { cadence: pausedRead, your_bots: [] } };
  panel = context.repeatPanel(plan, 'ann', bots);
  await tick();
  reads['PUT /api/events/ev1/cadence'] = (body) => ({ ...pausedRead, paused: Boolean(body.paused) });
  await named(panel, 'save').onclick();
  assert.equal(calls.at(-1).body.paused, true);
  assert.ok(panel.children[0].children[0].includes('(paused)'));
  named(panel, 'resume').checked = true;
  await named(panel, 'save').onclick();
  assert.equal(calls.at(-1).body.paused, undefined, 'Resume sends not paused');
  assert.equal(context.repeatBody({ kind: 'daily', times: '09:00' }).paused, undefined);

  // A repeat the form can't say exactly is not rewritten: no Save, only Stop.
  const odd = [
    { freq: 'weekly', days: ['mon', 'wed'], times: ['09:00'] },
    { freq: 'daily', days: week, times: ['09:00'] },
    { freq: 'hourly', days: week, times: [], interval: 3, window: { from: '09:00', to: '17:00' } },
  ];
  for (const cadence of odd) {
    reads = { 'GET /api/events/ev1/cadence': { cadence, your_bots: [] } };
    panel = context.repeatPanel(plan, 'ann', bots);
    await tick();
    assert.equal(named(panel, 'save'), undefined, `no Save for ${cadence.freq} on ${cadence.days}`);
    assert.ok(named(panel, 'locked'));
    assert.ok(named(panel, 'stop'));
  }
  const fine = [
    { freq: 'weekly', days: week, times: ['09:00'] },
    { freq: 'daily', days: [...week, 'sat', 'sun'], times: ['09:00'] },
    { freq: 'monthly', days: [], times: ['09:00'], month_day: 3 },
    { freq: 'hourly', days: [...week, 'sat', 'sun'], times: [], interval: 3, window: { from: '09:00', to: '17:00' } },
  ];
  for (const cadence of fine) {
    reads = { 'GET /api/events/ev1/cadence': { cadence, your_bots: [] } };
    panel = context.repeatPanel(plan, 'ann', bots);
    await tick();
    assert.ok(named(panel, 'save'), `Save for ${cadence.freq}`);
  }

  // A room reload while the owner is typing keeps the same panel, open, with what they typed, and
  // reads nothing again; opening it later reads it fresh.
  reads = { 'GET /api/events/ev1/cadence': { cadence: { freq: 'weekly', days: week, times: ['09:00'] }, your_bots: [] } };
  const cache = new Map();
  panel = context.repeatPanelFor(cache, plan, 'ann', bots);
  await tick();
  panel.open = true;
  named(panel, 'times').value = '08:15, 12:30';
  named(panel, 'times').oninput();
  const readsBefore = calls.filter((c) => c.method === 'GET').length;
  for (let reload = 0; reload < 3; reload += 1) {
    assert.equal(context.repeatPanelFor(cache, { ...plan }, 'ann', bots), panel, 'the same panel after a reload');
  }
  await tick();
  assert.equal(panel.open, true);
  assert.equal(named(panel, 'times').value, '08:15, 12:30', 'typed times survive the reload');
  assert.equal(calls.filter((c) => c.method === 'GET').length, readsBefore, 'no read per reload');
  panel.open = false;
  panel.open = true;
  await panel.ontoggle();
  await tick();
  assert.equal(calls.filter((c) => c.method === 'GET').length, readsBefore + 1, 'opening reads it fresh');
  assert.equal(named(panel, 'times').value, '08:15, 12:30', 'a read never wipes typed times');
  // A canceled plan, or a different set of your bots, gets what fits it now.
  assert.equal(context.repeatPanelFor(cache, { ...plan, status: 'cancelled' }, 'ann', bots), null);
  const fewer = context.repeatPanelFor(cache, plan, 'ann', bots.slice(0, 1));
  assert.notEqual(fewer, panel);

  // The yes box says what a yes does: the plan may wake the bot, which keeps its own rules.
  reads = { 'GET /api/events/ev1/cadence': { cadence: { freq: 'weekly', days: week, times: ['09:00'] }, your_bots: [] } };
  panel = context.repeatPanel(plan, 'bo', bots);
  await tick();
  const words = JSON.stringify(panel);
  assert.ok(words.includes('Let this plan wake Rover'), words);
  assert.ok(words.includes('Work hours and only-me'));
  assert.ok(!words.includes('come'), 'no wording that the bot is brought in');

  // A slow read can't undo a newer yes: b1 is turned off while the read is out, and the read's old
  // answer is dropped, so turning off b2 then sends [] and not [b1].
  {
    const both = { cadence: { freq: 'weekly', days: week, times: ['09:00'] }, your_bots: ['b1', 'b2'] };
    reads = { 'GET /api/events/ev1/cadence': both, 'PUT /api/events/ev1/cue-bots': (body) => ({ bots: body.bots }) };
    const member = context.repeatPanel(plan, 'bo', bots);
    await tick();
    let answer;
    reads['GET /api/events/ev1/cadence'] = () => new Promise((resolve) => { answer = () => resolve(both); });
    member.open = true;
    const reading = member.ontoggle();
    const [b1, b2] = all(member, (n) => n.type === 'checkbox');
    b1.checked = false;
    await b1.onchange();
    answer();
    await reading;
    await tick();
    const [again1, again2] = all(member, (n) => n.type === 'checkbox');
    assert.equal(again1.checked, false, 'the stale read did not tick b1 again');
    again2.checked = false;
    await again2.onchange();
    assert.equal(json(calls.at(-1).body), json({ bots: [] }), 'consent b1 withdrew stays withdrawn');
    assert.equal(b2, again2);
  }

  // Nor can a read during a pending Save reset the owner's lock: Stop stays off until Save settles.
  {
    const weekly = { cadence: { freq: 'weekly', days: week, times: ['09:00'] }, your_bots: [] };
    reads = { 'GET /api/events/ev1/cadence': weekly, 'DELETE /api/events/ev1/cadence': { ok: true } };
    const own = context.repeatPanel(plan, 'ann', bots);
    await tick();
    let release;
    reads['PUT /api/events/ev1/cadence'] = () => new Promise((resolve) => { release = () => resolve(weekly.cadence); });
    const saving = named(own, 'save').onclick();
    own.open = true;
    await own.ontoggle();
    await tick();
    const mark = calls.length;
    await named(own, 'stop').onclick();
    assert.equal(calls.slice(mark).filter((c) => c.method === 'DELETE').length, 0, 'no DELETE behind a pending Save');
    release();
    await saving;
  }

  // A member who first saw no repeat finds one its owner started later, by opening the panel.
  {
    reads = { 'GET /api/events/ev1/cadence': { cadence: null, your_bots: [] } };
    const cacheLater = new Map();
    const member = context.repeatPanelFor(cacheLater, plan, 'bo', bots);
    await tick();
    assert.ok(named(member, 'not-repeating'));
    reads['GET /api/events/ev1/cadence'] = { cadence: { freq: 'weekly', days: week, times: ['09:00'] }, your_bots: [] };
    assert.equal(context.repeatPanelFor(cacheLater, plan, 'bo', bots), member, 'a reload keeps it');
    member.open = true;
    await member.ontoggle();
    await tick();
    assert.equal(all(member, (n) => n.type === 'checkbox').length, 2, 'opening shows the yes boxes');
  }

  // No bots here, or a canceled plan: nothing at all.
  assert.equal(context.repeatPanel(plan, 'bo', []), null);
  assert.equal(context.repeatPanel({ ...plan, status: 'cancelled' }, 'ann', bots), null);
  console.log('Plan Repeat control: owner sets, shows and stops it; other members say yes per bot.');
})().catch((failure) => { console.error(failure); process.exit(1); });
