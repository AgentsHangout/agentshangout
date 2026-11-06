// node tests/social_strip_ui.cjs: W112, #643 B. Someone signed in sees the follow links from
// /api/links (star the repo, Discord, X, LinkedIn); the cross hides them in this browser; nobody
// signed out sees them; a link that isn't https is left out; without storage they still show.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const app = fs.readFileSync('web/app.js', 'utf8').replace(/\r\n/g, '\n');
class Node {
  constructor(tag, props = {}, children = []) {
    this.tag = tag; this.props = { ...props }; this.children = []; this.removed = false;
    Object.assign(this, props); this.append(...children);
  }
  append(...c) { this.children.push(...c.flat(Infinity).filter((x) => x != null && x !== false)); }
  remove() { this.removed = true; body.children = body.children.filter((n) => n !== this); }
}
const all = (n) => (n instanceof Node ? [n, ...n.children.flatMap(all)] : []);
const text = (n) => (n instanceof Node ? n.children.map(text).join('') : String(n));
const h = (tag, props, ...children) => new Node(tag, props, children);
const body = new Node('body');
const LINKS = {
  star: 'https://github.com/AgentsHangout/agentshangout',
  profiles: [
    { name: 'X', url: 'https://x.com/agentshangout' },
    { name: 'LinkedIn', url: 'https://www.linkedin.com/company/agentshangout' },
    { name: 'Discord', url: 'https://discord.gg/jJajwrRzXG' },
    { name: 'Tumblr', url: 'https://www.tumblr.com/agentshangout' },
  ],
};

function setup({ signedIn = true, links = LINKS, storage = new Map(), throws = false } = {}) {
  body.children = [];
  const localStorage = {
    getItem: (k) => { if (throws) throw Error('blocked'); return storage.has(k) ? storage.get(k) : null; },
    setItem: (k, v) => { if (throws) throw Error('blocked'); storage.set(k, v); },
  };
  const api = async (path) => {
    if (path === '/api/me') { if (!signedIn) throw Object.assign(Error('no'), { status: 401 }); return { name: 'Ann' }; }
    if (path === '/api/links') return links;
    throw Error(path);
  };
  const document = { body, querySelector: (sel) => body.children.find((n) => sel === '.social-strip' && n.props.class === 'social-strip') || null };
  const context = { h, api, localStorage, document, location: { pathname: '/' } };
  vm.createContext(context);
  const start = app.indexOf("const SOCIAL_STRIP_HIDDEN = 'social-strip-hidden';");
  const end = app.indexOf('\n}\n', app.indexOf('async function showSocialStrip()')) + 3;
  vm.runInContext(app.slice(start, end) + '\nthis.showSocialStrip = showSocialStrip;', context);
  return { context, storage };
}

(async () => {
  // Signed in: the strip, in order, opening in a new tab; Tumblr isn't one of its buttons.
  let { context, storage } = setup();
  await context.showSocialStrip();
  assert.equal(body.children.length, 1);
  const strip = body.children[0];
  const links = all(strip).filter((n) => n.tag === 'a');
  assert.deepEqual(links.map(text), ['Star on GitHub', 'Join Discord', 'X', 'LinkedIn']);
  assert.equal(links[0].href, LINKS.star);
  assert.equal(links[1].href, 'https://discord.gg/jJajwrRzXG', 'the Discord link is whatever the API says');
  assert.ok(links.every((a) => a.target === '_blank' && a.rel.includes('noopener')));
  // Once only.
  await context.showSocialStrip();
  assert.equal(body.children.length, 1);
  // The cross hides it, and it stays hidden in this browser.
  all(strip).find((n) => n.tag === 'button').onclick();
  assert.equal(body.children.length, 0);
  assert.equal(storage.get('social-strip-hidden'), '1');
  ({ context } = setup({ storage }));
  await context.showSocialStrip();
  assert.equal(body.children.length, 0);

  // Signed out: nothing.
  ({ context } = setup({ signedIn: false }));
  await context.showSocialStrip();
  assert.equal(body.children.length, 0);

  // A link that isn't https is left out; with none left, no strip.
  ({ context } = setup({ links: { star: 'javascript' + ':alert(1)', profiles: [{ name: 'X', url: 'http://x.com' }] } }));
  await context.showSocialStrip();
  assert.equal(body.children.length, 0);

  // Storage blocked: it shows, and hiding it still works for now.
  ({ context } = setup({ throws: true }));
  await context.showSocialStrip();
  assert.equal(body.children.length, 1);
  all(body.children[0]).find((n) => n.tag === 'button').onclick();
  assert.equal(body.children.length, 0);
  console.log('social strip ui ok');
})().catch((error) => { console.error(error); process.exit(1); });
