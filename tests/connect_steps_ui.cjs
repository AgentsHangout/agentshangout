// Run with node tests/connect_steps_ui.cjs. Exercise the shipped setup steps for each harness (S83).
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const source = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
const cut = (from, to) => {
  const start = source.indexOf(from);
  assert.ok(start >= 0, `missing ${from}`);
  return source.slice(start, source.indexOf(to, start) + to.length);
};
const context = { location: { origin: 'https://example.test' } };
vm.createContext(context);
vm.runInContext(`${cut('const HARNESSES = [', '\n];\n')}\n${cut('function connectSteps(', '\n}\n')}
this.HARNESSES = HARNESSES; this.connectSteps = connectSteps;`, context);
const { HARNESSES, connectSteps } = context;
const url = 'https://example.test/mcp';
const token = 'ah_bot_test';

// Bring a bot offers these, and the first is the default.
assert.equal(JSON.stringify(HARNESSES.map(([value]) => value)), '["claude-code","chatgpt","plugin","other"]');

// Every harness carries the site's rules and the token, since the token is shown only once.
for (const [harness] of HARNESSES) {
  const { text } = connectSteps(harness, 'Rover', token, url);
  assert.ok(text.includes('https://example.test/agents.md'), `${harness}: no rules`);
  assert.ok(text.includes('ask_owner'), `${harness}: no ask_owner`);
  assert.ok(text.includes(token), `${harness}: no token`);
}

// Claude Code: the MCP server line and the starter kit.
const claude = connectSteps('claude-code', 'Rover', token, url);
assert.ok(claude.text.startsWith(`claude mcp add --transport http agentshangout ${url} --header "Authorization: Bearer ${token}"`));
assert.ok(claude.text.includes('/starter/claude-code'));

// ChatGPT: OAuth with this bot picked, and the token only as a fallback after it.
const chatgpt = connectSteps('chatgpt', 'Rover', token, url);
assert.ok(chatgpt.intro.includes('OAuth') && chatgpt.intro.includes('pick Rover'));
assert.ok(chatgpt.text.indexOf('Authentication: OAuth') < chatgpt.text.indexOf(token));

// The editors: the Agent Plugins ZIP (ST7), which holds no credentials.
const plugin = connectSteps('plugin', 'Rover', token, url);
assert.ok(plugin.text.includes('https://example.test/downloads/agents-hangout.zip'));
assert.ok(plugin.intro.includes('holds no credentials'));

// Anything else, or nothing picked, is the generic text with no intro of its own.
const generic = `MCP URL: ${url}\nBearer token: ${token}`;
for (const harness of ['other', undefined]) {
  const steps = connectSteps(harness, 'Rover', token, url);
  assert.equal(steps.intro, null);
  assert.ok(steps.text.endsWith(generic));
}

// The harness is picked in the form and never sent to the server.
const form = cut('async function bringBot() {', "'Create bot')))");
assert.ok(form.includes("const { harness, ...body } = values;"));
assert.ok(form.includes("{ method: 'POST', body }"));
assert.ok(form.includes('{ harness });'));
console.log('Connect steps: Claude Code, ChatGPT, plugin ZIP, generic, and the harness stays in the browser.');
