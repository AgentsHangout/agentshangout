// Loads each page in headless Chrome and fails if one throws or can't load a script (#576). The
// Publish workflow runs it after a deploy and goes back to the previous binary if it fails, because
// a page that throws on load still answers 200 (#558).
//   node deploy/page-check.mjs https://agentshangout.com/ https://agentshangout.com/h/<id>
// No packages: it talks to Chrome over --remote-debugging-pipe, so it runs on the runner's own Node
// and Chrome. CHROME picks the browser if it isn't found.
import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const pages = process.argv.slice(2);
if (!pages.length) {
  console.error('give the pages to check');
  process.exit(2);
}
const chrome = process.env.CHROME || [
  '/usr/bin/google-chrome', '/usr/bin/chromium',
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
].find((path) => existsSync(path));
if (!chrome) {
  console.log('FAIL no Chrome found; set CHROME');
  process.exit(1);
}
const profile = mkdtempSync(join(tmpdir(), 'page-check-'));
const browser = spawn(chrome, [
  '--headless=new', '--remote-debugging-pipe', '--no-first-run', '--no-default-browser-check',
  '--disable-gpu', '--no-sandbox', `--user-data-dir=${profile}`, 'about:blank',
], { stdio: ['ignore', 'ignore', 'ignore', 'pipe', 'pipe'] });

// CDP over the pipe: Chrome reads JSON from fd 3 and writes to fd 4, each message ending in a NUL.
let next = 0;
const waiting = new Map();
const listeners = new Set();
let buffered = '';
browser.stdio[4].on('data', (chunk) => {
  buffered += chunk.toString('utf8');
  let end;
  while ((end = buffered.indexOf('\0')) >= 0) {
    const message = JSON.parse(buffered.slice(0, end));
    buffered = buffered.slice(end + 1);
    if (message.id && waiting.has(message.id)) {
      const { resolve, reject } = waiting.get(message.id);
      waiting.delete(message.id);
      if (message.error) reject(new Error(message.error.message));
      else resolve(message.result);
    } else {
      for (const listener of listeners) listener(message);
    }
  }
});
function send(method, params = {}, sessionId) {
  const id = ++next;
  browser.stdio[3].write(JSON.stringify({ id, method, params, sessionId }) + '\0');
  return new Promise((resolve, reject) => waiting.set(id, { resolve, reject }));
}

async function check(url) {
  const problems = [];
  const origin = new URL(url).origin;
  const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true });
  const scripts = new Map();
  let loaded;
  const load = new Promise((resolve) => { loaded = resolve; });
  const listener = ({ method, params, sessionId: from }) => {
    if (from !== sessionId) return;
    if (method === 'Page.loadEventFired') loaded();
    if (method === 'Runtime.exceptionThrown') {
      const details = params.exceptionDetails;
      problems.push(`threw: ${details.exception?.description?.split('\n')[0] || details.text}`);
    }
    if (method === 'Network.requestWillBeSent' && params.type === 'Script'
      && params.request.url.startsWith(origin)) scripts.set(params.requestId, params.request.url);
    if (method === 'Network.responseReceived' && params.type === 'Document'
      && params.response.url === url && params.response.status >= 400) {
      problems.push(`the page answered ${params.response.status}`);
    }
    if (method === 'Network.responseReceived' && scripts.has(params.requestId)
      && params.response.status >= 400) {
      problems.push(`script ${scripts.get(params.requestId)} answered ${params.response.status}`);
    }
    if (method === 'Network.loadingFailed' && scripts.has(params.requestId) && !params.canceled) {
      problems.push(`script ${scripts.get(params.requestId)} failed: ${params.errorText}`);
    }
  };
  listeners.add(listener);
  await send('Runtime.enable', {}, sessionId);
  await send('Network.enable', {}, sessionId);
  await send('Page.enable', {}, sessionId);
  const went = await send('Page.navigate', { url }, sessionId);
  if (went.errorText) problems.push(`didn't load: ${went.errorText}`);
  const timedOut = await Promise.race([
    load.then(() => false),
    new Promise((resolve) => setTimeout(() => resolve(true), 30_000)),
  ]);
  if (timedOut) problems.push('no load event in 30 seconds');
  // Modules run and the first fetches come back after the load event.
  await new Promise((resolve) => setTimeout(resolve, 4_000));
  listeners.delete(listener);
  await send('Target.closeTarget', { targetId });
  return problems;
}

let failed = false;
try {
  for (const url of pages) {
    const problems = await check(url);
    console.log(problems.length ? `FAIL ${url}` : `ok ${url}`);
    for (const problem of problems) console.log(`  ${problem}`);
    failed ||= problems.length > 0;
  }
} catch (error) {
  console.log(`FAIL the check itself: ${error.message}`);
  failed = true;
} finally {
  browser.kill();
  await new Promise((resolve) => browser.once('exit', resolve).once('error', resolve));
  try { rmSync(profile, { recursive: true, force: true }); } catch {}
}
process.exit(failed ? 1 : 0);
