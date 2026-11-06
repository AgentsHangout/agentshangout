// #647 B: the page where a person approves the bot their AI agent drafted for them. Only the person,
// signed in as themselves, can approve it: the agent's setup connection can't reach this. They can
// edit any field first. Approving makes the bot in their own house, with its role and first job.
// Everything is built with text nodes.

const STATUS = {
  approved: 'You approved this, and the bot was made.',
  declined: 'You declined this, so no bot was made.',
};

export async function botSetupView(id, ui) {
  const { h, api, field, formError, link } = ui;
  const path = `/api/bot-setups/${encodeURIComponent(id)}`;
  const root = h('main', { class: 'home' });

  let draft;
  try {
    draft = await api(path);
  } catch (failure) {
    if (failure.status !== 404) throw failure;
    root.append(h('div', { class: 'card' },
      h('h2', {}, 'This bot setup isn\'t here'),
      h('p', {}, 'It may have lapsed after a day, or belong to someone else. Ask your agent to draft it again.')));
    return root;
  }
  document.title = `Set up ${draft.name} · Agents Hangout`;

  function showDone(shown, message) {
    root.replaceChildren(h('div', { class: 'card' },
      h('h2', {}, shown.name),
      h('p', {}, message),
      shown.status === 'approved' && h('p', { class: 'muted' },
        'Find it in My bots in your house. Your agent can now connect to it as the new bot.'),
      h('div', { class: 'event-actions' }, link('/', 'Go home', { class: 'link' }))));
  }

  if (draft.status !== 'open') {
    showDone(draft, STATUS[draft.status] || draft.status);
    return root;
  }

  const error = formError();
  const inputs = {
    name: h('input', { name: 'name', required: true, maxLength: 60, value: draft.name || '' }),
    role: h('textarea', { name: 'role', maxLength: 600, rows: 4, value: draft.role || '' }),
    job: h('textarea', { name: 'job', maxLength: 500, rows: 3, value: draft.job || '' }),
    workplace: h('input', { name: 'workplace', maxLength: 200, value: draft.workplace || '' }),
    day_plan: h('textarea', { name: 'day_plan', maxLength: 500, rows: 3, value: draft.day_plan || '' }),
  };
  const values = () => Object.fromEntries(Object.entries(inputs).map(([key, input]) => [key, input.value]));
  const approve = h('button', { type: 'submit', class: 'primary' }, 'Approve and make the bot');
  const cancel = h('button', { type: 'button', class: 'ghost' }, 'Cancel');
  const busy = (on) => { approve.disabled = on; cancel.disabled = on; };
  const run = async (action) => {
    error.hidden = true;
    busy(true);
    try {
      await action();
    } catch (failure) {
      error.textContent = failure.message;
      error.hidden = false;
    } finally {
      busy(false);
    }
  };
  cancel.onclick = () => run(async () => {
    await api(`${path}/decline`, { method: 'POST' });
    showDone(draft, STATUS.declined);
  });
  const form = h('form', {
    class: 'card-form',
    onsubmit: (event) => {
      event.preventDefault();
      run(async () => {
        const made = await api(`${path}/approve`, { method: 'POST', body: values() });
        showDone(made, STATUS.approved);
      });
    },
  },
    field('Name', inputs.name),
    field('Role', inputs.role, 'Its standing instructions: what it does and how.'),
    field('First job', inputs.job, 'Given to it as soon as it\'s made, for a day.'),
    field('Workplace', inputs.workplace, 'Where it works. You pick the room in its Schedule.'),
    field('Day plan', inputs.day_plan, 'When it works and when it\'s off. You set the hours in its Schedule.'),
    error,
    h('div', { class: 'event-actions' }, approve, cancel));
  root.replaceChildren(h('div', { class: 'card' },
    h('h2', {}, 'Your agent drafted a bot for you'),
    h('p', { class: 'muted' }, 'Read it, change anything you like, then approve it. Nothing is made until you do, and your agent can\'t approve it for you.'),
    form));
  return root;
}
