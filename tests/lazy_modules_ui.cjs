// #913: three small modules load when something asks for them, not with the map. tutorial.js stays
// static on purpose: its Get started button and arrive() run on every page load.
const assert = require('node:assert');
const fs = require('node:fs');

const app = fs.readFileSync('web/app.js', 'utf8').split('\r\n').join('\n');
const imports = app.split('\n').filter((line) => line.startsWith('import '));

for (const file of ['passkey', 'plan-invite', 'bot-setup']) {
  assert(!imports.some((line) => line.includes(`./${file}.js`)), `${file}.js is imported statically`);
  assert(app.includes(`import('./${file}.js')`), `${file}.js is never loaded with import()`);
}
assert(!/^await /m.test(app), 'top-level await in app.js');
console.log('lazy modules ok');
