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
    setAttribute(name, value) { this[name] = value; },
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
vm.runInContext(`${extract('researchRepositoryLink')}\n${extract('researchMapObject')}\n${extract('researchMapCard')}\n${extract('researchMenuLink')}\n${extract('researchLabsPanel')}\n${extract('researchWorkContent')}\n${extract('researchParticipation')}\nthis.panel = researchLabsPanel; this.repositoryLink = researchRepositoryLink; this.participation = researchParticipation; this.mapObject = researchMapObject; this.mapCard = researchMapCard; this.menuLink = researchMenuLink;`, context);
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
    tool: 'research_lab', me: { kind: 'person' }, pendingResearchLab: lab, placingResearchLab: false,
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
  // RL7: map identity comes from explicit class metadata, never an ordinary studio name.
  const mapLab = { x: 4, y: 8, kind: 'research_lab', github: lab.github, github_url: 'javascript:alert(1)' };
  const view = { research_labs: [mapLab] };
  assert.equal(context.mapObject(view, 4, 8), mapLab);
  assert.equal(context.mapObject(view, 4, 9), null);
  const card = context.mapCard(mapLab);
  assert.equal(card.tag, 'span', 'repository belongs in one ordinary card row');
  assert.equal(card.class, 'city-card-hint', 'use the existing hover-card row style');
  const css = fs.readFileSync('web/style.css', 'utf8');
  assert.equal(css.includes('.city-card-research'), false, 'no separate research hover layout');
  assert.equal(css.includes('.city-card.research-object'), false, 'no special research box styling');
  const menuLink = context.menuLink(mapLab.github);
  assert.equal(menuLink.className, 'game-menu-item', 'the repository uses the established menu style');
  assert.equal(menuLink.role, 'menuitem');
  assert.equal(context.menuLink('javascript:alert(1)'), null);
  const tileMenuStart = source.indexOf('  function tileMenu(t)');
  const tileMenu = source.slice(tileMenuStart, source.indexOf('\n  }\n', tileMenuStart) + 4);
  Object.assign(context, { latest: view, me: null, BIG_KINDS: new Set(), KIND_NAME: { studio: 'Studio' } });
  vm.runInContext(`${tileMenu}\nthis.tileMenu = tileMenu;`, context);
  const tile = { x: 4, y: 8, kind: 'studio', level: 0 };
  assert.equal(context.tileMenu(tile).sub, 'Research lab');
  assert.equal(context.tileMenu(tile).items[0].repository, lab.github);
  context.latest = { research_labs: [] };
  assert.equal(context.tileMenu(tile).sub, 'Studio', 'ordinary studios keep their established identity');
  context.me = { kind: 'person' };
  assert.equal(find(card, n => n.tag === 'a').href, 'https://github.com/AgentsHangout/Cure-MS', 'hover links are constructed from a validated repository, never an arbitrary supplied URL');
  assert.equal(context.mapObject({ research_labs: [{ ...mapLab, github: 'owner/../../bad' }] }, 4, 8), null);
  assert.equal(context.repositoryLink('../bad'), null);
  assert.equal(find(card, n => n.tag === 'a').target, '_blank');
  // RL7: keep the actionable hover card stationary while its GitHub link is approached.
  const cardAtStart = source.indexOf('  function placeCardAt()');
  const cardAt = source.slice(cardAtStart, source.indexOf('\n  }\n', cardAtStart) + 4);
  context.pointer = { x: 100, y: 100 };
  context.placeCard = { hidden: false, dataset: {}, style: {}, offsetWidth: 200, offsetHeight: 130,
    classList: { contains: name => name === 'has-repository' },
    parentElement: { getBoundingClientRect: () => ({ left: 0, top: 0, width: 800, height: 600 }) } };
  vm.runInContext(`${cardAt}\nthis.placeCardAt = placeCardAt;`, context);
  context.placeCardAt();
  const left = context.placeCard.style.left;
  context.pointer = { x: 140, y: 130 };
  context.placeCardAt();
  assert.equal(context.placeCard.style.left, left, 'the research card must not chase the pointer');
  // RL8: a fresh menu choice opens lab creation; it cannot fall through to generic studio placement.
  const pickStart = source.indexOf('  function pick(name)');
  const pick = source.slice(pickStart, source.indexOf('\n  }\n', pickStart) + 4);
  context.panels = { open(name) { context.openedPanel = name; } };
  vm.runInContext(`${pick}\nthis.pick = pick;`, context);
  context.pendingResearchLab = null;
  context.pick('research_lab');
  assert.equal(context.openedPanel, 'research');
  context.tool = 'research_lab';
  context.openedPanel = null;
  context.build({ x: 47, y: 42 });
  assert.equal(context.openedPanel, 'research');
  assert.ok(source.includes("['research_lab', 'Research lab',"), 'the lab is a regular build-menu choice');
  // RL7: renderer class changes invalidate only affected standing buildings, including removal.
  const world = fs.readFileSync('web/world.js', 'utf8').replace(/\r\n/g, '\n');
  const applyStart = world.indexOf('  applyResearchLabs(rows)');
  const apply = world.slice(applyStart, world.indexOf('\n  }\n', applyStart) + 4);
  context.K_HOUSE = 7;
  vm.runInContext(`this.applyResearchLabs = function ${apply.trim()};`, context);
  const dirtied = [];
  const render = { n: 16, kind: new Uint8Array(256), researchLabs: new Set(), markDirty(x, y) { dirtied.push([x, y]); } };
  render.kind[8 * 16 + 4] = 10;
  context.applyResearchLabs.call(render, [mapLab, { x: -1, y: 0 }, { x: 2, y: 2 }]);
  assert.equal(render.researchLabs.size, 1);
  assert.deepEqual(dirtied, [[4, 8]]);
  context.applyResearchLabs.call(render, [mapLab]);
  assert.equal(dirtied.length, 1, 'unchanged lab classes do not repaint');
  context.applyResearchLabs.call(render, []);
  assert.equal(render.researchLabs.size, 0);
  assert.equal(dirtied.length, 2, 'closing or removing a lab repaints its object');
  console.log('Research UI: eligibility, defaults, shared-room door, placement and failed creates verified.');
}
check().catch(error => { console.error(error); process.exitCode = 1; });
