// node tests/blocks_ui.cjs: A100, A104 (#685). A person's card offers Block and Mute their
// mentions, then Unblock or Unmute once done, from GET /api/blocks; a blocked person shows no mute.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
const tick = () => new Promise((r) => setImmediate(r));
(async () => {
  let lists = { blocks: [], mutes: [] };
  const calls = [];
  const api = async (path, options = {}) => {
    calls.push({ path, method: options.method || 'GET', body: options.body });
    if (path === '/api/blocks' && options.method === 'POST') lists = { ...lists, blocks: [{ person_id: options.body.person_id }] };
    if (path.startsWith('/api/blocks/') && options.method === 'DELETE') lists = { ...lists, blocks: [] };
    if (path === '/api/mutes' && options.method === 'POST') lists = { ...lists, mutes: [{ person_id: options.body.person_id }] };
    return JSON.parse(JSON.stringify(lists));
  };
  const context = { api };
  vm.createContext(context);
  const start = app.indexOf('function blockTies() {');
  const end = app.indexOf('\n}\n', start) + 3;
  vm.runInContext(app.slice(start, end) + '\nthis.blockTies = blockTies;', context);
  const ties = context.blockTies();
  await tick();
  const labels = () => ties.items('p 1', 'Bo').map((item) => item.label);
  assert.deepEqual([...labels()], ['Block Bo', 'Mute their mentions']);
  await ties.items('p 1', 'Bo')[1].run();
  assert.deepEqual([...labels()], ['Block Bo', 'Unmute their mentions']);
  await ties.items('p 1', 'Bo')[0].run();
  const posted = calls.find((c) => c.path === '/api/blocks' && c.method === 'POST');
  assert.equal(JSON.stringify(posted.body), JSON.stringify({ person_id: 'p 1' }));
  assert.deepEqual([...labels()], ['Unblock Bo'], 'blocked: no separate mute item');
  await ties.items('p 1', 'Bo')[0].run();
  assert.ok(calls.some((c) => c.path === '/api/blocks/p%201' && c.method === 'DELETE'), 'the id is encoded');
  assert.equal(labels()[0], 'Block Bo');
  console.log('blocks ui ok');
})().catch((error) => { console.error(error); process.exit(1); });
