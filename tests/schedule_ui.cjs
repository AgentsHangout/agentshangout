// Run with node tests/schedule_ui.cjs. Exercise the shipped day plan painter (L50, L51).
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const source = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
const start = source.indexOf('  function schedulePainter(');
assert.ok(start >= 0, 'missing schedulePainter');
const painterSource = source.slice(start, source.indexOf('\n  }\n', start) + 4);
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
all(painter.element, (n) => n.tag === 'button' && n.children[0] === 'Use the hours above instead')[0].onclick();
assert.equal(painter.cleared(), true);
assert.equal(painter.changed(), false);
console.log('Day plan painter: read-off strip, brush and drag, presets, keyboard and clearing verified.');
