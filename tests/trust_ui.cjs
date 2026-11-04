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
const node = name => all(body).find(n=>n.tag==='g'&&n.props['aria-label']?.startsWith(`${name}, `));
const tick = () => new Promise(resolve => setImmediate(resolve));
const policy = () => Object.fromEntries(['wake','invite_public','invite_private'].map(k => [k,{mode:'participants',people:[],bots:[]} ]));
const data = {owner_id:'owner', people:[{id:'owner',name:'You'},{id:'friend',name:'<img src=x onerror=alert(1)>'}], bots:[{id:'one',name:'Fern',owner_id:'owner'},{id:'two',name:'Moss',owner_id:'owner'},{id:'other',name:'Pine',owner_id:'friend'}], agents:[{id:'one',name:'Fern',doorbell:{paused:false},trust:policy()},{id:'two',name:'Moss',trust:policy()}], friendships:[{kind:'person',from_id:'friend',to_id:'owner',status:'pending'}],looks:{people:{owner:{skin:'peach',hair:'red'}},bots:{one:{head:'twin'},two:{head:'wide'}}}};
let crowd = false, gods = false;
let body, options, requests = [], rejectSave = false, deferred = [];
const api = async (path, options = {}) => {
  requests.push({path,...options});
  if (path === '/api/trust') return JSON.parse(JSON.stringify(data));
  if (path.endsWith('/trust')) { if (rejectSave) throw Error('Save failed. Your changes are still here.'); return {}; }
  if (path.includes('/friendships')) return {};
  if (path === '/api/usage') { if (!gods) throw Error('Not found'); return {by_tool:[{name:'/api/me',calls:3,refused:0,errors:0,avg_ms:4,p50_ms:5,p90_ms:10,slow:0,top_refusal:null}],by_day:[{day:'2026-11-03',active_people:2,active_bots:1,calls:3,refused:0,p50_ms:5,p90_ms:10,lines:{events_delivered:7,long_polls:2,webhooks_sent:4,webhooks_failed:1}}],by_hour:[{hour:'2026-11-03T04:00Z',active_people:1,active_bots:1,calls:3,refused:0,p50_ms:5,p90_ms:10}],by_version:[{version:'local',calls:3,refused:0,errors:0,avg_ms:4,p50_ms:5,p90_ms:10}],lines:{events_delivered:7,long_polls:2,webhooks_sent:4,webhooks_failed:1}}; }
  if (path.endsWith('/monitoring')) return new Promise((resolve,reject) => deferred.push({path,resolve,reject}));
  if (path.endsWith('/doorbell')) return {};
  if (path.endsWith('/bonds')) return {bonds: crowd ? ['Mac','Chainy','Victor Sausage','Axiom','Ron','Morgkorg','A very long bot name indeed','Aionoro','Kyle’s Agent'].map((name,i)=>({agent:{id:`x${i}`,name},strength:i+1,built:0})) : []};
  if (path.endsWith('/notify')) { if (options.method === 'PUT') return {}; if (path.includes('/two/')) throw Error('Notify temporarily unavailable'); return {only_me:false,url:null}; }
  if (path.endsWith('/routine')) return options.method === 'PUT' ? {} : {days:['mon'],starts:'09:00',ends:'24:00',public_places:true};
  throw Error(`Unexpected ${path}`);
};
const panels = { add: (_name,_title,opts) => { options = opts; body = h('div'); return body; }, toggle: () => h('button'), open: () => options.onOpen() };
const pictures = [];
const canvas = () => ({width:0,height:0,getContext:()=>({imageSmoothingEnabled:true,drawImage(){}}),toDataURL:()=>'data:image/png;base64,HEAD'});
const figure = worn => { pictures.push(worn); return {width:26,height:36,getContext:()=>({getImageData:()=>({data:Uint8ClampedArray.from({length:26*36*4},(_,i)=>i%4===3&&i>=4*26*5?255:0)})})}; };
const context = {h,api,panels,document:{activeElement:null,body:h('body'),createElement:canvas},console};
vm.createContext(context);
vm.runInContext(fs.readFileSync('web/trust.js','utf8').replace('export function trustPanel','function trustPanel'),context);
const scheduled = [];
function build(extra = {}) { return context.trustPanel({h,api,panels,me:{name:'You',kind:'person'},crew:()=>{},schedule:(b)=>scheduled.push(b.name),...extra}); }
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
  findButton(body,'Connections').props.onclick();
  const resume = findButton(body,'Resume bot').props.onclick(); await tick();
  deferred.at(-1).reject(Error('Health offline')); await resume;
  assert.ok(findButton(body,'Pause bot')); assert.equal(text(body).includes('Health offline'),false);
  findButton(body,'Monitoring').props.onclick(); assert.ok(text(body).includes('Settings saved. Monitoring health could not be refreshed'));
  findButton(body,'Connections').props.onclick(); await findButton(body,'Wake-up settings').props.onclick();
  const wakeDialog = context.document.body.children.at(-1);
  const conversations = all(wakeDialog).find(n=>n.tag==='input'&&n.type==='checkbox'); conversations.checked=false;
  const wakeSave = all(wakeDialog).find(n=>n.tag==='form').props.onsubmit({preventDefault(){}}); await tick();
  deferred.at(-1).resolve({connections:[],eligible_rooms:[],wake_only_owner:true}); await wakeSave;
  findButton(body,'Monitoring').props.onclick(); assert.ok(text(body).includes('Only-owner mention mode is on.'));
  findButton(body,'Connections').props.onclick();
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
  findButton(body,'Connections').props.onclick();
  assert.ok(text(body).includes('plans, check-ins and owner-scheduled routines keep their own rules.'));
  const permissionSelect = all(body).find(n=>n.tag==='select'&&n.props['aria-label']==='Wake it by mention');
  permissionSelect.props.onchange({target:{value:'owner'}});
  assert.equal(body.querySelector('[aria-label="Bot to manage"]').disabled,true);
  assert.equal(findButton(body,'Save permissions').disabled,false);
  const dirtyLoads = requests.filter(r=>r.path==='/api/trust').length; await panel.open();
  assert.equal(requests.filter(r=>r.path==='/api/trust').length,dirtyLoads);
  rejectSave = true; await findButton(body,'Save permissions').props.onclick();
  assert.ok(text(body).includes('Save failed')); assert.ok(text(body).includes('Unsaved changes'));
  assert.equal(all(body).find(n=>n.tag==='select'&&n.props['aria-label']==='Wake it by mention').children.find(n=>n.selected).value,'owner');
  // While permissions are unsaved, picking another of your bots in the graph doesn't switch to it.
  node('Fern').props.onkeydown({key:'Enter',preventDefault(){}});
  assert.ok(text(body).includes('Save or reset this bot’s permissions')); assert.ok(text(body).includes('Unsaved changes'));
  rejectSave = false; await findButton(body,'Save permissions').props.onclick();
  assert.equal(requests.findLast(r=>r.path.endsWith('/trust')&&r.method==='PUT').body.wake.mode,'owner');
  assert.ok(text(body).includes('All changes saved'));
  // Schedule opens the same day plan dialog as the room's Crew, for the bot being edited.
  findButton(body,'Connections').props.onclick(); findButton(body,'Schedule').props.onclick();
  assert.deepEqual(scheduled,['Moss']);
  assert.ok(!all(body).some(n=>n.tag==='button'&&n.children.includes('Social schedule')),'the old social schedule is gone');
  // The graph has you, both your bots and the person who asked, with their request still open.
  findButton(body,'Connections').props.onclick();
  for (const name of ['You','Fern','Moss','<img src=x onerror=alert(1)>']) assert.ok(node(name),name);
  assert.ok(node('<img src=x onerror=alert(1)>').props.class.includes('waiting'));
  assert.ok(node('Moss').props.class.includes('editing')); assert.ok(!node('Fern').props.class.includes('editing'));
  assert.equal(all(body).filter(n=>n.tag==='path'&&n.props.class.includes('owns')).length,2);
  assert.equal(all(body).filter(n=>n.tag==='path'&&n.props.class.includes('pending')).length,1);
  // Picking a bot that's yours edits it in place; picking a person shows their friendship.
  node('Moss').props.onkeydown({key:'Enter',preventDefault(){}});
  assert.ok(all(body).some(n=>n.tag==='input'&&n.props['aria-label']==='Bot name'&&n.props.value==='Moss'));
  // Invitations are rendered as text and acceptance uses the server's owner identity.
  node('<img src=x onerror=alert(1)>').props.onkeydown({key:'Enter',preventDefault(){}});
  assert.ok(text(body).includes('<img src=x onerror=alert(1)>')); assert.equal(all(body).some(n=>n.tag==='img'),false);
  await findButton(body,'Accept').props.onclick();
  assert.equal(requests.at(-2).path,'/api/trust/friendships/accept');
  assert.equal(requests.at(-2).body.to_id,'owner');
  // Opening again refreshes an outside friendship change; no-bot accounts retain friendships.
  const loads = requests.filter(r=>r.path==='/api/trust').length;
  await panel.open(); await tick(); assert.ok(requests.filter(r=>r.path==='/api/trust').length>loads);
  // BT7: with a way to draw figures, each node is its head; a node with no look keeps its letter.
  const heady = build({figure}); await heady.open(); await tick();
  const images = all(body).filter(n=>n.tag==='image');
  assert.ok(images.length>=3,'heads for you and both bots'); assert.ok(images.every(n=>n.props.href.startsWith('data:image/png')));
  assert.ok(pictures.some(w=>w.worn.head==='twin')); assert.ok(pictures.some(w=>w.worn.hair==='red'));
  const has = name => all(node(name)).some(n=>n.tag==='image');
  assert.ok(has('Fern')&&has('Moss')&&has('You'));
  assert.equal(has('<img src=x onerror=alert(1)>'),false);
  assert.ok(all(node('<img src=x onerror=alert(1)>')).some(n=>n.tag==='text'&&n.props.class==='initial'));
  assert.equal(all(node('Fern')).some(n=>n.tag==='text'&&n.props.class==='initial'),false);
  assert.equal(all(body).some(n=>n.tag==='img'),false);
  // BT9: Change look hands the picked bot to the Character panel, and new looks reach the graph.
  const dressed = []; const dresser = build({figure,dress:id=>dressed.push(id)}); await dresser.open(); await tick();
  await findButton(body,'Change look').props.onclick(); assert.deepEqual(dressed,['one']);
  const before = pictures.length; data.looks.bots.one = {head:'antenna',chest:'moon'};
  await dresser.refreshLooks(); await tick();
  assert.ok(pictures.slice(before).some(w=>w.worn.chest==='moon'),'the picked bot is drawn in its new look');
  // BT8: nothing depends on the stylesheet: a background, and a fill and stroke on every shape and line.
  assert.ok(all(body).some(n=>n.tag==='rect'&&n.props.class==='trust-ground'&&n.props.fill));
  for (const n of all(body).filter(n=>n.tag==='path'&&n.props.class?.includes('trust-edge'))) assert.ok(n.props.stroke&&n.props['stroke-width'],n.props.class);
  for (const n of all(body).filter(n=>['circle','rect'].includes(n.tag)&&n.props.class!=='trust-ground')) assert.ok(n.props.fill,n.props.class||n.tag);
  for (const n of all(body).filter(n=>n.tag==='circle'&&n.props.class!=='ring'||n.tag==='rect'&&n.props.class!=='trust-ground')) assert.ok(n.props.stroke);
  assert.ok(text(body).includes('Round is a person, square is a bot'));
  // BT8: in a crowded graph no name overlaps another name or a node.
  crowd = true; data.friendships.push({kind:'bot',from_id:'one',to_id:'other',status:'accepted'});
  const busy = build({figure}); await busy.open(); await tick(); await tick();
  const placed = all(body).filter(n=>n.tag==='g'&&n.props.class?.includes('trust-node')).map(g=>{
    const [x,y] = g.props.transform.match(/-?[\d.]+/g).map(Number);
    const label = all(g).find(n=>n.props.class==='label'); const w = [...text(label).replace(/\s+/g,' ').trim()].length*6.4+6;
    const half = g.props.class.includes(' you')?22:g.props.class.includes(' person')?17:g.props.class.includes(' mine')?19:16;
    const cx = x+Number(label.props.x), cy = y+Number(label.props.y);
    return {name:g.props['aria-label'], x, y, half, box:{x:cx-w/2,y:cy-10,w,h:13}};
  });
  assert.ok(placed.length>=10,'a crowded graph');
  const meet = (a,b) => a.x<b.x+b.w&&b.x<a.x+a.w&&a.y<b.y+b.h&&b.y<a.y+a.h;
  for (const a of placed) for (const b of placed) {
    if (a===b) continue;
    if (a.name<b.name) assert.equal(meet(a.box,b.box),false,`names ${a.name} and ${b.name} overlap`);
    assert.equal(meet(a.box,{x:b.x-b.half,y:b.y-b.half,w:b.half*2,h:b.half*2}),false,`name of ${a.name} sits on ${b.name}`);
  }
  crowd = false;
  data.agents=[]; const empty=build(); await empty.open(); await tick();
  assert.ok(text(body).includes('Your people friendships')); assert.ok(findButton(body,'Accept')); assert.ok(findButton(body,'Open your crew'));
  // U21: someone who runs the server reaches the Usage tab with no bots; anyone else still has no tabs.
  assert.equal(findButton(body, 'Usage'), undefined);
  gods = true; const operator = build({me:{name:'You',kind:'person',god_mode:false}}); await operator.open(); await tick();
  const usageTab = findButton(body, 'Usage'); assert.ok(usageTab, 'an operator with no bots has the Usage tab');
  usageTab.props.onclick(); await tick(); await tick();
  const shown = text(body);
  assert.ok(shown.includes('Median ms') && shown.includes('Last 24 hours') && shown.includes('Events delivered 7') && shown.includes('webhook POSTs 4 (1 failed)'));
  assert.ok(!requests.some(r => r.path.includes('/agents/') && r.path.endsWith('/usage')), 'no bot, so no bot usage is asked for');
  console.log('Trust UI selection, stale requests, permissions, friendship consent and empty states passed');
})().catch(error=>{console.error(error);process.exitCode=1;});
