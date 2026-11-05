// node tests/notes_station_offer_ui.cjs: I119, a third space without a notes station offers one to whoever runs it.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
const start = app.indexOf('function notesStationOffer(');
assert.ok(start >= 0);
const context = {};
vm.createContext(context);
vm.runInContext(app.slice(start, app.indexOf('\n}\n', start) + 2) + '\nthis.notesStationOffer = notesStationOffer;', context);
const offer = context.notesStationOffer;
const third = { runs_it: true, space: { kind: 'third' } };
const station = [{ kind: 'shared_notes_station' }];
assert.equal(offer(third, [], false), true, 'an older third space with none is offered one');
assert.equal(offer(third, [{ kind: 'plant' }], false), true);
assert.equal(offer(third, station, false), false, 'one that has a station is not');
assert.equal(offer(third, [], true), false, 'Not now hides it');
assert.equal(offer({ ...third, runs_it: false }, [], false), false, 'only whoever runs the space');
assert.equal(offer({ runs_it: true, space: { kind: 'house' } }, [], false), false, 'a house never gets one (I101)');
assert.equal(offer(null, [], false), false);
// Add goes through the normal placing call on a free spot; nothing is placed without pressing it.
assert.ok(app.includes("await api(`/api/floors/${encodeURIComponent(id)}/items`, { method: 'POST', body: { kind: 'shared_notes_station', x, y, facing: 0 } });"));
assert.ok(app.includes("const { x, y } = firstSpot(1, 1);"));
assert.ok(app.includes("'Add a shared notes station'") && app.includes("'Not now'"));
assert.ok(app.includes('[offering ? notesOfferEl() : null, buildCollectionPicker('), 'it sits at the top of the catalog');
console.log('Notes station offer: only for whoever runs an older third space without one, until Add or Not now.');
