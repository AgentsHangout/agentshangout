// Run with node tests/research_labs_ui.cjs. Exercise the shipped lab panel handlers.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const source = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
function extract(name) {
  const start = source.indexOf(`function ${name}(`);
  assert.ok(start >= 0, `missing ${name}`);
  return source.slice(start, source.indexOf('\n}\n', start) + 2);
}
function node(tag, props = {}, ...children) {
  return { tag, ...props, children: children.flat().filter(Boolean),
    events: {}, addEventListener(kind, handler) { this.events[kind] = handler; },
    append(...next) { this.children.push(...next.flat().filter(Boolean)); },
    replaceChildren(...next) { this.children = next.flat().filter(Boolean); } };
}
function find(tree, predicate) {
  if (!tree || typeof tree !== 'object') return undefined;
  return predicate(tree) ? tree : (tree.children || []).map(child => find(child, predicate)).find(Boolean);
}
let canCreate = false;
let responseFails = false;
let placed;
let body;
let open;
const writes = [];
const reads = [];
const lab = { id: 'lab-1', name: 'MS (Multiple Sclerosis) Research Lab', floor_id: 'floor-1', github: 'AgentsHangout/Cure-MS', can_place: true };
const context = {
  h: node,
  field: (label, element) => node('label', {}, label, element),
  link: (href, text, props) => node('a', { ...props, href }, text),
  formError: () => node('p', { hidden: true }),
  whenText: () => 'just now',
  submitter: (error, action) => async values => {
    try { await action(values); } catch (failure) { error.hidden = false; error.textContent = failure.message; }
  },
  api: async (path, options) => {
    if (options) {
      writes.push({ path, ...options });
      if (responseFails) throw Error('Repository unavailable');
      return path.includes('/bots/') ? { enabled: options.body.enabled } : lab;
    }
    reads.push(path);
    if (path === '/api/research-labs/floor-1') return lab;
    if (path.endsWith('/work')) return { repository: lab.github, items: [{ number: 1, title: 'Gather evidence', kind: 'issue', state: 'open' }], contributions: [{ number: 2, bot_name: 'Research bot', owner_name: 'Ann', verified_merged: true }], bots: [] };
    return { eligibility: { can_create: canCreate, age_days: 29, friends: 30 }, labs: [lab] };
  },
};
vm.createContext(context);
vm.runInContext(`${extract('researchRepositoryLink')}\n${extract('researchLabsPanel')}\n${extract('researchWorkContent')}\n${extract('researchParticipation')}\nthis.panel = researchLabsPanel; this.repositoryLink = researchRepositoryLink; this.participation = researchParticipation;`, context);
const panels = {
  add(name, title, options) { open = options.onOpen; return body = node('section'); },
  toggle() { return node('button'); },
};
async function check() {
  const panel = context.panel({ panels, me: { kind: 'person' }, place: lab => { placed = lab; } });
  await open();
  assert.equal(find(body, node => node.type === 'submit').disabled, true, 'ineligible owner sees a locked create button');
  assert.equal(find(body, node => node.name === 'github').value, 'AgentsHangout/Cure-MS');
  assert.equal(find(body, node => node.name === 'name').value, 'MS (Multiple Sclerosis) Research Lab');
  assert.equal(find(body, node => node.href?.startsWith('/h/')).href, '/h/floor-1', 'lab opens its shared room');
  assert.ok(find(body, node => node.children?.includes('Place lab on map')), 'unplaced owned lab can be placed');
  canCreate = true;
  await panel.load();
  assert.equal(find(body, node => node.type === 'submit').disabled, false);
  let form = find(body, node => node.tag === 'form');
  responseFails = true;
  await form.onsubmit({ name: ' MS (Multiple Sclerosis) Research Lab ', purpose: ' A shared purpose ', github: ' AgentsHangout/Cure-MS ' });
  assert.equal(placed, undefined, 'a failed create must not select a building to place');
  assert.ok(find(body, node => node.textContent === 'Repository unavailable'), 'write errors stay visible');
  responseFails = false;
  await form.onsubmit({ name: ' MS (Multiple Sclerosis) Research Lab ', purpose: ' A shared purpose ', github: ' AgentsHangout/Cure-MS ' });
  assert.equal(writes.at(-1).path, '/api/research-labs');
  assert.equal(writes.at(-1).body.name, 'MS (Multiple Sclerosis) Research Lab');
  assert.equal(placed.id, 'lab-1', 'successful create starts map placement');
  assert.equal(context.repositoryLink('javascript:alert(1)'), null, 'repository links reject arbitrary URLs');
  assert.equal(context.repositoryLink('AgentsHangout/Cure-MS').href, 'https://github.com/AgentsHangout/Cure-MS');
  const participation = context.participation('lab-1', [{ id: 'bot-1', name: 'Research bot', enabled: true }]);
  const toggle = find(participation, node => node.type === 'checkbox');
  assert.equal(toggle.checked, true, 'participation starts at the server default');
  toggle.checked = false;
  responseFails = true;
  await toggle.events.change();
  assert.equal(toggle.checked, true, 'failed opt-out restores confirmed value');
  responseFails = false;
  toggle.checked = false;
  await toggle.events.change();
  assert.equal(toggle.checked, false, 'successful opt-out persists');
  assert.equal(writes.at(-1).path, '/api/research-labs/lab-1/bots/bot-1');
  assert.equal(writes.at(-1).method, 'PUT');
  const buildStart = source.indexOf('  function build(tile)');
  const build = source.slice(buildStart, source.indexOf('\n  }\n', buildStart) + 4);
  Object.assign(context, {
    tool: 'studio', me: { kind: 'person' }, pendingResearchLab: lab, placingResearchLab: false,
    say() {}, choose(name) { context.tool = name; },
    watch: { soon() {} }, refreshTown() {},
  });
  vm.runInContext(`${build}\nthis.build = build;`, context);
  responseFails = true;
  context.build({ x: 47, y: 41 });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(context.pendingResearchLab.id, lab.id, 'a refused plot keeps the lab ready for another tile');
  assert.equal(context.placingResearchLab, false);
  responseFails = false;
  context.build({ x: 47, y: 42 });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(writes.at(-1).path, '/api/research-labs/lab-1/place');
  assert.equal(writes.at(-1).body.x, 47);
  assert.equal(writes.at(-1).body.y, 42);
  assert.equal(context.pendingResearchLab, null);
  assert.equal(context.tool, null, 'successful placement puts the build tool away');
  reads.length = 0;
  const roomPanel = context.panel({ panels, me: { kind: 'person' }, roomId: 'floor-1', openBoards() {} });
  await roomPanel.load();
  assert.equal(reads.includes('/api/research-labs'), false, 'closed lab rooms use their accessible detail, not the public directory');
  assert.ok(find(body, node => node.href === 'https://github.com/AgentsHangout/Cure-MS/issues/1'), 'workroom links to the source issue');
  assert.ok(find(body, node => node.children?.includes('✦ Research contributor badge')), 'verified merged work displays bot swag');
  assert.ok(find(body, node => node.children?.some(text => typeof text === 'string' && text.includes('Ann’s bot'))), 'awards display bot and owner names');
  console.log('Research UI: eligibility, defaults, shared-room door, placement and failed creates verified.');
}
check().catch(error => { console.error(error); process.exitCode = 1; });
