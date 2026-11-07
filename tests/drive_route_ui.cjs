// Run with node tests/drive_route_ui.cjs. Runs the shipped routePoint from web/world.js (CR10): where a
// car is a fraction of the way along the server's route, and which way it's heading there.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const world = fs.readFileSync('web/world.js', 'utf8').replace(/\r\n/g, '\n');

const from = world.indexOf('function routePoint(');
assert.ok(from >= 0, 'missing routePoint');
const to = world.indexOf('\n}\n', from) + 3;
const sandbox = { lerp: (a, b, t) => a + (b - a) * t };
vm.createContext(sandbox);
vm.runInContext(`${world.slice(from, to)}\nthis.routePoint = routePoint;`, sandbox);
const { routePoint } = sandbox;

// An L: 4 tiles east, then 2 south. It's 6 tiles long.
const route = [[0, 0], [4, 0], [4, 2]];
const near = (got, x, y) => assert.ok(Math.abs(got.x - x) < 1e-9 && Math.abs(got.y - y) < 1e-9, JSON.stringify(got));

near(routePoint(route, 0), 0, 0);
near(routePoint(route, 0.5), 3, 0);
near(routePoint(route, 2 / 3), 4, 0);
near(routePoint(route, 5 / 6), 4, 1);
near(routePoint(route, 1), 4, 2);

// It faces the way it's going: east on the first run, south on the second.
assert.ok(routePoint(route, 0.25).dx > 0 && routePoint(route, 0.25).dy === 0);
assert.ok(routePoint(route, 0.9).dy > 0 && routePoint(route, 0.9).dx === 0);
// At the end it keeps the heading it arrived with.
assert.ok(routePoint(route, 1).dy > 0);

// Before it leaves or after it arrives it's at an end; a bad fraction is the start.
near(routePoint(route, -3), 0, 0);
near(routePoint(route, 7), 4, 2);
near(routePoint(route, NaN), 0, 0);
// One point is a car that goes nowhere, and no route is no car.
near(routePoint([[2, 3]], 0.5), 2, 3);
assert.equal(routePoint([], 0.5), null);
console.log('ok');
