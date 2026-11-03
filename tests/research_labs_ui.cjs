// Run with node tests/research_labs_ui.cjs. Exercise the shipped lab panel handlers.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const source = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
function extract(name) {
  let start = source.indexOf(`function ${name}(`);
  assert.ok(start >= 0, `missing ${name}`);
  if (source.slice(start - 6, start) === 'async ') start -= 6;
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
let workReply = { repository: 'AgentsHangout/Cure-MS', fetched_at: 100, items: [{ number: 1, title: 'Gather evidence', kind: 'issue', state: 'open' }], contributions: [{ number: 2, bot_name: 'Research bot', owner_name: 'Ann', verified_merged: true }], bots: [] };
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
    if (path.endsWith('/work')) return workReply;
    return { eligibility: { can_create: canCreate, age_days: 29, friends: 30 }, labs: [lab] };
  },
};
vm.createContext(context);
vm.runInContext(`${extract('researchRepositoryLink')}\n${extract('researchMapObject')}\n${extract('researchMapCard')}\n${extract('researchMenuLink')}\n${extract('researchLabsPanel')}\n${extract('researchWorkContent')}
${extract('researchLabLookup')}
${extract('researchDisplay')}\n${extract('researchParticipation')}\n${extract('researchItemDetails')}\nthis.panel = researchLabsPanel; this.workContent = researchWorkContent; this.researchDisplay = researchDisplay; this.lookup = researchLabLookup; this.repositoryLink = researchRepositoryLink; this.participation = researchParticipation; this.mapObject = researchMapObject; this.mapCard = researchMapCard; this.menuLink = researchMenuLink;`, context);
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
  // The research display panel: a GitHub snapshot in its own states, never sample data.
  const text = tree => find(tree, n => n.role === 'status').children.join('');
  const pressed = tree => find(tree, n => n['aria-pressed'] === 'true').children.join('');
  const articles = tree => { const all = []; (function walk(n) { if (!n || typeof n !== 'object' || n.hidden) return; if (n.tag === 'article') all.push(n); (n.children || []).forEach(walk); })(tree); return all; };
  const fresh = { repository: lab.github, fetched_at: 100, stale: false, items: [
    { number: 7, title: 'Add a review', kind: 'pull_request', state: 'closed' },
    { number: 8, title: 'Trial notes', kind: 'issue', state: 'open' }] };
  let shown = node('section', {}, ...context.workContent(fresh));
  assert.ok(text(shown).startsWith('Refreshed'), 'a fresh snapshot says when it was refreshed');
  assert.equal(pressed(shown), 'Pull requests (1)', 'pull requests open first, counted from the snapshot');
  assert.equal(articles(shown).length, 1, 'only the chosen tab shows');
  assert.equal(find(shown, n => n.tag === 'a').rel, 'noopener noreferrer');
  assert.equal(find(shown, n => n.tag === 'a').href, 'https://github.com/AgentsHangout/Cure-MS/pull/7');
  assert.ok(find(shown, n => n.children?.includes('Pull request · closed')), 'a closed pull request is shown as closed');
  assert.ok(!JSON.stringify(shown.children.filter(n => n.tag === 'div')).includes('merged'), 'the snapshot rows never claim a merge');
  find(shown, n => n.tag === 'button' && n.children[0].startsWith('Issues')).events.click();
  assert.equal(pressed(shown), 'Issues (1)');
  assert.equal(articles(shown)[0].children[0].children[0].href, 'https://github.com/AgentsHangout/Cure-MS/issues/8');
  shown = node('section', {}, ...context.workContent({ ...fresh, items: [] }));
  assert.ok(text(shown).startsWith('Refreshed'), 'an empty snapshot is still a successful one');
  assert.ok(find(shown, n => n.children?.includes('No public issues or pull requests in this snapshot.')));
  shown = node('section', {}, ...context.workContent({ ...fresh, stale: true }));
  assert.ok(text(shown).startsWith('Stale'), 'a failed refresh keeps the last snapshot and says it is stale');
  assert.equal(articles(shown).length, 1);
  // RW8 to RW11: areas, the last CI read, claims and watchers, and a watch button with a lab id.
  const rich = { repository: lab.github, fetched_at: 100, stale: false, items: [
    { number: 9, title: 'Fix the plots', kind: 'pull_request', state: 'open', labels: ['bug'], areas: ['Bugs'],
      pr_state: { ci: 'failing', review: 'changes_requested', stale: false },
      claims: [{ name: 'Rover', pairing: false }, { name: 'Ann', pairing: true }],
      watchers: [{ name: 'Rover', is_bot: true }], you_watch: false },
    { number: 10, title: 'Never read', kind: 'pull_request', state: 'open', labels: ['docs'], areas: [] }] };
  const says = (tree, words) => find(tree, n => n.children?.some(t => typeof t === 'string' && t.includes(words)));
  shown = node('section', {}, ...context.workContent(rich, 'lab-1'));
  assert.ok(says(shown, 'Areas: Bugs'), 'areas show when the owner named some');
  assert.ok(says(shown, 'Labels: docs'), 'labels show when no area matches');
  assert.ok(says(shown, 'CI failing · review changes requested'), 'the last CI and review read shows');
  assert.ok(says(shown, 'CI and review not checked yet'), 'a pull request never read is not shown as passing');
  assert.ok(says(shown, 'Working on it: Rover (leading), Ann (pairing)'), 'claims show who leads and who pairs');
  const watchButton = find(shown, n => n.tag === 'button' && n.children?.[0] === 'Watch');
  assert.ok(says(shown, 'Watching: 1'));
  await watchButton.events.click();
  assert.equal(writes.at(-1).path, '/api/research-labs/lab-1/work/9/watch');
  assert.equal(writes.at(-1).method, 'PUT');
  assert.equal(watchButton.children[0], 'Stop watching');
  assert.ok(says(shown, 'Watching: 2'));
  responseFails = true;
  await watchButton.events.click();
  assert.equal(watchButton.children[0], 'Stop watching', 'a failed unwatch keeps showing it as watched');
  responseFails = false;
  await watchButton.events.click();
  assert.equal(writes.at(-1).method, 'DELETE');
  assert.equal(watchButton.children[0], 'Watch');
  shown = node('section', {}, ...context.workContent(rich));
  assert.equal(find(shown, n => n.tag === 'button' && n.children?.[0] === 'Watch'), undefined, 'no lab id, no watch button');
  shown = node('section', {}, ...context.workContent({ ...fresh, stale: true, fetched_at: 0, items: [] }));
  assert.ok(text(shown).startsWith('GitHub snapshot unavailable'), 'nothing saved and nothing fetched is unavailable, not empty');
  assert.equal(find(shown, n => n.tag === 'button'), undefined, 'an unavailable snapshot shows no tabs or rows');
  assert.equal(find(shown, n => n.children?.includes('No public issues or pull requests in this snapshot.')), undefined);
  // Using the display opens this room's workroom and carries no lab id of its own.
  assert.ok(source.includes("else if (item?.kind === 'research_display') useDisplay();"));
  assert.ok(source.includes("item.kind === 'research_display' ? action('Use display', useDisplay) : null"), 'the Build list gives keyboard users a way to use the display');
  // A walk-in can't open Build, so the room's More panel has the same action for everyone here.
  const roomStart = source.indexOf('async function roomView(id)');
  const roomCode = source.slice(roomStart, source.indexOf('\n}\n', roomStart));
  assert.ok(roomCode.includes("const displayMore = h('button', { type: 'button', class: 'wd-btn wide', hidden: true, onclick: useDisplay }, 'Use research display');"), 'More has a keyboard button for the display');
  assert.ok(roomCode.includes("displayMore.hidden = !snapshot.items.some((item) => item.kind === 'research_display');"), 'it shows only while the room has a display');
  const moreList = roomCode.slice(roomCode.indexOf("panels.add('more'"), roomCode.indexOf('].filter(Boolean)));', roomCode.indexOf("panels.add('more'")));
  assert.ok(/\n      displayMore,\n/.test(moreList), 'the button sits in More unconditionally, not behind build rights or membership');
  const said = [];
  const opened = [];
  const displayPanels = { open(name) { opened.push(name); } };
  await context.researchDisplay({ panels: displayPanels, say: t => said.push(t), research: {}, state: 'lab', lookup() { throw Error('no lookup in a lab'); } })();
  assert.deepEqual(opened, ['research'], 'in a lab the display opens the room workroom');
  await context.researchDisplay({ panels: displayPanels, say: t => said.push(t), research: null, state: 'none', lookup() { throw Error('a known non-lab is not looked up again'); } })();
  assert.ok(said.at(-1).startsWith('Sample bars'), 'a confirmed non-lab says its bars are sample art');
  let lookups = 0;
  const retrying = context.researchDisplay({ panels: displayPanels, say: t => said.push(t), research: null, state: 'unknown', lookup: async () => (++lookups === 1 ? 'unknown' : 'none') });
  await retrying();
  assert.ok(said.at(-1).startsWith('Couldn’t check'), 'a failed lookup is never called sample art');
  await retrying();
  assert.equal(lookups, 2, 'each use retries an unknown room');
  assert.ok(said.at(-1).startsWith('Sample bars'), 'a retry that confirms a non-lab says so');
  const realApi = context.api;
  const failing = status => async () => { const e = Error('no'); e.status = status; throw e; };
  context.api = failing(404);
  assert.equal((await context.lookup('room-1')).state, 'none', 'only a 404 means not a lab');
  context.api = failing(500);
  assert.equal((await context.lookup('room-1')).state, 'unknown', 'a server error leaves the room unknown');
  context.api = async () => { throw Error('offline'); };
  assert.equal((await context.lookup('room-1')).state, 'unknown', 'a network error leaves the room unknown');
  context.api = realApi;
  assert.equal((await context.lookup('floor-1')).state, 'lab');
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
  // A saved studio exterior must not hide a lab's dedicated building (day or night).
  const buildingStart = world.indexOf('function buildingObject(');
  const building = world.slice(buildingStart, world.indexOf('\n}\n', buildingStart) + 2);
  const painted = [];
  const graphics = { save() {}, restore() {}, scale() {} };
  const renderContext = vm.createContext({
    sprites: new Map(), LOD: 1, SIDING: ['wall'], ROOFS: [], DOORC: [], AWN: [],
    KIND_NAME: { 10: 'studio' }, BUILDERS: { 10: [() => painted.push('studio')] },
    isBig: () => false, ih: () => 0, clamp: (n, a, b) => Math.max(a, Math.min(b, n)),
    placedResearchLab: () => painted.push('lab'), researchEmblem: () => painted.push('emblem'),
    paintOutsideStanding: () => painted.push('custom studio'),
    paintOutsideGround: () => painted.push('yard'),
    object(key, draw) { draw(graphics); return {}; },
    sprite(key, w, h, ax, ay, draw) { draw(graphics); return {}; },
  });
  vm.runInContext(`${building}\nthis.buildingObject = buildingObject;`, renderContext);
  const exterior = { key: 'saved', outside: { body: { floors: 3 } } };
  const labSprite = renderContext.buildingObject(10, 0, 0, 0, 0, exterior, true);
  labSprite.lit();
  labSprite.ground();
  assert.deepEqual(painted, ['lab', 'emblem', 'lab', 'emblem', 'yard']);
  painted.length = 0;
  renderContext.buildingObject(10, 0, 0, 0, 0, exterior, false);
  assert.deepEqual(painted, ['custom studio'], 'non-lab outside customizations still render');
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
