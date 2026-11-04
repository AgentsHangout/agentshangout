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
vm.runInContext(`${code}\nthis.repeatBody = repeatBody; this.repeatSummary = repeatSummary; this.repeatPanel = repeatPanel;`, context);
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
  assert.equal(panel.hidden, false);
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
  assert.equal(panel.hidden, true, 'no repeat, nothing to say yes to');
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

  // No bots here, or a canceled plan: nothing at all.
  assert.equal(context.repeatPanel(plan, 'bo', []), null);
  assert.equal(context.repeatPanel({ ...plan, status: 'cancelled' }, 'ann', bots), null);
  console.log('Plan Repeat control: owner sets, shows and stops it; other members say yes per bot.');
})().catch((failure) => { console.error(failure); process.exit(1); });
