// Owner-controlled relationships and permissions. All names are rendered as text.
export function trustPanel({ h, api, panels, me, crew }) {
  let data, selected, tab = 'overview', draft, saved, busy = false, generation = 0, healthGeneration = 0;
  let monitoring, bonds, config, notice = '', dirty = false;
  const body = panels.add('trust', 'Trust & bots', { onOpen: () => { if (!dirty && !busy) load(); } });
  body.classList.add('trust-body');
  const toggle = panels.toggle('trust', 'Trust & bots', { short: 'Trust' });
  const button = (label, run, props = {}) => h('button', { type: 'button', onclick: run, ...props }, label);
  const path = () => `/api/agents/${encodeURIComponent(selected)}`;
  const bot = () => data?.agents.find(a => a.id === selected);
  const personName = id => data.people.find(p => p.id === id)?.name || 'Person';
  const botName = id => data.bots.find(p => p.id === id)?.name || 'Bot';
  const clone = value => JSON.parse(JSON.stringify(value));
  const text = (words, className = 'muted') => h('p', { class: className }, words);
  const card = (title, ...children) => h('section', { class: 'trust-card' }, h('h3', {}, title), ...children);
  const state = words => h('span', { class: 'trust-tag' }, words);
  const field = (label, control, hint) => h('label', { class: 'trust-field' }, h('span', {}, label), control, hint ? h('small', { class: 'muted' }, hint) : null);
  const error = failure => { notice = failure.message || String(failure); render(); };
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
      if (selected) await choose(selected); else render();
    } catch (failure) {
      body.replaceChildren(text(failure.message, 'error'), button('Try again', load));
    }
  }
  async function choose(id) {
    selected = id; monitoring = bonds = config = undefined; notice = ''; dirty = false;
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
        if (!dirty && !body.contains(document.activeElement)) render();
        else if (!dirty && !['INPUT', 'SELECT', 'TEXTAREA'].includes(document.activeElement?.tagName)) render();
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
    if (!data.agents.length) {
      const owner = data.owner_id;
      body.replaceChildren(h('div', { class: 'trust-empty' }, h('span', { class: 'trust-emblem', 'aria-hidden': 'true' }, '✦'),
        h('h3', {}, 'A home for your bots'), text('Add a bot through Crew in your house. Its relationships, permissions and monitoring will live here.'), button('Open your crew', crew, { class: 'primary' })));
      body.append(h('div', { class: 'trust-content' }, card('Your people friendships', text('Friendships belong to people too. Both people must agree.'), h('ul', { class: 'trust-list' }, ...friendshipRows('person', owner)), addFriend('person', owner))));
      return;
    }
    const chooser = h('select', { 'aria-label': 'Bot to manage', disabled: busy || dirty, onchange: e => choose(e.target.value) },
      ...data.agents.map(a => h('option', { value: a.id, selected: a.id === selected }, a.name)));
    const tabs = [['overview', 'Overview'], ['relationships', 'Relationships'], ['permissions', 'Permissions'], ['monitoring', 'Monitoring']];
    const nav = h('div', { class: 'trust-tabs', role: 'tablist', 'aria-label': 'Bot settings' }, ...tabs.map(([id, label]) => button(label, () => { tab = id; notice = ''; render(); }, {
      role: 'tab', id: `trust-tab-${id}`, 'aria-selected': String(tab === id), 'aria-controls': 'trust-content', tabindex: tab === id ? 0 : -1,
      onkeydown: e => {
        if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(e.key)) return;
        e.preventDefault(); const i = tabs.findIndex(t => t[0] === tab);
        tab = tabs[e.key === 'Home' ? 0 : e.key === 'End' ? tabs.length - 1 : (i + (e.key === 'ArrowRight' ? 1 : tabs.length - 1)) % tabs.length][0];
        render(); body.querySelector(`#trust-tab-${tab}`)?.focus();
      },
    })));
    body.replaceChildren(h('div', { class: 'trust-intro' }, text('YOUR CREW, YOUR CALL', 'trust-eyebrow'),
      h('div', { class: 'trust-heading' }, h('span', { class: 'trust-emblem', 'aria-hidden': 'true' }, '✦'), field('Managing', chooser)),
      text('Choose who can reach your bot, and see what is connected.')),
      nav, ...(notice ? [h('p', { class: 'error trust-notice', role: 'alert' }, notice)] : []),
      h('div', { id: 'trust-content', class: 'trust-content', role: 'tabpanel', 'aria-labelledby': `trust-tab-${tab}` },
        ...({ overview, relationships, permissions, monitoring: monitorView }[tab])()));
    body.querySelectorAll('button').forEach(b => { if (busy) b.disabled = true; });
  }
  function overview() {
    const accepted = data.friendships.filter(f => f.status === 'accepted' && ((f.kind === 'person' && [f.from_id, f.to_id].includes(data.owner_id)) || (f.kind === 'bot' && [f.from_id, f.to_id].includes(selected))));
    return [h('div', { class: 'trust-stats' }, ...[[accepted.length, 'Friendships'], [Array.isArray(bonds) ? bonds.length : '—', 'Earned bonds'], [monitoring?.eligible_rooms?.filter(r => r.covered).length ?? '—', 'Rooms covered']].map(([n, label]) => h('div', {}, h('strong', {}, String(n)), h('small', {}, label)))),
      card('A circle you choose', text('Friendships are explicit and need both owners to agree. Bonds grow from doing things together. A bond never grants permission.'), button('Explore relationships', () => { tab = 'relationships'; render(); }, { class: 'link' })),
      card('Bot settings', text('Control public outings, social hours and delivery without leaving the map.'),
        h('div', { class: 'trust-actions' }, button('Rename', renameDialog), button('Social schedule', scheduleDialog), button('Wake-up settings', wakeDialog),
          bot().doorbell ? button(bot().doorbell.paused ? 'Resume bot' : 'Pause bot', () => action(async () => {
            const paused = !bot().doorbell.paused;
            await api(`${path()}/doorbell`, { method: 'PUT', body: { paused } }); bot().doorbell.paused = paused;
            await refreshHealthAfterSettings();
          })) : null,
          button('More bot controls', () => crew(bot()))),
        text('Crew also holds the bot’s appearance, room membership and account controls.')),
      card('Permission snapshot', ...permissionRows(false)),
      text('Waking a bot can start a run on your machine. The bot’s host decides how that run works.')];
  }
  function friendshipRows(kind, from) {
    const friends = data.friendships.filter(f => f.kind === kind && [f.from_id, f.to_id].includes(from));
    const name = kind === 'person' ? personName : botName;
    return friends.length ? friends.map(f => {
      const other = f.from_id === from ? f.to_id : f.from_id;
      return h('li', { class: 'trust-person' }, h('span', {}, h('strong', {}, name(other)), h('small', { class: 'muted' }, f.status === 'accepted' ? 'Mutual friendship' : f.to_id === from ? 'Waiting for your answer' : 'Request sent')),
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
    if (!candidates.length) return text('Meet someone in a shared room to find more friends.');
    const select = h('select', { 'aria-label': kind === 'person' ? 'Person to befriend' : 'Bot to befriend' }, ...candidates.map(p => h('option', { value: p.id }, p.name)));
    return h('div', { class: 'trust-add' }, select, button('Ask to be friends', () => friendship('POST', { kind, from_id: from, to_id: select.value })));
  }
  function relationships() {
    const owner = data.owner_id;
    const people = data.friendships.filter(f => f.kind === 'person' && f.status === 'accepted' && [f.from_id, f.to_id].includes(owner));
    const bots = data.friendships.filter(f => f.kind === 'bot' && f.status === 'accepted' && [f.from_id, f.to_id].includes(selected));
    const other = (f, id) => f.from_id === id ? f.to_id : f.from_id;
    const group = (title, names, type) => h('div', { class: `trust-constellation-group ${type}` }, h('small', {}, title), ...names.slice(0, 3).map(n => h('span', {}, n)), names.length > 3 ? h('small', {}, `+${names.length - 3} more below`) : null, !names.length ? h('span', { class: 'muted' }, 'None yet') : null);
    return [h('div', { class: 'trust-constellation', 'aria-label': `${bot().name} relationship map` },
      h('div', { class: 'trust-owner' }, h('small', {}, 'OWNER'), h('strong', {}, me.name || personName(owner))),
      h('div', { class: 'trust-orbit' }, group('People friends', people.map(f => personName(other(f, owner))), 'people'),
        h('div', { class: 'trust-center' }, h('span', { 'aria-hidden': 'true' }, '✦'), h('strong', {}, bot().name), h('small', {}, 'Your bot')),
        group('Bot friends', bots.map(f => botName(other(f, selected))), 'bots')),
      group('Earned bonds · no permissions', Array.isArray(bonds) ? bonds.map(b => `${b.agent?.name || 'A bot you haven’t met'} · ${b.strength}`) : [bonds === undefined ? 'Loading…' : 'Unavailable'], 'bonds')),
      text('Solid connections are agreed friendships. Dashed connections are earned bonds. The lists below show every connection and its controls.'),
      card('Your people friendships', text('These are friendships between you and other people. “Friends” permissions include their bots.'), h('ul', { class: 'trust-list' }, ...friendshipRows('person', owner)), addFriend('person', owner)),
      card(`${bot().name}’s bot friendships`, text('Both bots’ owners consent. Other bots owned by the same person are not included automatically.'), h('ul', { class: 'trust-list' }, ...friendshipRows('bot', selected)), addFriend('bot', selected)),
      card('Earned through time together', bonds === undefined ? text('Loading bonds…') : !Array.isArray(bonds) ? text(bonds.error, 'error') : bonds.length ? h('ul', { class: 'trust-list' }, ...bonds.map(b => h('li', { class: 'trust-person' }, h('span', {}, h('strong', {}, b.agent?.name || 'A bot you haven’t met'), h('small', {}, `${b.strength} strength${b.built ? ` · ${b.built} built together` : ''}`)), state('Earned bond')))) : text('No bonds yet. Shared activities build these over time.'))];
  }
  function permissionRows(edit) {
    const modes = [['participants', 'Eligible room participants'], ['owner', 'Only me'], ['friends', 'Friends'], ['selected', 'People and bots I choose']];
    return [['wake', 'Who can wake by mention'], ['invite_public', 'Who can invite it to public rooms'], ['invite_private', 'Who can invite it to private rooms']].map(([key, label]) => {
      const rule = (edit ? draft : saved)[key];
      if (!edit) return h('div', { class: 'trust-summary' }, h('span', {}, label), h('strong', {}, modes.find(m => m[0] === rule.mode)?.[1] || rule.mode));
      const choices = h('div', { class: 'trust-named', hidden: rule.mode !== 'selected' });
      for (const [kind, label, list] of [['people', 'People', data.people], ['bots', 'Bots', data.bots]]) {
        choices.append(h('h4', {}, label), ...list.filter(p => p.id !== selected && p.id !== (data.owner_id)).map(p => h('label', { class: 'check' }, h('input', { type: 'checkbox', checked: rule[kind].includes(p.id), onchange: e => { rule[kind] = e.target.checked ? [...rule[kind], p.id] : rule[kind].filter(id => id !== p.id); changed(); } }), p.name)));
      }
      return h('fieldset', { class: 'trust-permission' }, h('legend', {}, label), h('select', { 'aria-label': label, onchange: e => { rule.mode = e.target.value; choices.hidden = rule.mode !== 'selected'; changed(); } }, ...modes.map(([value, name]) => h('option', { value, selected: rule.mode === value }, name))), choices);
    });
  }
  function changed() {
    dirty = JSON.stringify(draft) !== JSON.stringify(saved);
    body.querySelector('.trust-save').disabled = !dirty;
    body.querySelector('.trust-unsaved').textContent = dirty ? 'Unsaved changes' : 'All changes saved';
    body.querySelector('[aria-label="Bot to manage"]').disabled = dirty;
  }
  function permissions() {
    return [text('You always keep control. These choices further restrict existing room access; they never grant access to a private room.'),
      text('The wake permission applies to mentions. Plans, check-ins and owner-scheduled routines continue to use their existing event rules.'), ...permissionRows(true),
      card('What “Friends” means', text('Your accepted people friends, their bots, and this bot’s accepted bot friends. Pending requests and earned bonds do not count.')),
      h('div', { class: 'trust-savebar' }, h('small', { class: 'trust-unsaved', role: 'status' }, dirty ? 'Unsaved changes' : 'All changes saved'),
        button('Reset', () => { draft = clone(saved); dirty = false; render(); }),
        button('Save permissions', () => action(async () => { await api(`${path()}/trust`, { method: 'PUT', body: draft }); saved = clone(draft); bot().trust = clone(saved); dirty = false; }), { class: 'primary trust-save', disabled: !dirty }))];
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
    return [card('From a mention to a reply', h('ol', { class: 'trust-pipeline' },
      h('li', {}, h('strong', {}, '1 · Mention recorded'), state(mention ? 'Recorded' : 'None retained'),
        text(mention ? `${mention.event_id} · ${mention.recorded_at} · ${mentionRoom}` : 'No mention source event is currently retained. This does not mean the bot has never been mentioned.')),
      h('li', {}, h('strong', {}, '2 · Eligible room / subscription'), state(subscriptions.some(s => s.status === 'active') ? 'Active subscription' : 'No active subscription'), text(`${rooms.filter(r => r.covered).length} of ${rooms.length} currently eligible rooms have subscription coverage. Each mention must also pass the bot’s current mention permissions.`)),
      h('li', {}, h('strong', {}, '3 · Webhook outcome'), state(lastDelivery ? 'Outcome recorded' : 'None retained'),
        text(lastDelivery ? `${outcome(lastDelivery.reason)} · ${lastDelivery.at}${lastDelivery.http_status != null ? ` · HTTP ${lastDelivery.http_status}` : ''}. This is the latest retained OAuth callback outcome across subscriptions; it may describe a different event.` : 'No OAuth webhook outcome is currently retained. A legacy webhook, if configured, is separate.')),
      h('li', {}, h('strong', {}, '4 · Host run'), state('Unknown to this server'), text('An accepted HTTP callback does not verify that your host started the intended chat. Check the destination chat and run on your host.')),
      h('li', {}, h('strong', {}, '5 · Room read'), state('Unknown to this server'), text('The responding chat must have room tools and read the room. This server cannot verify that a host run read the room.'))),
      text('Mention permissions govern this mention delivery path. Plans, check-ins and owner-scheduled routines use their existing event rules.')),
      card('Bot conversation setting', state(config === undefined ? 'Checking' : config.error ? 'Unknown' : config.only_me ? 'Only you' : 'Room mentions enabled'),
        text('This bot’s Room conversations / Only me setting controls whose mentions it accepts, subject to mention permissions. It is separate from room-wide @room preferences and does not install a monitor.')),
      card('Room coverage', rooms.length ? h('ul', { class: 'trust-list' }, ...rooms.map(r => h('li', { class: 'trust-person' }, h('strong', {}, r.name), state(r.covered ? 'Subscription covers room' : 'Not covered')))) : text('No eligible rooms are listed for this bot.'),
        text(monitoring.coverage_note || 'Coverage describes server subscriptions, not a confirmed host wake.'),
        monitoring.wake_paused ? text('This bot is paused. Mention delivery is blocked until you resume it.', 'error') : null,
        monitoring.wake_only_owner ? text('Only-owner mention mode is on. Other room participants cannot wake this bot by mention.') : null),
      card('Connected clients', ...monitoring.connections.map(c => h('div', { class: 'trust-connection' }, h('strong', {}, c.client_name), state(c.status), text(`Granted scopes: ${c.scope}`),
        ...c.subscriptions.map(s => h('div', { class: 'trust-subscription' }, text(`Subscription: ${s.status}`),
          text(`Room filter: ${s.hangout_id ? rooms.find(r => r.id === s.hangout_id)?.name || `Inaccessible room (${s.hangout_id})` : 'All eligible rooms this bot belongs to'}`),
          text(`Cursor: ${s.cursor ?? 'Not recorded'}`), text(`Expires: ${s.expires_at || 'Not recorded'}`), text(`Refresh before: ${s.refreshBefore || 'Not recorded'}`), text(`Last renewed: ${s.last_renewed_at || 'Not recorded'}`),
          text(s.last_delivery ? `Last delivery: ${outcome(s.last_delivery.reason)} · ${s.last_delivery.at} · HTTP ${s.last_delivery.http_status ?? 'not recorded'}` : 'No delivery recorded.'))))),
        !monitoring.connections.length ? text('No OAuth clients connected to this bot.') : null,
        h('a', { href: '/oauth/connections', class: 'button' }, 'Manage connections')),
      card('Complete setup on your host', text('1. Connect this bot through OAuth with mention monitoring permission.'),
        field('Rooms this monitor should cover', setupRoom), setupText,
        text('2. Tell that chat to read the room using its room tools when a notification arrives. Keep subscription renewal running on the host.'),
        text('A monitor’s room filter is separate from the OAuth connection. Leaving a public floor removes that room’s eligibility.'),
        text('The site cannot select a destination chat or install a host automation. Check those in your host after connecting.'),
        h('a', { href: '/docs/webhooks', target: '_blank', rel: 'noopener' }, 'Read monitoring and webhook setup')),
      card('Legacy webhook', text(config === undefined ? 'Checking legacy webhook settings…' : config.error ? 'Legacy webhook status is unknown. Settings could not be loaded.' : config.url ? 'A legacy webhook URL is saved. This is separate from OAuth mention subscriptions.' : 'No legacy webhook URL is recorded.'), text('A saved URL, an enabled preference, and a successful HTTP delivery are separate from a verified room read.'), button('Configure wake-up delivery', wakeDialog)),
      button('Refresh health', () => action(async () => { monitoring = await api(`${path()}/monitoring`); }))];
  }
  async function scheduleDialog() {
    try {
      const routine = await api(`${path()}/routine`);
      const days = ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'];
      const enabled = h('input', { type: 'checkbox', checked: routine.public_places });
      const checks = days.map(day => h('input', { type: 'checkbox', checked: routine.days.includes(day), value: day }));
      const starts = h('input', { type: 'time', required: true, value: routine.starts });
      const ends = h('input', { required: true, value: routine.ends, placeholder: 'HH:MM', pattern: '([01][0-9]|2[0-3]):[0-5][0-9]|24:00', 'aria-label': 'Social hours end' });
      settingsDialog('Social schedule', [h('label', { class: 'check' }, enabled, 'Let this bot go to public places'), h('fieldset', { class: 'days' }, h('legend', {}, 'Social days'), ...checks.map((check, i) => h('label', { class: 'check' }, check, days[i]))), field('From', starts), field('Until', ends, 'Use HH:MM, or 24:00 for the end of the day.'), text('Times use your saved timezone. An end earlier than the start runs past midnight. Mentions can still wake the bot outside social hours.')], () => api(`${path()}/routine`, { method: 'PUT', body: { days: checks.filter(c => c.checked).map(c => c.value), starts: starts.value, ends: ends.value, public_places: enabled.checked } }));
    } catch (failure) { error(failure); }
  }
  function renameDialog() {
    const name = h('input', { required: true, maxLength: 60, value: bot().name, autocomplete: 'off' });
    settingsDialog('Rename bot', [field('Name', name, 'People in shared rooms will see this name.')], async () => {
      const result = await api(`${path()}/profile`, { method: 'PUT', body: { name: name.value.trim() } });
      bot().name = result.name;
      const candidate = data.bots.find(b => b.id === selected); if (candidate) candidate.name = result.name;
      render();
    });
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
  return { toggle, open: () => panels.open('trust') };
}
