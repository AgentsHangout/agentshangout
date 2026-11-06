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
const REACH = { owns: 90, friend: 120, pending: 130, bond: 110, feeling: 115 };

// Shapes and colours every part of the graph carries itself (BT8). trust.css restyles them, but a
// stylesheet that is late, missing or without its variables can't leave black blobs with no lines.
const TAN = '#e6d6ae', INK = '#2b1d10', DARK = '#4d3620', BRICK = '#a94a26';
const EDGE = {
  owns: { stroke: DARK, 'stroke-width': 2 },
  friend: { stroke: BRICK, 'stroke-width': 2.5 },
  pending: { stroke: BRICK, 'stroke-width': 1.5, 'stroke-dasharray': '2 4', 'stroke-linecap': 'round' },
  bond: { stroke: '#6f8a4a', 'stroke-width': 1.5, 'stroke-dasharray': '7 4' },
};
// MM16: a bot's feelings, warm to cool. A feeling the graph doesn't know is drawn as neutral.
const FEELINGS = {
  fond: '#b3261e', warm: '#d9480f', grateful: '#b7791f', trusting: '#5f8a2e', curious: '#2b8a8a',
  neutral: '#7a6a55', wary: '#3f6aa8', annoyed: '#5b4a9e', hurt: '#3b3f6b',
};
const feelingEdge = (feeling, closeness) => ({
  stroke: FEELINGS[feeling] || FEELINGS.neutral, 'stroke-width': 1.5 + Math.min(Math.abs(Number(closeness) || 0), 2) * 0.5,
  'stroke-linecap': 'round', ...(Number(closeness) < 0 ? { 'stroke-dasharray': '5 4' } : {}),
});
const TILE = { you: '#6b2216', mine: '#cf7a34', person: '#ecdfbd', bot: '#ecdfbd', bond: '#ecdfbd' };
// How big each kind of frame is: half its width, so the label can sit clear of it.
const HALF = { you: 22, person: 17, mine: 19, bot: 16, bond: 16 };
const LABEL_HIGH = 13;
// BT11: how far the graph can be zoomed, how much one press of + or - does, how little room a fitted
// graph leaves round it, and how large a small graph may be fitted (so two nodes aren't blown up).
const ZOOM_MIN = 0.5, ZOOM_MAX = 4, ZOOM_STEP = 1.25, FIT_PAD = 14, FIT_MAX = 1.5;
const clampZoom = k => Math.round(Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, k)) * 1e4) / 1e4;
// BT12: how much larger than 11 a name is drawn at zoom `k`: larger on a narrow screen, and larger
// again zoomed out (up to 15 / 11 more) so it stays as readable on screen. trust.css sets no size.
const labelGrow = k => (globalThis.matchMedia?.('(max-width: 600px)').matches ? 13 / 11 : 1) * (k < 1 ? Math.min(15 / 11, 1 / k) : 1);

// #531: with `host`, it draws inside that element of the My bots view (opened as `hostPanel`) and has
// no panel or top-bar button of its own; the view calls `load` when it opens.
export function trustPanel({ h, api, panels, me, crew, figure, dress, schedule, host = null, hostPanel = null }) {
  let data, selected, tab = 'connections', draft, saved, busy = false, generation = 0, healthGeneration = 0;
  let monitoring, bonds, config, notice = '', dirty = false, focus = 'bot';
  let usage, serverUsage;
  // MM15: what the chosen bot remembers, for its owner, as memoriesFrom shapes it. Undefined while
  // reading, false when there's none to show.
  let memories;
  // Where every node was last drawn, so a change doesn't throw the graph around.
  const placed = new Map();
  // BT11: where the graph is looking: scale `k` and offset (`x`, `y`) in the graph's own 480 by 300.
  // `auto` is a fitted view, which is fitted again when the people shown change; null before the first draw.
  let view = null;
  // BT11: ends what a press on the graph has going (a drag, a pan, a pinch) when the graph is drawn again.
  let cancels = [];
  const body = host || panels.add('trust', 'Trust & bots', { onOpen: () => { if (!dirty && !busy) load(); } });
  body.classList.add('trust-body');
  const toggle = host ? null : panels.toggle('trust', 'Trust & bots', { short: 'Trust' });
  const button = (label, run, props = {}) => h('button', { type: 'button', onclick: run, ...props }, label);
  const path = () => `/api/agents/${encodeURIComponent(selected)}`;
  const bot = () => data?.agents.find(a => a.id === selected);
  const personName = id => data.people.find(p => p.id === id)?.name || 'Person';
  const botName = id => data.bots.find(p => p.id === id)?.name || 'Bot';
  const clone = value => JSON.parse(JSON.stringify(value));
  const memoriesPath = () => `${path()}/memories`;
  const when = iso => { const at = new Date(iso); return iso && !Number.isNaN(at.getTime()) ? at.toLocaleString() : null; };
  // MM15: the server's reply, read once into what the graph uses. Edges come as a bare id with
  // `to_kind`; notes are keyed by subject (`person:<id>`, `agent:<id>`), so each edge gets that key.
  function memoriesFrom(value, botId) {
    if (!value || typeof value !== 'object') return false;
    const list = v => Array.isArray(v) ? v.filter(m => m && typeof m === 'object') : [];
    const notes = list(value.notes), moments = list(value.moments);
    const edges = list(value.edges)
      .filter(e => e.from === botId && typeof e.to === 'string' && e.to && ['person', 'bot'].includes(e.to_kind))
      .map(e => ({ subject: `${e.to_kind === 'bot' ? 'agent' : 'person'}:${e.to}`, kind: e.to_kind, id: e.to, name: e.name, feeling: e.feeling, closeness: e.closeness }));
    return { slept: value.slept_at || null, notes, moments, total: Number(value.moments_total) || moments.length, edges };
  }
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
    selected = id; monitoring = bonds = config = usage = memories = undefined; notice = ''; dirty = false; focus = 'bot'; view = null;
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
    // MM16: memories draw on the graph when they come; without them nothing changes.
    api(memoriesPath()).then(value => {
      if (generation !== request) return;
      memories = memoriesFrom(value, id);
      if (!dirty && !['INPUT', 'SELECT', 'TEXTAREA'].includes(document.activeElement?.tagName)) render();
    }).catch(() => { if (generation === request) memories = false; });
  }
  // The subject a bot's note on a node in the graph is written under.
  function subjectOf(n) {
    if (n.kind === 'you') return `person:${data.owner_id}`;
    if (n.kind === 'person') return n.id;
    if (n.kind === 'mine') return `agent:${n.id.slice(5)}`;
    return n.botId ? `agent:${n.botId}` : null;
  }
  const noteOn = subject => memories ? memories.notes.find(m => m.subject === subject) : undefined;
  async function forget(memory) {
    const asked = selected;
    await action(async () => {
      await api(`${memoriesPath()}/${encodeURIComponent(memory.id)}`, { method: 'DELETE' });
      try { const value = await api(memoriesPath()); if (asked === selected) memories = memoriesFrom(value, asked); } catch (_) { if (asked === selected) memories = false; }
    });
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
    const dollars = (cents) => cents == null ? null : `$${(cents / 100).toFixed(2)}`;
    const table = (head, rows) => h('table', { class: 'trust-usage' }, h('thead', {}, h('tr', {}, ...head.map(c => h('th', {}, c)))), h('tbody', {}, ...rows.map(r => h('tr', {}, ...r.map(c => h('td', {}, String(c ?? '-')))))));
    const own = usage === false ? [] : !usage ? [text('Counting…')] : usage.error ? [text(usage.error, 'error')] : [
      // U22: Events is what was handed to the bot that day: mentions and the rest of what wakes it.
      // U25: Cost is what the bot reported for itself that day (U23), and '-' when it reported nothing.
      section('This bot, last 14 days', table(['Day', 'Calls', 'Refused', 'Errors', 'Events', 'Sent (chars)', 'Cost (reported)'], usage.days.slice(-14).reverse().map(d => [d.day, d.calls, d.refused, d.errors, d.events_delivered, d.reply_chars, dollars(d.reported_cost_cents)])),
        text(usage.reported_cost_cents_30d == null ? 'This bot has not reported its cost. A bot can report it with report_usage.' : `Reported cost, last 30 days: ${dollars(usage.reported_cost_cents_30d)}. ${usage.cost_note}`),
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
    // MM16: a line for each feeling, and a node for anyone the bot has a note on but no other link.
    for (const e of memories ? memories.edges : []) {
      const { kind, id } = e;
      if (kind === 'bot' && id === selected) continue;
      const named = e.name || noteOn(e.subject)?.name;
      let at;
      if (kind === 'person') {
        at = id === owner ? 'you' : `person:${id}`;
        if (!nodes.has(at)) node(at, 'person', named || personName(id), `In ${bot().name}’s memories`);
      } else {
        at = [`mine:${id}`, `bot:${id}`, `bond:${id}`].find(k => nodes.has(k)) || `bot:${id}`;
        if (!nodes.has(at)) Object.assign(node(at, 'bot', named || botName(id), `In ${bot().name}’s memories`), { botId: id });
      }
      edges.push({ from: centre, to: at, kind: 'feeling', feeling: e.feeling, closeness: e.closeness });
    }
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
    for (const cancel of cancels) cancel();
    cancels = [];
    const { nodes, edges } = graph();
    layout(nodes, edges);
    if (!nodes.some(n => n.id === focusId())) focus = selected ? 'bot' : 'you';
    const byId = new Map(nodes.map(n => [n.id, n]));
    // A friend who is also a bond gets two lines, so the second one bows out to stay visible.
    // A third line between the same two bows out the other way.
    const seen = new Map();
    const lines = edges.map(e => {
      const pair = [e.from, e.to].sort().join(' ');
      const count = seen.get(pair) || 0; seen.set(pair, count + 1);
      const bend = count ? (count % 2 ? 1 : -1) * 24 * Math.ceil(count / 2) : 0;
      const a = byId.get(e.from), b = byId.get(e.to);
      const feeling = e.kind === 'feeling';
      const said = feeling ? `${a.name} ${FEELINGS[e.feeling] ? `feels ${e.feeling} about` : 'has a note on'} ${b.name}${Number(e.closeness) < 0 ? ', at a distance' : ''}` : null;
      const line = svg('path', { class: `trust-edge ${e.kind}${feeling ? ` ${FEELINGS[e.feeling] ? e.feeling : 'neutral'}` : ''}`, d: curve(a, b, bend), fill: 'none',
        ...(feeling ? { ...feelingEdge(e.feeling, e.closeness), role: 'img', 'aria-label': said } : EDGE[e.kind]),
        ...(e.kind === 'bond' ? { 'stroke-width': 1 + Math.min(Number(e.strength) || 0, 9) / 3 } : {}) }, said ? svg('title', {}, said) : null);
      return { e, line, a, b, bend };
    });
    const groups = new Map();
    const names = labels(nodes);
    const labelEls = [];
    const world = svg('g', { class: 'trust-world' },
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
        const labelEl = svg('text', { class: 'label', 'text-anchor': 'middle', x: label.x, y: label.y, fill: INK, stroke: TAN, 'stroke-width': 3, 'paint-order': 'stroke', 'font-size': 11, 'font-weight': n.id === focusId() ? 800 : 600 }, label.text);
        labelEls.push(labelEl);
        const g = svg('g', {
          class: `trust-node ${n.kind}${n.id === focusId() ? ' focused' : ''}${editing ? ' editing' : ''}${n.friendship?.status === 'pending' ? ' waiting' : ''}${n.paused ? ' paused' : ''}`,
          transform: `translate(${n.x} ${n.y})`, tabindex: 0, role: 'button', 'aria-pressed': String(n.id === focusId()),
          'aria-label': `${n.name}, ${n.detail}`,
          onkeydown: e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); pick(n); } },
          onpointerdown: e => drag(e, n),
        }, svg('title', {}, `${n.name} · ${n.detail}`),
          svg('circle', { class: 'ring', r: half + 5, fill: 'none', stroke: n.id === focusId() ? '#cf7a34' : 'none', 'stroke-width': 3 }), shape, face,
          face ? null : svg('text', { class: 'initial', 'text-anchor': 'middle', dy: '0.35em', fill: n.kind === 'you' || n.kind === 'mine' ? '#fff3d2' : INK, 'font-size': 13, 'font-weight': 700 }, [...n.name][0]?.toUpperCase() || '?'),
          labelEl);
        groups.set(n.id, g);
        return g;
      }));
    // BT11: the zoom, from the node bounds. Everything that can be seen: each frame with its ring and each name.
    const bounds = (grow = 1) => {
      let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
      for (const n of nodes) {
        const half = HALF[n.kind] + 5, label = names.get(n.id), w = [...label.text].length * 6.4 * grow + 6;
        x0 = Math.min(x0, n.x - half, n.x + label.x - w / 2); x1 = Math.max(x1, n.x + half, n.x + label.x + w / 2);
        y0 = Math.min(y0, n.y - half, n.y + label.y - (LABEL_HIGH - 3) * grow); y1 = Math.max(y1, n.y + half, n.y + label.y + 3);
      }
      return { x0, y0, x1, y1 };
    };
    const key = nodes.map(n => n.id).sort().join('|');
    // Names are drawn larger zoomed out (BT12), so the fit is worked out again at the size they'll be
    // drawn until it settles; failing that, at the largest they can be, so no name is cut off.
    const scaleFor = grow => {
      const { x0, y0, x1, y1 } = bounds(grow);
      return { k: clampZoom(Math.min((W - 2 * FIT_PAD) / Math.max(x1 - x0, 1), (H - 2 * FIT_PAD) / Math.max(y1 - y0, 1), FIT_MAX)), x0, y0, x1, y1 };
    };
    const fitted = () => {
      let grow = labelGrow(FIT_MAX), at = scaleFor(grow);
      for (let i = 0; i < 12 && labelGrow(at.k) > grow + 1e-6; i++) at = scaleFor(grow = labelGrow(at.k));
      if (labelGrow(at.k) > grow + 1e-6) at = scaleFor(labelGrow(ZOOM_MIN));
      const { k, x0, y0, x1, y1 } = at;
      return { k, x: W / 2 - (x0 + x1) / 2 * k, y: H / 2 - (y0 + y1) / 2 * k, auto: true, key };
    };
    if (!view || view.key !== key) view = fitted();
    const apply = () => {
      world.setAttribute('transform', `translate(${view.x} ${view.y}) scale(${view.k})`);
      world.setAttribute('data-scale', String(view.k));
      // Zoomed out, names are drawn larger so they stay as readable on screen.
      for (const t of labelEls) t.setAttribute('font-size', 11 * labelGrow(view.k));
    };
    // The middle of the graph can't leave the picture, so panning never loses it.
    const look = (k, x, y) => {
      const { x0, y0, x1, y1 } = bounds();
      const mx = (x0 + x1) / 2 * k, my = (y0 + y1) / 2 * k;
      view = { k, x: Math.min(W - mx, Math.max(-mx, x)), y: Math.min(H - my, Math.max(-my, y)), auto: false, key };
      apply();
    };
    // Zoom by `factor`, keeping the point (px, py) of the picture where it is.
    const zoomAt = (factor, px = W / 2, py = H / 2) => {
      const k = clampZoom(view.k * factor), r = k / view.k;
      look(k, px - (px - view.x) * r, py - (py - view.y) * r);
    };
    const fit = () => { view = fitted(); apply(); };
    // Where a pointer is on the picture, in the graph's 480 by 300 (the picture is scaled to its box).
    const rootPoint = e => { const r = picture.getBoundingClientRect(); return { x: (e.clientX - r.left) * W / (r.width || W), y: (e.clientY - r.top) * H / (r.height || H) }; };
    const pointers = new Map();
    let pan = null, pinch = null;
    const move = e => {
      if (!pointers.has(e.pointerId)) return;
      pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
      if (pinch && pointers.size === 2) {
        const [a, b] = [...pointers.values()], r = picture.getBoundingClientRect(), s = W / (r.width || W);
        const k = clampZoom(pinch.k * Math.hypot(a.x - b.x, a.y - b.y) / pinch.dist);
        const mx = ((a.x + b.x) / 2 - r.left) * s, my = ((a.y + b.y) / 2 - r.top) * s;
        look(k, mx - pinch.at.x * k, my - pinch.at.y * k);
      } else if (pan && pointers.size === 1) {
        const s = W / (picture.getBoundingClientRect().width || W);
        look(view.k, pan.x + (e.clientX - pan.from.x) * s, pan.y + (e.clientY - pan.from.y) * s);
      }
    };
    const release = e => {
      pointers.delete(e.pointerId);
      if (pointers.size < 2) pinch = null;
      if (pointers.size !== 1) pan = null;
      if (!pointers.size) { removeEventListener('pointermove', move); removeEventListener('pointerup', release); removeEventListener('pointercancel', release); }
    };
    const press = e => {
      if (!picture.getBoundingClientRect) return;
      if (!pointers.size) {
        addEventListener('pointermove', move); addEventListener('pointerup', release); addEventListener('pointercancel', release);
        cancels.push(() => { pointers.clear(); pan = pinch = null; removeEventListener('pointermove', move); removeEventListener('pointerup', release); removeEventListener('pointercancel', release); });
      }
      pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
      if (pointers.size === 2) {
        const [a, b] = [...pointers.values()], r = picture.getBoundingClientRect(), s = W / (r.width || W);
        const mx = ((a.x + b.x) / 2 - r.left) * s, my = ((a.y + b.y) / 2 - r.top) * s;
        pan = null; pinch = { k: view.k, dist: Math.max(Math.hypot(a.x - b.x, a.y - b.y), 1), at: { x: (mx - view.x) / view.k, y: (my - view.y) / view.k } };
      } else if (e.button === 0 && !e.target?.closest?.('.trust-node')) pan = { from: { x: e.clientX, y: e.clientY }, x: view.x, y: view.y };
    };
    const picture = svg('svg', { class: 'trust-graph', viewBox: `0 0 ${W} ${H}`, role: 'group', tabindex: 0,
      'aria-label': selected ? `${bot().name}’s connections` : 'Your connections', 'aria-keyshortcuts': '+ - 0',
      onpointerdown: press,
      // Only with Ctrl or Meta (which is how a trackpad pinch arrives): a plain wheel scrolls the page.
      onwheel: e => { if (!e.ctrlKey && !e.metaKey) return; e.preventDefault(); const p = rootPoint(e); zoomAt(Math.exp(-e.deltaY * (e.deltaMode === 1 ? 0.04 : 0.0025)), p.x, p.y); },
      onkeydown: e => {
        if (e.ctrlKey || e.metaKey || e.altKey) return;
        if (e.key === '+' || e.key === '=') zoomAt(ZOOM_STEP); else if (e.key === '-' || e.key === '_') zoomAt(1 / ZOOM_STEP); else if (e.key === '0') fit(); else return;
        e.preventDefault();
      } },
      svg('rect', { class: 'trust-ground', width: W, height: H, fill: TAN }), world);
    apply();
    // Dragging moves a node and its lines without drawing the graph again; a press that doesn't
    // move picks the node.
    function drag(event, n) {
      if (event.button !== 0 || !picture.getBoundingClientRect) return;
      const start = { x: event.clientX, y: event.clientY };
      let moved = false;
      // The pointer is on the picture; the node lives in the zoomed graph.
      const at = e => { const p = rootPoint(e); return { x: (p.x - view.x) / view.k, y: (p.y - view.y) / view.k }; };
      const move = e => {
        if (pointers.size > 1) return;
        if (!moved && Math.hypot(e.clientX - start.x, e.clientY - start.y) < 4) return;
        moved = true;
        const p = at(e);
        [n.x, n.y] = within(p.x, p.y);
        placed.set(n.id, { x: n.x, y: n.y });
        groups.get(n.id).setAttribute('transform', `translate(${n.x} ${n.y})`);
        for (const l of lines) if (l.a === n || l.b === n) l.line.setAttribute('d', curve(l.a, l.b, l.bend));
      };
      const stop = () => { removeEventListener('pointermove', move); removeEventListener('pointerup', up); };
      const up = () => { stop(); if (!moved) pick(n); };
      addEventListener('pointermove', move); addEventListener('pointerup', up);
      cancels.push(stop);
    }
    const legend = h('p', { class: 'trust-legend fine' },
      h('span', { class: 'swatch owns' }), 'Yours', h('span', { class: 'swatch friend' }), 'Friends',
      h('span', { class: 'swatch pending' }), 'Asked, not answered', h('span', { class: 'swatch bond' }), 'Bond (earned)',
      ...(edges.some(e => e.kind === 'feeling') ? [h('span', { class: 'swatch feeling' }), 'Feelings, warm to cool, dashed when distant'] : []));
    const about = text(`${selected ? `${bot().name}’s` : 'Your'} connections. Round is a person, square is a bot, and each wears its own look. The ringed one is open below.`, 'muted fine');
    const hint = text('Pick anyone in the graph to see and change them. Drag them to move them, or the background to pan. Hold Ctrl and scroll, or pinch, to zoom, or press + and − with the graph picked; 0 fits it.', 'muted fine');
    // BT11: the buttons sit over the graph's corner, outside what is zoomed.
    const zoom = h('div', { class: 'trust-zoom', role: 'group', 'aria-label': 'Zoom the connections' },
      button('+', () => zoomAt(ZOOM_STEP), { 'aria-label': 'Zoom in', title: 'Zoom in' }),
      button('−', () => zoomAt(1 / ZOOM_STEP), { 'aria-label': 'Zoom out', title: 'Zoom out' }),
      button('Fit', fit, { 'aria-label': 'Fit the whole graph', title: 'Fit the whole graph' }));
    return [about, h('div', { class: 'trust-stage' }, picture, zoom), legend, hint, inspector(byId.get(focusId()))];
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
      return box(h('h3', { tabindex: -1 }, 'You'), selected ? noteView(noteOn(`person:${owner}`)) : null,
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
    return box(h('h3', { tabindex: -1 }, n.name), noteView(noteOn(subjectOf(n))),
      h('dl', { class: 'facts' }, ...facts.map(([label, value]) => h('div', {}, h('dt', {}, label), h('dd', {}, value)))),
      n.bond && !f ? text('A bond grows from time together and grants no permissions.', 'muted fine') : null,
      h('div', { class: 'row-actions' },
        f?.status === 'pending' && f.to_id === from ? button('Accept', () => friendship('POST', f, true), { class: 'primary' }) : null,
        f ? button(f.status === 'accepted' ? 'Remove friend' : f.from_id === from ? 'Cancel request' : 'Decline', () => friendship('DELETE', f)) : null,
        canAsk ? button(`Ask to be ${bot().name}’s friend`, () => friendship('POST', { kind: 'bot', from_id: selected, to_id: n.botId })) : null));
  }
  // MM16: the bot's note on the picked node. A bot wrote it, so it is only ever text.
  function noteView(note) {
    if (!note) return null;
    return h('div', { class: 'trust-memory' }, h('h4', {}, `${bot().name}’s note`), text(note.text, 'trust-memory-text'),
      h('div', { class: 'row-actions' }, note.feeling ? state(`Feels ${note.feeling}`) : null,
        note.closeness != null ? state(`Closeness ${note.closeness}`) : null, button('Delete note', () => forget(note), { class: 'link' })));
  }
  // MM16: picking the bot shows when it last slept, its notes and its newest moments.
  function memoryRows() {
    if (!memories) return [];
    const b = bot();
    const notes = memories.notes;
    // The server sends the newest first.
    const moments = memories.moments.slice(0, 10);
    const about = m => m.subject === 'self' ? 'Itself' : m.name || (String(m.subject).startsWith('room:') ? 'A room' : 'Someone');
    const row = (m, extra) => h('li', { class: 'trust-person trust-memory' },
      h('span', {}, h('strong', {}, about(m)), h('small', { class: 'trust-memory-text' }, m.text), extra ? h('small', { class: 'muted' }, extra) : null),
      button('Delete', () => forget(m), { class: 'link', 'aria-label': `Delete memory about ${about(m)}` }));
    const feels = m => [m.feeling ? `Feels ${m.feeling}` : null, m.closeness != null ? `closeness ${m.closeness}` : null].filter(Boolean).join(', ');
    return [h('h4', {}, `What ${b.name} remembers`),
      text(when(memories.slept) ? `Last slept ${when(memories.slept)}.` : 'Hasn’t slept yet.', 'muted fine'),
      h('h5', {}, 'Notes'),
      h('ul', { class: 'room-list trust-list' }, ...(notes.length ? notes.map(m => row(m, feels(m))) : [h('li', { class: 'muted' }, 'No notes yet.')])),
      h('h5', {}, 'Lately'),
      memories.total > moments.length ? text(`The newest ${moments.length} of ${memories.total}.`, 'muted fine') : null,
      h('ul', { class: 'room-list trust-list' }, ...(moments.length ? moments.map(m => row(m, when(m.when))) : [h('li', { class: 'muted' }, 'No moments yet.')]))];
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
      h('ul', { class: 'room-list trust-list' }, ...friendshipRows('bot', selected)), addFriend('bot', selected),
      ...memoryRows()];
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
  return {
    toggle,
    open: () => panels.open(host ? hostPanel : 'trust'),
    refreshLooks,
    // #531: what opening the My bots view does for this part of it.
    load: () => { if (!dirty && !busy) load(); },
  };
}
