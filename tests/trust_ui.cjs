// Run the shipped trust panel against a small DOM and a controlled server.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
class Node {
  constructor(tag, props = {}, children = []) {
    this.tag = tag; this.tagName = tag.toUpperCase(); this.props = props; this.children = [];
    Object.assign(this, props); this.className = props.class || ''; this.value = props.value || '';
    this.classList = {add: (...names) => { this.className += ` ${names.join(' ')}`; }};
    this.append(...children);
    if (tag === 'select') this.value = this.children.find(c => c.selected)?.value || this.children[0]?.value || '';
  }
  append(...children) { this.children.push(...children.flat(Infinity).filter(c => c != null && c !== false)); }
  replaceChildren(...children) { this.children = []; this.append(...children); }
  contains(node) { return all(this).includes(node); }
  setAttribute(key, value) { this.props[key] = value; }
  querySelector(selector) { return this.querySelectorAll(selector)[0]; }
  querySelectorAll(selector) {
    return all(this).filter(n => selector === 'button' ? n.tag === 'button' : selector.startsWith('.') ? n.className.split(' ').includes(selector.slice(1)) : selector.startsWith('#') ? n.id === selector.slice(1) : selector === '[aria-label="Bot to manage"]' ? n.props['aria-label'] === 'Bot to manage' : selector === 'input' ? n.tag === 'input' : false);
  }
  focus() {} showModal() {} close() { this.props.onclose?.(); } remove() {}
}
const h = (tag, props = {}, ...children) => new Node(tag, props, children);
const all = node => node instanceof Node ? [node, ...node.children.flatMap(all)] : [];
const text = node => node instanceof Node ? node.children.map(text).join(' ') : String(node);
const findButton = (node, label) => all(node).find(n => n.tag === 'button' && text(n) === label);
const tick = () => new Promise(resolve => setImmediate(resolve));
const policy = () => Object.fromEntries(['wake','invite_public','invite_private'].map(k => [k,{mode:'participants',people:[],bots:[]} ]));
const data = {owner_id:'owner', people:[{id:'owner',name:'You'},{id:'friend',name:'<img src=x onerror=alert(1)>'}], bots:[{id:'one',name:'Fern',owner_id:'owner'},{id:'two',name:'Moss',owner_id:'owner'},{id:'other',name:'Pine',owner_id:'friend'}], agents:[{id:'one',name:'Fern',doorbell:{paused:false},trust:policy()},{id:'two',name:'Moss',trust:policy()}], friendships:[{kind:'person',from_id:'friend',to_id:'owner',status:'pending'}]};
let body, options, requests = [], rejectSave = false, deferred = [];
const api = async (path, options = {}) => {
  requests.push({path,...options});
  if (path === '/api/trust') return JSON.parse(JSON.stringify(data));
  if (path.endsWith('/trust')) { if (rejectSave) throw Error('Save failed. Your changes are still here.'); return {}; }
  if (path.includes('/friendships')) return {};
  if (path.endsWith('/monitoring')) return new Promise((resolve,reject) => deferred.push({path,resolve,reject}));
  if (path.endsWith('/doorbell')) return {};
  if (path.endsWith('/bonds')) return {bonds:[]};
  if (path.endsWith('/notify')) { if (options.method === 'PUT') return {}; if (path.includes('/two/')) throw Error('Notify temporarily unavailable'); return {only_me:false,url:null}; }
  if (path.endsWith('/routine')) return options.method === 'PUT' ? {} : {days:['mon'],starts:'09:00',ends:'24:00',public_places:true};
  throw Error(`Unexpected ${path}`);
};
const panels = { add: (_name,_title,opts) => { options = opts; body = h('div'); return body; }, toggle: () => h('button'), open: () => options.onOpen() };
const context = {h,api,panels,document:{activeElement:null,body:h('body')},console};
vm.createContext(context);
vm.runInContext(fs.readFileSync('web/trust.js','utf8').replace('export function trustPanel','function trustPanel'),context);
function build() { return context.trustPanel({h,api,panels,me:{name:'You',kind:'person'},crew:()=>{}}); }
(async () => {
  const panel = build(); await panel.open(); await tick();
  assert.ok(text(body).includes('Fern')); assert.ok(findButton(body,'Wake-up settings'));
  // Confirmed settings updates refresh health and invalidate an older in-flight snapshot.
  const pause = findButton(body,'Pause bot').props.onclick(); await tick();
  deferred.at(-1).resolve({latest_mention:null,connections:[{client_name:'Host',status:'active',scope:'agent:read',subscriptions:[{status:'active',hangout_id:null,cursor:1,last_delivery:{reason:'http_accepted',at:'2026-11-03T04:00:00Z',http_status:202}}]}],eligible_rooms:[],wake_paused:true}); await pause;
  deferred[0].resolve({connections:[],eligible_rooms:[],wake_paused:false}); await tick();
  findButton(body,'Monitoring').props.onclick(); assert.ok(text(body).includes('This bot is paused.'));
  assert.ok(text(body).includes('This does not mean the bot has never been mentioned.'));
  assert.ok(text(body).includes('Accepted by the host callback'));
  assert.ok(text(body).includes('HTTP 202'));
  assert.equal(all(body).filter(n=>n.tag==='span'&&text(n)==='Unknown to this server').length,2);
  findButton(body,'Overview').props.onclick();
  const resume = findButton(body,'Resume bot').props.onclick(); await tick();
  deferred.at(-1).reject(Error('Health offline')); await resume;
  assert.ok(findButton(body,'Pause bot')); assert.equal(text(body).includes('Health offline'),false);
  findButton(body,'Monitoring').props.onclick(); assert.ok(text(body).includes('Settings saved. Monitoring health could not be refreshed'));
  findButton(body,'Overview').props.onclick(); await findButton(body,'Wake-up settings').props.onclick();
  const wakeDialog = context.document.body.children.at(-1);
  const conversations = all(wakeDialog).find(n=>n.tag==='input'&&n.type==='checkbox'); conversations.checked=false;
  const wakeSave = all(wakeDialog).find(n=>n.tag==='form').props.onsubmit({preventDefault(){}}); await tick();
  deferred.at(-1).resolve({connections:[],eligible_rooms:[],wake_only_owner:true}); await wakeSave;
  findButton(body,'Monitoring').props.onclick(); assert.ok(text(body).includes('Only-owner mention mode is on.'));
  findButton(body,'Overview').props.onclick();
  // Switching the selected bot ignores the previous bot's delayed health response.
  const chooser = body.querySelector('[aria-label="Bot to manage"]');
  await chooser.props.onchange({target:{value:'two'}}); await tick();
  deferred.find(r=>r.path.includes('/one/')).resolve({connections:[],eligible_rooms:[{id:'old',name:'STALE ROOM',covered:true}]});
  deferred.find(r=>r.path.includes('/two/')).resolve({latest_mention:{event_id:'evt_42',recorded_at:'2026-11-03T05:00:00Z',hangout_id:'room'},connections:[{client_name:'Host',status:'active',scope:'agent:read',subscriptions:[{status:'expired',hangout_id:'room',cursor:42,last_delivery:{reason:'callback_failed',at:'2026-11-03',http_status:503}},{status:'expired',hangout_id:null,cursor:0},{status:'stopped',hangout_id:'hidden',cursor:7}]}],eligible_rooms:[{id:'room',name:'Moss room',covered:false}]}); await tick();
  findButton(body,'Monitoring').props.onclick();
  assert.ok(text(body).includes('Moss room')); assert.ok(!text(body).includes('STALE ROOM'));
  assert.ok(text(body).includes('Unknown to this server')); assert.ok(text(body).includes('No active subscription'));
  for (const stage of ['1 · Mention recorded','2 · Eligible room / subscription','3 · Webhook outcome','4 · Host run','5 · Room read','evt_42 · 2026-11-03T05:00:00Z · Moss room','Bot conversation setting','separate from room-wide @room preferences']) assert.ok(text(body).includes(stage),stage);
  assert.equal(all(body).filter(n=>n.tag==='span'&&text(n)==='Unknown to this server').length,2);
  for (const expected of ['Room filter: Moss room','All eligible rooms this bot belongs to','Inaccessible room (hidden)','Cursor: 42','Cursor: 0','Host callback failed','Legacy webhook status is unknown']) assert.ok(text(body).includes(expected),expected);
  const setup = all(body).find(n=>n.tag==='select'&&n.props['aria-label']==='Room for monitoring setup');
  setup.props.onchange({target:{value:'room'}});
  assert.equal(all(body).find(n=>n.textContent?.includes('arguments.hangout_id')).textContent.includes('Moss room'),true);
  // Editing selects keeps a draft, locks bot switching, and a failed save retains it.
  findButton(body,'Permissions').props.onclick();
  assert.ok(text(body).includes('Plans, check-ins and owner-scheduled routines continue to use their existing event rules.'));
  const permissionSelect = all(body).find(n=>n.tag==='select'&&n.props['aria-label']==='Who can wake by mention');
  permissionSelect.props.onchange({target:{value:'owner'}});
  assert.equal(body.querySelector('[aria-label="Bot to manage"]').disabled,true);
  assert.equal(findButton(body,'Save permissions').disabled,false);
  const dirtyLoads = requests.filter(r=>r.path==='/api/trust').length; await panel.open();
  assert.equal(requests.filter(r=>r.path==='/api/trust').length,dirtyLoads);
  rejectSave = true; await findButton(body,'Save permissions').props.onclick();
  assert.ok(text(body).includes('Save failed')); assert.ok(text(body).includes('Unsaved changes'));
  assert.equal(all(body).find(n=>n.tag==='select'&&n.props['aria-label']==='Who can wake by mention').children.find(n=>n.selected).value,'owner');
  rejectSave = false; await findButton(body,'Save permissions').props.onclick();
  assert.equal(requests.findLast(r=>r.path.endsWith('/trust')&&r.method==='PUT').body.wake.mode,'owner');
  assert.ok(text(body).includes('All changes saved'));
  // Editing an unrelated schedule setting preserves a saved end of day exactly.
  findButton(body,'Overview').props.onclick(); await findButton(body,'Social schedule').props.onclick();
  const dialog = context.document.body.children.at(-1);
  const until = all(dialog).find(n=>n.props['aria-label']==='Social hours end'); assert.equal(until.value,'24:00');
  await all(dialog).find(n=>n.tag==='form').props.onsubmit({preventDefault(){}});
  assert.equal(requests.findLast(r=>r.path.endsWith('/routine')&&r.method==='PUT').body.ends,'24:00');
  // Invitations are rendered as text and acceptance uses the server's owner identity.
  findButton(body,'Relationships').props.onclick();
  assert.ok(text(body).includes('<img src=x onerror=alert(1)>')); assert.equal(all(body).some(n=>n.tag==='img'),false);
  await findButton(body,'Accept').props.onclick();
  assert.equal(requests.at(-2).path,'/api/trust/friendships/accept');
  assert.equal(requests.at(-2).body.to_id,'owner');
  // Opening again refreshes an outside friendship change; no-bot accounts retain friendships.
  const loads = requests.filter(r=>r.path==='/api/trust').length;
  await panel.open(); await tick(); assert.ok(requests.filter(r=>r.path==='/api/trust').length>loads);
  data.agents=[]; const empty=build(); await empty.open(); await tick();
  assert.ok(text(body).includes('Your people friendships')); assert.ok(findButton(body,'Accept')); assert.ok(findButton(body,'Open your crew'));
  console.log('Trust UI selection, stale requests, permissions, friendship consent and empty states passed');
})().catch(error=>{console.error(error);process.exitCode=1;});
