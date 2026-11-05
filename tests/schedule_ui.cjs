// Run with node tests/schedule_ui.cjs. Exercise the shipped day plan painter (L50, L51).
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const source = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
const start = source.indexOf('function schedulePainter(');
assert.ok(start >= 0, 'missing schedulePainter');
const painterSource = source.slice(start, source.indexOf('\n}\n', start) + 3);
function node(tag, props = {}, ...children) {
  return { tag, ...props, children: children.flat().filter(Boolean), attrs: {},
    events: {}, addEventListener(kind, handler) { this.events[kind] = handler; },
    setAttribute(name, value) { this.attrs[name] = value; },
    replaceChildren(...next) { this.children = next.flat().filter(Boolean); } };
}
function all(tree, predicate, out = []) {
  if (!tree || typeof tree !== 'object') return out;
  if (predicate(tree)) out.push(tree);
  (tree.children || []).forEach((child) => all(child, predicate, out));
  return out;
}
const says = (tree, words) => all(tree, (n) => n.children?.some((t) => typeof t === 'string' && t.includes(words))).length > 0;
const context = { h: node };
vm.createContext(context);
vm.runInContext(`${painterSource}\nthis.painter = schedulePainter;`, context);

// Not saved yet: the strip shows the routine read as blocks, and nothing is sent.
const read = { schedule_saved: false, schedule: { every_day: Array(24).fill('social'), days: {} }, workplace: null, public_places: true };
let painter = context.painter(read);
const hours = all(painter.element, (n) => n.class?.startsWith('sched-hour'));
assert.equal(hours.length, 24);
assert.equal(hours[9].attrs['aria-label'], '09:00 Social');
assert.equal(painter.changed(), false, 'opening the dialog sends nothing');
assert.ok(says(painter.element, 'Not saved yet'));

// Pick Work and drag across 9 to 11.
const brush = (name) => all(painter.element, (n) => n.class?.startsWith('sched-brush') && n.children[0] === name)[0];
brush('Work').events.click();
assert.equal(brush('Work').attrs['aria-pressed'], 'true');
hours[9].events.mousedown({ preventDefault() {} });
hours[10].events.mouseenter();
hours[11].events.mouseenter();
assert.equal(painter.value().slice(8, 13).join(), 'social,work,work,work,social');
assert.equal(hours[10].attrs['aria-label'], '10:00 Work');
assert.equal(painter.changed(), true);

// A preset fills the whole day.
all(painter.element, (n) => n.tag === 'button' && n.children[0] === 'Workday 9 to 5')[0].onclick();
const workday = painter.value();
assert.equal(workday[3], 'home');
assert.equal(workday[9], 'work');
assert.equal(workday[12], 'social');
assert.equal(workday[18], 'social');
assert.equal(workday[20], 'home');

// A keyboard click paints too.
brush('Home').events.click();
hours[9].events.click({ detail: 0 });
assert.equal(painter.value()[9], 'home');

// Saved: a way back to the hours above, which clears it, and weekday plans are mentioned.
const saved = { schedule_saved: true, schedule: { every_day: Array(24).fill('work'), days: { sat: Array(24).fill('anything') } }, workplace: null, public_places: false };
painter = context.painter(saved);
assert.ok(says(painter.element, 'Its own plan for sat stays as it is.'));
assert.ok(says(painter.element, 'With no workplace, a Work hour leaves it where it is.'));
all(painter.element, (n) => n.tag === 'button' && n.children[0] === 'Go back to its old social hours')[0].onclick();
assert.equal(painter.cleared(), true);
assert.equal(painter.changed(), false);
assert.deepEqual(painter.days(), { sat: Array(24).fill('anything') }, 'a saved weekday plan is kept');

// Not saved, and its hours differ by weekday: the read-off rows are shown, not sent, so what's
// painted counts on every day instead of being covered by seven read-off weekdays.
const weekdayRow = Array(24).fill('anything').fill('social', 9, 17);
const weekdayRead = { schedule_saved: false, schedule: { every_day: Array(24).fill('anything'), days: { mon: weekdayRow, tue: weekdayRow, wed: weekdayRow, thu: weekdayRow, fri: weekdayRow, sat: Array(24).fill('anything'), sun: Array(24).fill('anything') } }, workplace: null, public_places: true };
painter = context.painter(weekdayRead);
assert.equal(painter.value()[10], 'social', "the strip starts from a weekday's hours");
assert.ok(!says(painter.element, 'stays as it is'), 'no saved weekday plan to keep');
assert.equal(JSON.stringify(painter.days()), '{}', 'read-off weekdays are not saved');

// L51, L63: a bot that never had social hours is free all day, and says so, not "old social hours".
const fresh = { schedule_saved: false, schedule: { every_day: Array(24).fill('anything'), days: {} }, workplace: null, public_places: false };
painter = context.painter(fresh);
assert.ok(says(painter.element, 'Not saved yet: free all day.'));
assert.ok(!says(painter.element, 'old social hours'));
assert.ok(!says(painter.element, 'With public places off'), 'free hours need no warning');
// The free preset is called what it does.
all(painter.element, (n) => n.tag === 'button' && n.children[0] === 'Social all day')[0].onclick();
assert.ok(says(painter.element, 'With public places off, Work and Social hours leave it where it is.'), 'painting updates the hints');
// L67: ticking public places in the dialog takes the warning away at once.
painter.setContext({ public_places: true });
assert.ok(!says(painter.element, 'With public places off'));
all(painter.element, (n) => n.tag === 'button' && n.children[0] === 'Always working')[0].onclick();
assert.ok(says(painter.element, 'With no workplace, a Work hour leaves it where it is.'));
painter.setContext({ workplace: 'room-1' });
assert.ok(!says(painter.element, 'With no workplace'), 'picking a workplace takes it away');
all(painter.element, (n) => n.tag === 'button' && n.children[0] === 'Free all day')[0].onclick();
assert.ok(painter.value().every((block) => block === 'anything'));
console.log('Day plan painter: read-off strip, brush and drag, presets, keyboard, clearing and live hints verified.');

// L67 (#527): one helper says how long a bot is out and why, for Crew and the schedule alike.
const outStart = source.indexOf('function outText(');
assert.ok(outStart >= 0, 'missing outText');
const outSource = source.slice(outStart, source.indexOf('\n}\n', outStart) + 3);
const clock = { dayFormat: new Intl.DateTimeFormat('en-US', { weekday: 'short', month: 'short', day: 'numeric' }),
  timeFormat: new Intl.DateTimeFormat('en-US', { hour: 'numeric', minute: '2-digit' }) };
clock.fromUnix = (unix) => new Date(unix * 1000);
clock.whenText = (unix) => `${clock.dayFormat.format(clock.fromUnix(unix))}, ${clock.timeFormat.format(clock.fromUnix(unix))}`;
vm.createContext(clock);
vm.runInContext(`${outSource}\nthis.outText = outText;`, clock);
const soon = Math.floor(Date.now() / 1000) + 60;
const at = clock.timeFormat.format(clock.fromUnix(soon));
const sameDay = clock.dayFormat.format(new Date()) === clock.dayFormat.format(clock.fromUnix(soon));
const shown = sameDay ? at : clock.whenText(soon);
assert.equal(clock.outText(soon, 'stay_limit'), `until ${shown} (stay limit)`);
assert.equal(clock.outText(soon, 'schedule'), `until ${shown} (schedule)`);
assert.equal(clock.outText(soon, 'half_hour'), `until ${shown} (wandering)`);
assert.equal(clock.outText(soon + 9 * 86400, 'none'), "until it's quiet (no limit)");
assert.equal(clock.outText(null, 'schedule'), '');
// Crew and the schedule dialog both use it.
assert.ok(source.includes('out ${outText(agent.visitor_until, agent.visitor_until_why)}'));
assert.ok(source.includes('${outText(routine.out_until, routine.out_why)}'));
assert.ok(source.includes("const STATE_TEXT = { active: 'At the table', idle: 'Online', offline: 'Napping' };"));
assert.ok(!source.includes("'Wandering"), 'wandering only means going from place to place');
console.log('outText: one reason, in Crew and the schedule.');

// G18: Bring a bot asks about public places and how long it can stay, off and Forever to start.
const bring = source.slice(source.indexOf('async function bringBot('), source.indexOf('function proposeHangout('));
assert.ok(bring.includes("h('input', { type: 'checkbox', name: 'public_places' }), 'Let it go to public places'"));
assert.ok(bring.includes('STRANGERS_TEXT') && bring.includes('stayField(null)'));
assert.ok(bring.includes('`/api/agents/${result.agent.id}/stay_limit`') && bring.includes('`/api/agents/${result.agent.id}/routine`'));
assert.ok(source.includes("[['', 'Forever'], ['1', '1 hour'], ['4', '4 hours'], ['12', '12 hours']]"));
console.log('Bring a bot: public places off and Forever to start.');
