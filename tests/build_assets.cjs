// W101: default buildings stay within their plot; the picker reuses those exact renderers.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('web/world.js', 'utf8').replace(/\r\n/g, '\n');
const names = ['House', 'Cafe', 'Club', 'ResearchLab', 'Studio', 'Lounge', 'Bar', 'Office', 'Workshop', 'Library'];
const graphics = new Proxy({}, { get: () => () => {} });
const geometry = [];
function footprint(rect, height = 0) { geometry.push({rect: [...rect], height}); }
const context = vm.createContext({
  BRICKS: ['brick', 'brick', 'brick', 'brick'],
  paving: (_, rect) => footprint(rect), box: (_, rect, low, high) => footprint(rect, high),
  flatRoof: (_, rect, height) => footprint(rect, height),
  gable: (_, rect, height, peak, color, over) => footprint([rect[0]-over, rect[1]-over, rect[2]+over, rect[3]+over], height+peak),
  brickChimney: (_, u, v, low, high, width) => footprint([u, v, u+width, v+width], high),
  awning: (_, rect, height, depth) => footprint([rect[0], rect[1], rect[2]+depth, rect[3]+depth], height),
  picket: (_, u0, v0, u1, v1) => footprint([u0, v0, u1, v1]),
  shadow() {}, drawTree() {}, slope() {}, P: (u,v,z=0) => [u-v,(u+v)/2-z],
});
for (const name of names) {
  const start = source.indexOf(`function placed${name}(`);
  const code = source.slice(start, source.indexOf('\n}\n', start) + 2);
  vm.runInContext(`${code}; this.paint = placed${name};`, context);
  geometry.length = 0;
  context.paint({g: graphics, day: true, seed: 1, v: 0, pal: 0, door: 'L'}, {wall:'wall', roof:'roof', door:'door', awn:['a','b']});
  assert.ok(geometry.length, `${name} paints a building`);
  for (const {rect, height} of geometry) {
    assert.ok(rect.every(value => value >= 1 && value <= 31), `${name} stays inside its ground tile: ${rect}`);
    assert.ok(height <= 25, `${name} has a compact roofline: ${height}`);
  }
}
const icons = source.slice(source.indexOf('const PLACE_ICONS = {'), source.indexOf('\n};', source.indexOf('const PLACE_ICONS = {')));
for (const name of names.filter(name => name !== 'ResearchLab')) assert.ok(icons.includes(`placed${name}`), `${name} icon uses the map renderer`);
console.log('W101: all default building footprints, compact rooflines, and shared picker renderers verified.');
