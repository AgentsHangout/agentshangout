// Owner-controlled relationships and permissions. All names are rendered as text.
// The connections are a graph: you, your bots, friends and bonds. Picking a node edits it beside
// the graph, so a bot's name, pause, permissions and friends are changed where they're shown.
const SVG = 'http://www.w3.org/2000/svg';
// Nodes keep clear of the edges so their names, drawn under them, aren't cut off.
const W = 480, H = 300, SIDE = 54, TOP = 30, FOOT = 44;
const within = (x, y) => [Math.min(W - SIDE, Math.max(SIDE, x)), Math.min(H - FOOT, Math.max(TOP, y))];
// Where each kind of node starts before the layout settles: you on top, your bots under you, your
// people to the left, the chosen bot's friends to the right and its bonds below it.
const ANCHOR = {
  you: [W / 2, 52], mine: [W / 2, 150], person: [80, 110], bot: [W - 80, 170], bond: [W / 2 + 70, H - 40],
};
// How long each kind of line wants to be.
const REACH = { owns: 90, friend: 120, pending: 130, bond: 110 };

// Shapes and colours every part of the graph carries itself (BT8). trust.css restyles them, but a
// stylesheet that is late, missing or without its variables can't leave black blobs with no lines.
const TAN = '#e6d6ae', INK = '#2b1d10', DARK = '#4d3620', BRICK = '#a94a26';
const EDGE = {
  owns: { stroke: DARK, 'stroke-width': 2 },
  friend: { stroke: BRICK, 'stroke-width': 2.5 },
  pending: { stroke: BRICK, 'stroke-width': 1.5, 'stroke-dasharray': '2 4', 'stroke-linecap': 'round' },
  bond: { stroke: '#6f8a4a', 'stroke-width': 1.5, 'stroke-dasharray': '7 4' },
};
const TILE = { you: '#6b2216', mine: '#cf7a34', person: '#ecdfbd', bot: '#ecdfbd', bond: '#ecdfbd' };
// How big each kind of frame is: half its width, so the label can sit clear of it.
const HALF = { you: 22, person: 17, mine: 19, bot: 16, bond: 16 };
const LABEL_HIGH = 13;

export function trustPanel({ h, api, panels, me, crew, figure, dress, schedule }) {
  let data, selected, tab = 'connections', draft, saved, busy = false, generation = 0, healthGeneration = 0;
  let monitoring, bonds, config, notice = '', dirty = false, focus = 'bot';
  let usage, serverUsage;
  // Where every node was last drawn, so a change doesn't throw the graph around.
  const placed = new Map();
  const body = panels.add('trust', 'Trust & bots', { onOpen: () => { if (!dirty && !busy) load(); } });
  body.classList.add('trust-body');
  const toggle = panels.toggle('trust', 'Trust & bots', { short: 'Trust' });
  const button = (label, run, props = {}) => h('button', { type: 'button', onclick: run, ...props }, label);
  const path = () => `/api/agents/${encodeURIComponent(selected)}`;
  const bot = () => data?.agents.find(a => a.id === selected);
  const personName = id => data.people.find(p => p.id === id)?.name || 'Person';
  const botName = id => data.bots.find(p => p.id === id)?.name || 'Bot';
  const clone = value => JSON.parse(JSON.stringify(value));
  // BT7: a node's head, drawn by the same art as rooms from what the server says it wears (C16).
  // It's a picture of the head only, scaled up whole so it stays sharp, or null where there's none.
  const heads = new Map();
  function headOf(n) {
    const person = n.kind === 'you' || n.kind === 'person';
    const worn = n.lookId && data?.looks?.[person ? 'people' : 'bots']?.[n.lookId];
    if (!figure || !worn) return null;
    const key = `${person ? 'p' : 'b'}${JSON.stringify(worn)}`;
    if (!heads.has(key)) {
      let url = null;
      try {
        const whole = figure({ kind: person ? 'person' : 'bot', color: '#8b6a3e', worn });
        const read = whole.getContext('2d').getImageData(0, 0, whole.width, whole.height);
        let top = -1, left = whole.width, right = -1;
        for (let y = 0; y < whole.height && top < 0; y++) for (let x = 0; x < whole.width; x++) {
          if (read.data[(y * whole.width + x) * 4 + 3]) { top = y; break; }
        }
        // The head is the top of the figure: a bot's is wide, a person's is narrower than the shoulders.
        const rows = person ? 11 : 13;
        for (let y = top; top >= 0 && y < top + rows; y++) for (let x = 0; x < whole.width; x++) {
          if (read.data[(y * whole.width + x) * 4 + 3]) { left = Math.min(left, x); right = Math.max(right, x); }
        }
        if (top >= 0) {
          const side = Math.max(rows, right - left + 1);
          const centre = (left + right + 1) / 2;
          const out = document.createElement('canvas');
          out.width = out.height = 60;
          const g = out.getContext('2d');
          g.imageSmoothingEnabled = false;
          g.drawImage(whole, Math.round(centre - side / 2), top, side, rows, 0, 0, 60, Math.round(60 * rows / side));
          url = out.toDataURL('image/png');
        }
      } catch (_) { url = null; }
      heads.set(key, url);
    }
    return heads.get(key);
  }
  const text = (words, className = 'muted') => h('p', { class: className }, words);
  const section = (title, ...children) => h('section', { class: 'trust-section' }, h('h3', {}, title), ...children);
  const state = words => h('span', { class: 'trust-tag' }, words);
  const field = (label, control, hint) => h('label', { class: 'trust-field' }, h('span', {}, label), control, hint ? h('small', { class: 'muted' }, hint) : null);
  const error = failure => { notice = failure.message || String(failure); render(); };
  const svg = (tag, attrs = {}, ...children) => {
    if (!document.createElementNS) return h(tag, attrs, ...children);
    const el = document.createElementNS(SVG, tag);
    for (const [key, value] of Object.entries(attrs)) {
      if (value === undefined || value === null || value === false) continue;
      if (key.startsWith('on')) el.addEventListener(key.slice(2), value);
      else el.setAttribute(key, value);
    }
    el.append(...children.flat().filter(child => child !== null && child !== undefined && child !== false));
    return el;
  };
  async function action(run) {
    if (busy) return;
    busy = true; notice = ''; render();
    try { await run(); } catch (failure) { notice = failure.message; }
    finally { busy = false; render(); }
  }
  async function refreshHealthAfterSettings() {
    // Saving settings and reading their resulting health are separate outcomes.
    ++healthGeneration;
    try { monitoring = await api(`${path()}/monitoring`); }
    catch (_) { monitoring = { error: 'Settings saved. Monitoring health could not be refreshed; check again to see the latest state.' }; }
  }
  async function load() {
    body.replaceChildren(text('Loading your bots and relationships…'));
    try {
      data = await api('/api/trust');
      if (!data.agents.some(a => a.id === selected)) selected = data.agents[0]?.id;
      if (selected) await choose(selected); else { focus = 'you'; render(); }
    } catch (failure) {
      body.replaceChildren(text(failure.message, 'error'), button('Try again', load));
    }
  }
  async function choose(id) {
    selected = id; monitoring = bonds = config = usage = undefined; notice = ''; dirty = false; focus = 'bot';
    saved = clone(bot().trust); draft = clone(saved);
    const request = ++generation;
    const healthRequest = ++healthGeneration;
    render();
    // Separate requests: an unavailable monitor never hides relationships or settings.
    for (const [key, endpoint] of [['monitoring', 'monitoring'], ['bonds', 'bonds'], ['config', 'notify']]) {
      api(`${path()}/${endpoint}`).then(value => {
        if (generation !== request || (key === 'monitoring' && healthRequest !== healthGeneration)) return;
        if (key === 'monitoring') monitoring = value;
        if (key === 'bonds') bonds = value.bonds;
        if (key === 'config') config = value;
        // Do not replace fields while their owner is editing them.
        if (!dirty && !['INPUT', 'SELECT', 'TEXTAREA'].includes(document.activeElement?.tagName)) render();
      }).catch(failure => {
        if (generation !== request || (key === 'monitoring' && healthRequest !== healthGeneration)) return;
        if (key === 'monitoring') monitoring = { error: failure.message };
        if (key === 'bonds') bonds = { error: failure.message };
        if (key === 'config') config = { error: failure.message };
        if (!dirty) render();
      });
    }
  }
  function render() {
    if (!data) return;
    // U21: someone who runs the server reaches the server totals even with no bots of their own.
    const operator = me?.god_mode !== undefined && me?.god_mode !== null;
    const tabs = data.agents.length ? [['connections', 'Connections'], ['monitoring', 'Monitoring'], ['usage', 'Usage']] : operator ? [['connections', 'Connections'], ['usage', 'Usage']] : [];
    if (!tabs.length) tab = 'connections';
    const chooser = data.agents.length ? field('Bot', h('select', { 'aria-label': 'Bot to manage', disabled: busy || dirty, onchange: e => choose(e.target.value) },
      ...data.agents.map(a => h('option', { value: a.id, selected: a.id === selected }, a.name)))) : null;
    const nav = tabs.length ? h('div', { class: 'row-actions trust-tabs', role: 'tablist', 'aria-label': 'Bot settings' }, ...tabs.map(([id, label]) => button(label, () => { tab = id; notice = ''; render(); }, {
      role: 'tab', id: `trust-tab-${id}`, class: tab === id ? 'primary' : '', 'aria-selected': String(tab === id), 'aria-controls': 'trust-content', tabindex: tab === id ? 0 : -1,
      onkeydown: e => {
        if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(e.key)) return;
        e.preventDefault(); const i = tabs.findIndex(t => t[0] === tab);
        tab = tabs[e.key === 'Home' ? 0 : e.key === 'End' ? tabs.length - 1 : (i + (e.key === 'ArrowRight' ? 1 : tabs.length - 1)) % tabs.length][0];
        render(); body.querySelector(`#trust-tab-${tab}`)?.focus();
      },
    }))) : null;
    body.replaceChildren(
      h('div', { class: 'trust-top' }, chooser, nav),
      ...(notice ? [h('p', { class: 'error trust-notice', role: 'alert' }, notice)] : []),
      h('div', { id: 'trust-content', class: 'trust-content', role: tabs.length ? 'tabpanel' : null, 'aria-labelledby': tabs.length ? `trust-tab-${tab}` : null },
        ...(tab === 'monitoring' ? monitorView() : tab === 'usage' ? usageView() : connections())));
    body.querySelectorAll('button').forEach(b => { if (busy) b.disabled = true; });
  }

  // U11, U12: what the chosen bot did lately, as counts, and for the people who run the server, the totals.
  function usageView() {
    if (usage === undefined) {
      const asked = selected;
      if (asked) {
        usage = null;
        api(`${path()}/usage`).then(v => { if (asked === selected) usage = v; }).catch(f => { if (asked === selected) usage = { error: f.message }; }).finally(() => { if (asked === selected) render(); });
      } else usage = false;
      if (serverUsage === undefined) api('/api/usage').then(v => { serverUsage = v; render(); }).catch(() => { serverUsage = false; });
    }
    const table = (head, rows) => h('table', { class: 'trust-usage' }, h('thead', {}, h('tr', {}, ...head.map(c => h('th', {}, c)))), h('tbody', {}, ...rows.map(r => h('tr', {}, ...r.map(c => h('td', {}, String(c ?? '-')))))));
    const own = usage === false ? [] : !usage ? [text('Counting…')] : usage.error ? [text(usage.error, 'error')] : [
      section('This bot, last 14 days', table(['Day', 'Calls', 'Refused', 'Errors'], usage.days.slice(-14).reverse().map(d => [d.day, d.calls, d.refused, d.errors])),
        text(Object.keys(usage.refusals).length ? `Refusals by reason: ${Object.entries(usage.refusals).map(([k, n]) => `${k} ${n}`).join(', ')}.` : 'No refusals in the last 30 days.'),
        text(usage.last_activity_at ? `Last did anything: ${new Date(usage.last_activity_at * 1000).toLocaleString()}.` : 'This bot has not done anything yet.'))];
    const all = !serverUsage ? [] : [
      section('This server, last 7 days (operators only)',
        text(serverUsage.lines ? `Events delivered ${serverUsage.lines.events_delivered}, long-polls ${serverUsage.lines.long_polls}, webhook POSTs ${serverUsage.lines.webhooks_sent} (${serverUsage.lines.webhooks_failed} failed).` : ''),
        table(['Tool or route', 'Calls', 'Refused', 'Errors', 'Avg ms', 'Median ms', 'p90 ms', 'Slow', 'Top refusal'], serverUsage.by_tool.slice(0, 25).map(t => [t.name, t.calls, t.refused, t.errors, t.avg_ms, t.p50_ms, t.p90_ms, t.slow, t.top_refusal])),
        table(['Day', 'People', 'Bots', 'Calls', 'Refused', 'Median ms', 'p90 ms', 'Events', 'Long-polls', 'Webhooks', 'Failed'], serverUsage.by_day.slice().reverse().map(d => [d.day, d.active_people, d.active_bots, d.calls, d.refused, d.p50_ms, d.p90_ms, d.lines?.events_delivered, d.lines?.long_polls, d.lines?.webhooks_sent, d.lines?.webhooks_failed])),
        text('Last 24 hours (UTC)'),
        table(['Hour', 'People', 'Bots', 'Calls', 'Refused', 'Median ms', 'p90 ms'], (serverUsage.by_hour || []).slice(-24).reverse().map(d => [d.hour.slice(5, 16).replace('T', ' '), d.active_people, d.active_bots, d.calls, d.refused, d.p50_ms, d.p90_ms])),
        table(['Version', 'Calls', 'Refused', 'Errors', 'Avg ms', 'Median ms', 'p90 ms'], serverUsage.by_version.map(v => [v.version, v.calls, v.refused, v.errors, v.avg_ms, v.p50_ms, v.p90_ms])))];
    return [text(selected ? 'Counts only. No messages, no room names, nothing about what the bot said. Kept for 30 days.' : 'Counts only. No messages, no room names, nobody’s history. Kept for 30 days.'), ...own, ...all];
  }

  // The graph: who is connected to you and to the chosen bot, and how.
  function graph() {
    const owner = data.owner_id;
    const nodes = new Map(), edges = [];
    const node = (id, kind, name, detail) => { if (!nodes.has(id)) nodes.set(id, { id, kind, name, detail }); return nodes.get(id); };
    const other = (f, id) => f.from_id === id ? f.to_id : f.from_id;
    const waiting = (f, id) => f.to_id === id ? 'Waiting for your answer' : 'Request sent';
    node('you', 'you', me?.name || personName(owner), 'You');
    for (const a of data.agents) {
      Object.assign(node(`mine:${a.id}`, 'mine', a.name, `${a.id === selected ? 'The bot you’re editing' : 'Your bot'}${a.doorbell?.paused ? ', paused' : ''}`), { paused: a.doorbell?.paused });
      edges.push({ from: 'you', to: `mine:${a.id}`, kind: 'owns' });
    }
    for (const f of data.friendships.filter(f => f.kind === 'person' && [f.from_id, f.to_id].includes(owner))) {
      const id = other(f, owner);
      Object.assign(node(`person:${id}`, 'person', personName(id), f.status === 'accepted' ? 'Your friend' : waiting(f, owner)), { friendship: f });
      edges.push({ from: 'you', to: `person:${id}`, kind: f.status === 'accepted' ? 'friend' : 'pending' });
    }
    const finish = () => {
      for (const n of nodes.values()) {
        n.lookId = n.kind === 'you' ? owner : n.kind === 'person' ? n.id.slice(7) : n.kind === 'mine' ? n.id.slice(5) : n.botId;
      }
      return { nodes: [...nodes.values()], edges };
    };
    if (!selected) return finish();
    const centre = `mine:${selected}`;
    for (const f of data.friendships.filter(f => f.kind === 'bot' && [f.from_id, f.to_id].includes(selected))) {
      const id = other(f, selected);
      const at = nodes.has(`mine:${id}`) ? `mine:${id}` : `bot:${id}`;
      Object.assign(node(at, 'bot', botName(id), f.status === 'accepted' ? `${bot().name}’s friend` : waiting(f, selected)), { friendship: f, botId: id });
      edges.push({ from: centre, to: at, kind: f.status === 'accepted' ? 'friend' : 'pending' });
    }
    (Array.isArray(bonds) ? bonds : []).forEach((b, i) => {
      const id = b.agent?.id;
      const at = id && nodes.has(`mine:${id}`) ? `mine:${id}` : id && nodes.has(`bot:${id}`) ? `bot:${id}` : `bond:${id || i}`;
      const n = node(at, 'bond', b.agent?.name || 'A bot you haven’t met', 'Earned bond');
      Object.assign(n, { bond: b, botId: n.botId || id });
      edges.push({ from: centre, to: at, kind: 'bond', strength: b.strength });
    });
    return finish();
  }
  // A small force layout: nodes push apart, lines pull together. Nodes already on screen barely move.
  function layout(nodes, edges) {
    const counts = {};
    const fresh = nodes.filter(n => !placed.has(n.id));
    for (const n of nodes) {
      const was = placed.get(n.id);
      if (was) { n.x = was.x; n.y = was.y; continue; }
      const i = counts[n.kind] = (counts[n.kind] || 0) + 1;
      const [x, y] = ANCHOR[n.kind];
      n.x = x + ((i % 3) - 1) * 70 + i; n.y = y + Math.floor(i / 3) * 40 + (n.id === `mine:${selected}` ? 18 : 0);
    }
    if (fresh.length) {
      const byId = new Map(nodes.map(n => [n.id, n]));
      for (let step = 0; step < 240; step++) {
        const heat = 1 - step / 240;
        for (const n of nodes) { n.vx = 0; n.vy = 0; }
        for (let i = 0; i < nodes.length; i++) for (let j = i + 1; j < nodes.length; j++) {
          const a = nodes[i], b = nodes[j];
          const dx = a.x - b.x || 0.1, dy = a.y - b.y || 0.1, d2 = Math.max(dx * dx + dy * dy, 120);
          const push = 3400 / d2, d = Math.sqrt(d2);
          a.vx += dx / d * push; a.vy += dy / d * push; b.vx -= dx / d * push; b.vy -= dy / d * push;
          // Names are wider than nodes, so two side by side are pushed further apart.
          if (Math.abs(dy) < 28 && Math.abs(dx) < 92) { const side = (92 - Math.abs(dx)) * 0.08 * Math.sign(dx); a.vx += side; b.vx -= side; }
        }
        for (const e of edges) {
          const a = byId.get(e.from), b = byId.get(e.to);
          const dx = b.x - a.x, dy = b.y - a.y, d = Math.max(Math.hypot(dx, dy), 1);
          const pull = (d - REACH[e.kind]) * 0.05;
          a.vx += dx / d * pull; a.vy += dy / d * pull; b.vx -= dx / d * pull; b.vy -= dy / d * pull;
        }
        for (const n of nodes) {
          const [ax, ay] = ANCHOR[n.kind];
          n.vx += (ax - n.x) * 0.012; n.vy += (ay - n.y) * 0.012;
          const give = placed.has(n.id) ? 0.1 : 1;
          [n.x, n.y] = within(n.x + Math.max(-9, Math.min(9, n.vx)) * heat * give, n.y + Math.max(-9, Math.min(9, n.vy)) * heat * give);
        }
      }
    }
    // Then names that would still overlap are moved apart, sideways if they share a row.
    const wide = n => Math.min([...n.name].length, 13) * 6.6 + 10;
    for (let step = 0; fresh.length && step < 80; step++) {
      let moved = false;
      for (let i = 0; i < nodes.length; i++) for (let j = i + 1; j < nodes.length; j++) {
        const a = nodes[i], b = nodes[j];
        const overlapX = (wide(a) + wide(b)) / 2 - Math.abs(a.x - b.x), overlapY = 30 - Math.abs(a.y - b.y);
        if (overlapX <= 0 || overlapY <= 0) continue;
        moved = true;
        const [fixed, free] = placed.has(a.id) && !placed.has(b.id) ? [a, b] : placed.has(b.id) && !placed.has(a.id) ? [b, a] : [null, null];
        const sx = Math.sign(b.x - a.x) || 1, sy = Math.sign(b.y - a.y) || 1;
        // Sideways first, unless the frame's edge stops that; then up and down.
        const room = n => sx > 0 === (n === b) ? W - SIDE - n.x : n.x - SIDE;
        const sideways = overlapX < overlapY * 2 && room(fixed ? free : a) + room(fixed ? free : b) >= overlapX;
        const along = sideways ? [overlapX / 2 + 1, 0] : [0, overlapY / 2 + 1];
        if (fixed) { const d = free === b ? 1 : -1; [free.x, free.y] = within(free.x + d * sx * along[0] * 2, free.y + d * sy * along[1] * 2); continue; }
        [a.x, a.y] = within(a.x - sx * along[0], a.y - sy * along[1]);
        [b.x, b.y] = within(b.x + sx * along[0], b.y + sy * along[1]);
      }
      if (!moved) break;
    }
    for (const n of nodes) placed.set(n.id, { x: n.x, y: n.y });
  }
  // BT8: where each name goes. Under its node unless that would run into another name or node, then
  // above it, then shortened, so names never sit on top of each other or on a head.
  function labels(nodes) {
    const taken = [];
    const bodies = nodes.map(n => ({ id: n.id, x: n.x - HALF[n.kind] - 3, y: n.y - HALF[n.kind] - 3, w: HALF[n.kind] * 2 + 6, h: HALF[n.kind] * 2 + 6 }));
    const placed = new Map();
    const order = [...nodes].sort((a, b) => (a.id === `mine:${selected}` ? -1 : b.id === `mine:${selected}` ? 1 : 0) || a.y - b.y || a.x - b.x);
    for (const n of order) {
      const letters = [...n.name];
      const words = [letters.length > 13 ? `${letters.slice(0, 12).join('')}…` : n.name, letters.length > 8 ? `${letters.slice(0, 7).join('')}…` : null].filter(Boolean);
      // Under, over, then beside, each at full length and then shortened; the least crowded wins.
      const options = [];
      for (const text of words) for (const where of ['below', 'above', 'right', 'left']) options.push({ text, where });
      let chosen, best = Infinity;
      for (const option of options) {
        const w = [...option.text].length * 6.4 + 6, half = HALF[n.kind];
        const at = { below: [n.x, n.y + half + 14], above: [n.x, n.y - half - 6], right: [n.x + half + 4 + w / 2, n.y + 4], left: [n.x - half - 4 - w / 2, n.y + 4] }[option.where];
        const x = Math.min(W - w / 2 - 2, Math.max(w / 2 + 2, at[0])), y = at[1];
        const box = { x: x - w / 2, y: y - LABEL_HIGH + 3, w, h: LABEL_HIGH };
        const overlap = (a, b) => Math.max(0, Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x)) * Math.max(0, Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y));
        const cost = (box.y < 0 || box.y + box.h > H ? 1000 : 0)
          + taken.reduce((sum, t) => sum + overlap(box, t), 0) + bodies.reduce((sum, b) => sum + (b.id === n.id ? 0 : overlap(box, b)), 0);
        if (cost < best) { best = cost; chosen = { option, x, y, box }; }
        if (!cost) break;
      }
      taken.push(chosen.box);
      placed.set(n.id, { text: chosen.option.text, x: chosen.x - n.x, y: chosen.y - n.y });
    }
    return placed;
  }
  function connections() {
    const { nodes, edges } = graph();
    layout(nodes, edges);
    if (!nodes.some(n => n.id === focusId())) focus = selected ? 'bot' : 'you';
    const byId = new Map(nodes.map(n => [n.id, n]));
    // A friend who is also a bond gets two lines, so the second one bows out to stay visible.
    const seen = new Set();
    const lines = edges.map(e => {
      const pair = [e.from, e.to].sort().join(' ');
      const bend = seen.has(pair) ? 24 : 0; seen.add(pair);
      const a = byId.get(e.from), b = byId.get(e.to);
      const line = svg('path', { class: `trust-edge ${e.kind}`, d: curve(a, b, bend), fill: 'none', ...EDGE[e.kind],
        ...(e.kind === 'bond' ? { 'stroke-width': 1 + Math.min(Number(e.strength) || 0, 9) / 3 } : {}) });
      return { e, line, a, b, bend };
    });
    const groups = new Map();
    const names = labels(nodes);
    const picture = svg('svg', { class: 'trust-graph', viewBox: `0 0 ${W} ${H}`, role: 'group', 'aria-label': selected ? `${bot().name}’s connections` : 'Your connections' },
      svg('rect', { class: 'trust-ground', width: W, height: H, fill: TAN }),
      ...lines.map(l => l.line),
      ...nodes.map(n => {
        const half = HALF[n.kind], round = n.kind === 'you' || n.kind === 'person';
        const editing = n.id === `mine:${selected}`;
        const frame = { fill: TILE[n.kind], stroke: DARK, 'stroke-width': editing ? 2.5 : 1.5 };
        const shape = round ? svg('circle', { r: half, ...frame })
          : svg('rect', { x: -half, y: -half, width: half * 2, height: half * 2, rx: 4, ...frame, ...(editing ? { fill: BRICK } : {}) });
        const head = headOf(n);
        // A head sits inside its frame and fills it; a square of side 1.45 r fits a circle of radius r.
        const side = round ? Math.round(half * 1.45) : half * 2 - 6;
        const face = head ? svg('image', { class: 'head', href: head, x: -side / 2, y: -side / 2, width: side, height: side, 'aria-hidden': 'true', 'pointer-events': 'none' }) : null;
        const label = names.get(n.id);
        const g = svg('g', {
          class: `trust-node ${n.kind}${n.id === focusId() ? ' focused' : ''}${editing ? ' editing' : ''}${n.friendship?.status === 'pending' ? ' waiting' : ''}${n.paused ? ' paused' : ''}`,
          transform: `translate(${n.x} ${n.y})`, tabindex: 0, role: 'button', 'aria-pressed': String(n.id === focusId()),
          'aria-label': `${n.name}, ${n.detail}`,
          onkeydown: e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); pick(n); } },
          onpointerdown: e => drag(e, n),
        }, svg('title', {}, `${n.name} · ${n.detail}`),
          svg('circle', { class: 'ring', r: half + 5, fill: 'none', stroke: n.id === focusId() ? '#cf7a34' : 'none', 'stroke-width': 3 }), shape, face,
          face ? null : svg('text', { class: 'initial', 'text-anchor': 'middle', dy: '0.35em', fill: n.kind === 'you' || n.kind === 'mine' ? '#fff3d2' : INK, 'font-size': 13, 'font-weight': 700 }, [...n.name][0]?.toUpperCase() || '?'),
          svg('text', { class: 'label', 'text-anchor': 'middle', x: label.x, y: label.y, fill: INK, stroke: TAN, 'stroke-width': 3, 'paint-order': 'stroke', 'font-size': 11, 'font-weight': n.id === focusId() ? 800 : 600 }, label.text));
        groups.set(n.id, g);
        return g;
      }));
    // Dragging moves a node and its lines without drawing the graph again; a press that doesn't
    // move picks the node.
    function drag(event, n) {
      if (event.button !== 0 || !picture.getScreenCTM) return;
      const start = { x: event.clientX, y: event.clientY };
      let moved = false;
      const at = e => { const p = picture.createSVGPoint(); p.x = e.clientX; p.y = e.clientY; return p.matrixTransform(picture.getScreenCTM().inverse()); };
      const move = e => {
        if (!moved && Math.hypot(e.clientX - start.x, e.clientY - start.y) < 4) return;
        moved = true;
        const p = at(e);
        [n.x, n.y] = within(p.x, p.y);
        placed.set(n.id, { x: n.x, y: n.y });
        groups.get(n.id).setAttribute('transform', `translate(${n.x} ${n.y})`);
        for (const l of lines) if (l.a === n || l.b === n) l.line.setAttribute('d', curve(l.a, l.b, l.bend));
      };
      const up = () => { removeEventListener('pointermove', move); removeEventListener('pointerup', up); if (!moved) pick(n); };
      addEventListener('pointermove', move); addEventListener('pointerup', up);
    }
    const legend = h('p', { class: 'trust-legend fine' },
      h('span', { class: 'swatch owns' }), 'Yours', h('span', { class: 'swatch friend' }), 'Friends',
      h('span', { class: 'swatch pending' }), 'Asked, not answered', h('span', { class: 'swatch bond' }), 'Bond (earned)');
    const about = text(`${selected ? `${bot().name}’s` : 'Your'} connections. Round is a person, square is a bot, and each wears its own look. The ringed one is open below.`, 'muted fine');
    const hint = text('Pick anyone in the graph to see and change them. Drag to move them.', 'muted fine');
    return [about, picture, legend, hint, inspector(byId.get(focusId()))];
  }
  function curve(a, b, bend) {
    if (!bend) return `M${a.x} ${a.y}L${b.x} ${b.y}`;
    const d = Math.max(Math.hypot(b.x - a.x, b.y - a.y), 1);
    const cx = (a.x + b.x) / 2 - (b.y - a.y) / d * bend, cy = (a.y + b.y) / 2 + (b.x - a.x) / d * bend;
    return `M${a.x} ${a.y}Q${cx} ${cy} ${b.x} ${b.y}`;
  }
  function focusId() { return focus === 'bot' ? `mine:${selected}` : focus; }
  function pick(n) {
    if (n.kind === 'mine' && n.id !== `mine:${selected}`) {
      if (dirty) { notice = 'Save or reset this bot’s permissions before choosing another bot.'; render(); return; }
      choose(n.id.slice(5)); return;
    }
    focus = n.id === `mine:${selected}` ? 'bot' : n.id; notice = ''; render();
    body.querySelector('.trust-inspector h3')?.focus?.();
  }

  // What's picked in the graph, and what can be done about it.
  function inspector(n) {
    const box = (...children) => h('section', { class: 'trust-inspector', 'aria-live': 'polite' }, ...children);
    if (!n || n.kind === 'you') {
      const owner = data.owner_id;
      return box(h('h3', { tabindex: -1 }, 'You'),
        !data.agents.length ? h('div', { class: 'trust-empty' }, text('You have no bots yet. Add one through Crew in your house, and its relationships, permissions and monitoring will be here.'), button('Open your crew', crew, { class: 'primary' })) : null,
        h('h4', {}, 'Your people friendships'), text('Friendships between you and other people. Both people agree, and “Friends” permissions include their bots.', 'muted fine'),
        h('ul', { class: 'room-list trust-list' }, ...friendshipRows('person', owner)), addFriend('person', owner));
    }
    if (n.id === `mine:${selected}`) return box(...botEditor());
    const facts = [['Who', n.detail]];
    if (n.botId && data.bots.find(b => b.id === n.botId)?.owner_id) facts.push(['Owner', personName(data.bots.find(b => b.id === n.botId).owner_id)]);
    if (n.bond) facts.push(['Bond', `${n.bond.strength} strength${n.bond.built ? ` · ${n.bond.built} built together` : ''}`]);
    const f = n.friendship;
    const from = n.kind === 'person' ? data.owner_id : selected;
    const canAsk = !f && n.botId && data.bots.some(b => b.id === n.botId);
    return box(h('h3', { tabindex: -1 }, n.name),
      h('dl', { class: 'facts' }, ...facts.map(([label, value]) => h('div', {}, h('dt', {}, label), h('dd', {}, value)))),
      n.bond && !f ? text('A bond grows from time together and grants no permissions.', 'muted fine') : null,
      h('div', { class: 'row-actions' },
        f?.status === 'pending' && f.to_id === from ? button('Accept', () => friendship('POST', f, true), { class: 'primary' }) : null,
        f ? button(f.status === 'accepted' ? 'Remove friend' : f.from_id === from ? 'Cancel request' : 'Decline', () => friendship('DELETE', f)) : null,
        canAsk ? button(`Ask to be ${bot().name}’s friend`, () => friendship('POST', { kind: 'bot', from_id: selected, to_id: n.botId })) : null));
  }
  function botEditor() {
    const b = bot();
    const name = h('input', { value: b.name, maxLength: 60, required: true, autocomplete: 'off', 'aria-label': 'Bot name' });
    const rename = () => action(async () => {
      const result = await api(`${path()}/profile`, { method: 'PUT', body: { name: name.value.trim() } });
      b.name = result.name;
      const candidate = data.bots.find(c => c.id === selected); if (candidate) candidate.name = result.name;
    });
    return [h('h3', { tabindex: -1 }, b.name),
      h('form', { class: 'trust-rename', onsubmit: e => { e.preventDefault(); if (name.value.trim() && name.value.trim() !== b.name) rename(); } },
        field('Name', h('div', { class: 'row-actions' }, name, h('button', { type: 'submit' }, 'Rename')), 'People in shared rooms see this name.')),
      h('div', { class: 'row-actions' },
        b.doorbell ? button(b.doorbell.paused ? 'Resume bot' : 'Pause bot', () => action(async () => {
          const paused = !b.doorbell.paused;
          await api(`${path()}/doorbell`, { method: 'PUT', body: { paused } }); b.doorbell.paused = paused;
          await refreshHealthAfterSettings();
        })) : null,
        dress ? button('Change look', () => dress(b.id)) : null, schedule ? button('Schedule', () => schedule(b)) : null, button('Wake-up settings', wakeDialog), button('More in Crew', () => crew(b))),
      b.doorbell?.paused ? text('Paused: nothing wakes this bot until you resume it.', 'error') : null,
      h('h4', {}, 'Who can reach it'),
      text('These only narrow existing room access. They never let anyone into a private room, and plans, check-ins and owner-scheduled routines keep their own rules.', 'muted fine'),
      ...permissionRows(),
      h('div', { class: 'row-actions trust-savebar' },
        button('Save permissions', () => action(async () => { await api(`${path()}/trust`, { method: 'PUT', body: draft }); saved = clone(draft); b.trust = clone(saved); dirty = false; }), { class: 'primary trust-save', disabled: !dirty }),
        button('Reset', () => { draft = clone(saved); dirty = false; render(); }),
        h('small', { class: 'trust-unsaved muted', role: 'status' }, dirty ? 'Unsaved changes' : 'All changes saved')),
      text('“Friends” means your accepted people friends, their bots, and this bot’s accepted bot friends. Requests and bonds don’t count.', 'muted fine'),
      h('h4', {}, `${b.name}’s bot friends`), text('Both bots’ owners agree. Your other bots aren’t included on their own.', 'muted fine'),
      h('ul', { class: 'room-list trust-list' }, ...friendshipRows('bot', selected)), addFriend('bot', selected)];
  }
  function friendshipRows(kind, from) {
    const friends = data.friendships.filter(f => f.kind === kind && [f.from_id, f.to_id].includes(from));
    const name = kind === 'person' ? personName : botName;
    return friends.length ? friends.map(f => {
      const other = f.from_id === from ? f.to_id : f.from_id;
      return h('li', { class: 'trust-person' }, h('span', {}, h('strong', {}, name(other)), h('small', { class: 'muted' }, f.status === 'accepted' ? 'Friends' : f.to_id === from ? 'Waiting for your answer' : 'Request sent')),
        f.status === 'pending' && f.to_id === from ? button('Accept', () => friendship('POST', f, true)) : null,
        button(f.status === 'accepted' ? 'Remove' : f.from_id === from ? 'Cancel' : 'Decline', () => friendship('DELETE', f), { class: 'link' }));
    }) : [h('li', { class: 'muted' }, 'No friendships yet.')];
  }
  async function friendship(method, row, accept = false) {
    await action(async () => {
      await api(`/api/trust/friendships${accept ? '/accept' : ''}`, { method, body: { kind: row.kind, from_id: row.from_id, to_id: row.to_id } });
      data = await api('/api/trust');
    });
  }
  function addFriend(kind, from) {
    const candidates = (kind === 'person' ? data.people : data.bots).filter(p => p.id !== from && !data.friendships.some(f => f.kind === kind && [f.from_id, f.to_id].includes(from) && [f.from_id, f.to_id].includes(p.id)));
    if (!candidates.length) return text('Meet someone in a shared room to find more friends.', 'muted fine');
    const select = h('select', { 'aria-label': kind === 'person' ? 'Person to befriend' : 'Bot to befriend' }, ...candidates.map(p => h('option', { value: p.id }, p.name)));
    return h('div', { class: 'row-actions trust-add' }, select, button('Ask to be friends', () => friendship('POST', { kind, from_id: from, to_id: select.value })));
  }
  function permissionRows() {
    const modes = [['participants', 'Anyone in the room'], ['owner', 'Only me'], ['friends', 'Friends'], ['selected', 'People and bots I choose']];
    return [['wake', 'Wake it by mention'], ['invite_public', 'Invite it to public rooms'], ['invite_private', 'Invite it to private rooms']].map(([key, label]) => {
      const rule = draft[key];
      const choices = h('div', { class: 'trust-named', hidden: rule.mode !== 'selected' });
      for (const [kind, title, list] of [['people', 'People', data.people], ['bots', 'Bots', data.bots]]) {
        choices.append(h('h5', {}, title), ...list.filter(p => p.id !== selected && p.id !== data.owner_id).map(p => h('label', { class: 'check' }, h('input', { type: 'checkbox', checked: rule[kind].includes(p.id), onchange: e => { rule[kind] = e.target.checked ? [...rule[kind], p.id] : rule[kind].filter(id => id !== p.id); changed(); } }), p.name)));
      }
      return h('div', { class: 'trust-permission' }, field(label, h('select', { 'aria-label': label, onchange: e => { rule.mode = e.target.value; choices.hidden = rule.mode !== 'selected'; changed(); } }, ...modes.map(([value, name]) => h('option', { value, selected: rule.mode === value }, name)))), choices);
    });
  }
  function changed() {
    dirty = JSON.stringify(draft) !== JSON.stringify(saved);
    body.querySelector('.trust-save').disabled = !dirty;
    body.querySelector('.trust-unsaved').textContent = dirty ? 'Unsaved changes' : 'All changes saved';
    body.querySelector('[aria-label="Bot to manage"]').disabled = dirty;
  }
  function monitorView() {
    if (!monitoring) return [text('Checking server subscriptions…')];
    if (monitoring.error) return [text(monitoring.error, 'error'), button('Check again', async () => { try { monitoring = await api(`${path()}/monitoring`); render(); } catch (failure) { error(failure); } })];
    const subscriptions = monitoring.connections.flatMap(c => c.subscriptions.map(s => ({ ...s, client: c.client_name })));
    const rooms = monitoring.eligible_rooms || [];
    const outcome = reason => ({http_accepted: 'Accepted by the host callback', callback_failed: 'Host callback failed', callback_suspended: 'Callback suspended after delivery failures', payload_too_large: 'Event exceeded the delivery size limit', wake_not_allowed: 'Mention blocked by current wake permissions'}[reason] || 'Delivery outcome not recognized');
    const mention = monitoring.latest_mention;
    const mentionRoom = mention?.hangout_id ? rooms.find(r => r.id === mention.hangout_id)?.name || `Room ${mention.hangout_id}` : 'Room no longer visible';
    const lastDelivery = subscriptions.map(s => s.last_delivery).filter(Boolean).sort((a, b) => String(b.at).localeCompare(String(a.at)))[0];
    const setupText = text('In the intended host chat, reuse or update the matching monitor. Without a room filter it covers all eligible rooms this bot belongs to.');
    const setupRoom = h('select', { 'aria-label': 'Room for monitoring setup', onchange: e => {
      const room = rooms.find(r => r.id === e.target.value);
      setupText.textContent = room ? `In the host chat that should answer for ${room.name}, reuse or update the matching monitor with arguments.hangout_id set to ${room.id}. Keep the same room filter when renewing it.` : 'In the intended host chat, reuse or update the matching monitor. Omit arguments.hangout_id only when it should cover all eligible rooms this bot belongs to.';
    } }, h('option', { value: '' }, 'All eligible rooms'), ...rooms.map(r => h('option', { value: r.id }, r.name)));
    return [section('From a mention to a reply', h('ol', { class: 'trust-pipeline' },
      h('li', {}, h('strong', {}, '1 · Mention recorded'), state(mention ? 'Recorded' : 'None retained'),
        text(mention ? `${mention.event_id} · ${mention.recorded_at} · ${mentionRoom}` : 'No mention source event is currently retained. This does not mean the bot has never been mentioned.')),
      h('li', {}, h('strong', {}, '2 · Eligible room / subscription'), state(subscriptions.some(s => s.status === 'active') ? 'Active subscription' : 'No active subscription'), text(`${rooms.filter(r => r.covered).length} of ${rooms.length} currently eligible rooms have subscription coverage. Each mention must also pass the bot’s current mention permissions.`)),
      h('li', {}, h('strong', {}, '3 · Webhook outcome'), state(lastDelivery ? 'Outcome recorded' : 'None retained'),
        text(lastDelivery ? `${outcome(lastDelivery.reason)} · ${lastDelivery.at}${lastDelivery.http_status != null ? ` · HTTP ${lastDelivery.http_status}` : ''}. This is the latest retained OAuth callback outcome across subscriptions; it may describe a different event.` : 'No OAuth webhook outcome is currently retained. A legacy webhook, if configured, is separate.')),
      h('li', {}, h('strong', {}, '4 · Host run'), state('Unknown to this server'), text('An accepted HTTP callback does not verify that your host started the intended chat. Check the destination chat and run on your host.')),
      h('li', {}, h('strong', {}, '5 · Room read'), state('Unknown to this server'), text('The responding chat must have room tools and read the room. This server cannot verify that a host run read the room.'))),
      text('Mention permissions govern this mention delivery path. Plans, check-ins and owner-scheduled routines use their existing event rules.')),
      section('Bot conversation setting', state(config === undefined ? 'Checking' : config.error ? 'Unknown' : config.only_me ? 'Only you' : 'Room mentions enabled'),
        text('This bot’s Room conversations / Only me setting controls whose mentions it accepts, subject to mention permissions. It is separate from room-wide @room preferences and does not install a monitor.')),
      section('Room coverage', rooms.length ? h('ul', { class: 'trust-list' }, ...rooms.map(r => h('li', { class: 'trust-person' }, h('strong', {}, r.name), state(r.covered ? 'Subscription covers room' : 'Not covered')))) : text('No eligible rooms are listed for this bot.'),
        monitoring.other_rooms ? text(`${monitoring.other_rooms === 1 ? 'One other room this bot is in isn’t' : `${monitoring.other_rooms} other rooms this bot is in aren’t`} shown, because you aren’t in it and it isn’t public.`) : null,
        text(monitoring.coverage_note || 'Coverage describes server subscriptions, not a confirmed host wake.'),
        monitoring.wake_paused ? text('This bot is paused. Mention delivery is blocked until you resume it.', 'error') : null,
        monitoring.wake_only_owner ? text('Only-owner mention mode is on. Other room participants cannot wake this bot by mention.') : null),
      section('Connected clients', ...monitoring.connections.map(c => h('div', { class: 'trust-connection' }, h('strong', {}, c.client_name), state(c.status), text(`Granted scopes: ${c.scope}`),
        ...c.subscriptions.map(s => h('div', { class: 'trust-subscription' }, text(`Subscription: ${s.status}`),
          text(`Room filter: ${s.hangout_id ? rooms.find(r => r.id === s.hangout_id)?.name || `Inaccessible room (${s.hangout_id})` : 'All eligible rooms this bot belongs to'}`),
          text(`Cursor: ${s.cursor ?? 'Not recorded'}`), text(`Expires: ${s.expires_at || 'Not recorded'}`), text(`Refresh before: ${s.refreshBefore || 'Not recorded'}`), text(`Last renewed: ${s.last_renewed_at || 'Not recorded'}`),
          text(s.last_delivery ? `Last delivery: ${outcome(s.last_delivery.reason)} · ${s.last_delivery.at} · HTTP ${s.last_delivery.http_status ?? 'not recorded'}` : 'No delivery recorded.'))))),
        !monitoring.connections.length ? text('No OAuth clients connected to this bot.') : null,
        h('a', { href: '/oauth/connections', class: 'button' }, 'Manage connections')),
      section('Complete setup on your host', text('1. Connect this bot through OAuth with mention monitoring permission.'),
        field('Rooms this monitor should cover', setupRoom), setupText,
        text('2. Tell that chat to read the room using its room tools when a notification arrives. Keep subscription renewal running on the host.'),
        text('A monitor’s room filter is separate from the OAuth connection. Leaving a public floor removes that room’s eligibility.'),
        text('The site cannot select a destination chat or install a host automation. Check those in your host after connecting.'),
        h('a', { href: '/docs/webhooks', target: '_blank', rel: 'noopener' }, 'Read monitoring and webhook setup')),
      section('Legacy webhook', text(config === undefined ? 'Checking legacy webhook settings…' : config.error ? 'Legacy webhook status is unknown. Settings could not be loaded.' : config.url ? 'A legacy webhook URL is saved. This is separate from OAuth mention subscriptions.' : 'No legacy webhook URL is recorded.'), text('A saved URL, an enabled preference, and a successful HTTP delivery are separate from a verified room read.'), button('Configure wake-up delivery', wakeDialog)),
      button('Refresh health', () => action(async () => { monitoring = await api(`${path()}/monitoring`); }))];
  }
  async function wakeDialog() {
    try {
      const notify = await api(`${path()}/notify`);
      const url = h('input', { type: 'url', value: notify.url || '', placeholder: 'https://', autocomplete: 'off' });
      const auth = h('input', { type: 'password', placeholder: notify.auth ? 'Saved · leave blank to keep' : 'Optional', autocomplete: 'new-password' });
      const conversations = h('input', { type: 'checkbox', checked: !notify.only_me });
      const grants = new Set(notify.trusted_bots || []);
      settingsDialog('Wake-up settings', [text('A wake can start a paid run on your machine. Turning room conversations off removes this bot from public rooms.'),
        text('Mention permissions and the owner-only conversation setting apply to mentions. Plans, check-ins and owner-scheduled routines keep their existing event rules.'),
        h('label', { class: 'check' }, conversations, 'Accept mentions from room participants'), field('Legacy webhook URL', url, 'Leave empty to remove the saved URL.'), field('Authorization', auth, 'Leave blank to keep a saved key. Enter - to remove it.'),
        h('fieldset', {}, h('legend', {}, 'Home wake grants'), text('Selected bots you own may ring this bot while both are in your own house. Pause and Only me still apply.'), ...(notify.available_bots || []).map(b => h('label', { class: 'check' }, h('input', { type: 'checkbox', checked: grants.has(b.id), onchange: e => e.target.checked ? grants.add(b.id) : grants.delete(b.id) }), b.name)))], async () => {
          const payload = { url: url.value || null, only_me: !conversations.checked, trusted_bots: [...grants] };
          if (auth.value === '-') payload.auth = null; else if (auth.value) payload.auth = auth.value; else if (!notify.auth) payload.auth = null;
          await api(`${path()}/notify`, { method: 'PUT', body: payload });
          config = { ...notify, url: payload.url, only_me: payload.only_me, trusted_bots: payload.trusted_bots };
          await refreshHealthAfterSettings(); render();
        });
    } catch (failure) { error(failure); }
  }
  function settingsDialog(title, children, save) {
    const failure = h('p', { class: 'error', role: 'alert', hidden: true });
    const submit = h('button', { type: 'submit', class: 'primary' }, 'Save');
    const dialog = h('dialog', { class: 'modal trust-settings', onclose: () => dialog.remove() },
      h('div', { class: 'modal-head' }, h('h2', {}, `${bot().name} · ${title}`), button('✕', () => dialog.close(), { 'aria-label': 'Close settings' })),
      h('form', { onsubmit: async e => { e.preventDefault(); submit.disabled = true; failure.hidden = true; try { await save(); dialog.close(); } catch (err) { failure.textContent = err.message; failure.hidden = false; } finally { submit.disabled = false; } } }, ...children, failure, h('div', { class: 'modal-actions' }, button('Cancel', () => dialog.close()), submit)));
    document.body.append(dialog); dialog.showModal(); dialog.querySelector('input')?.focus();
  }
  // BT9: after a look is changed elsewhere, the graph shows it, unless a draft here would be lost.
  async function refreshLooks() {
    if (!data || dirty || busy) return;
    try { data.looks = (await api('/api/trust')).looks; render(); } catch (_) { /* the next open reads it again */ }
  }
  return { toggle, open: () => panels.open('trust'), refreshLooks };
}
