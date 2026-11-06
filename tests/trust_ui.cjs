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
let delayNotify = false, delayedNotify;
let delayNotifySave = false, delayedNotifySave;
let body, options, requests = [], rejectSave = false, deferred = [];
const api = async (path, options = {}) => {
  requests.push({path,...options});
  if (path === '/api/trust') return JSON.parse(JSON.stringify(data));
  if (path.endsWith('/trust')) {
    if (rejectSave) throw Error('Save failed. Your changes are still here.');
    const trust = JSON.parse(JSON.stringify(options.body));
    for (const rule of Object.values(trust)) if (rule.mode !== 'selected') { rule.people = []; rule.bots = []; }
    return {trust};
  }
  if (path.includes('/friendships')) return {};
  if (path === '/api/usage') { if (!gods) throw Error('Not found'); return {by_tool:[{name:'/api/me',calls:3,refused:0,errors:0,avg_ms:4,p50_ms:5,p90_ms:10,slow:0,top_refusal:null}],by_day:[{day:'2026-11-03',active_people:2,active_bots:1,calls:3,refused:0,p50_ms:5,p90_ms:10,lines:{events_delivered:7,long_polls:2,webhooks_sent:4,webhooks_failed:1}}],by_hour:[{hour:'2026-11-03T04:00Z',active_people:1,active_bots:1,calls:3,refused:0,p50_ms:5,p90_ms:10}],by_version:[{version:'local',calls:3,refused:0,errors:0,avg_ms:4,p50_ms:5,p90_ms:10}],lines:{events_delivered:7,long_polls:2,webhooks_sent:4,webhooks_failed:1}}; }
  if (path.startsWith('/api/agents/') && path.endsWith('/usage')) return {days:[{day:'2026-11-03',calls:5,refused:1,errors:0,events_delivered:3,reply_chars:700,reported_cost_cents:null},{day:'2026-11-04',calls:2,refused:0,errors:0,events_delivered:9,reply_chars:41779,reported_cost_cents:1234}],refusals:{forbidden:1},last_activity_at:null,reported_cost_cents_30d:1234,cost_note:'Cost and tokens are reported by the bot itself.'};
  if (path.endsWith('/monitoring')) return new Promise((resolve,reject) => deferred.push({path,resolve,reject}));
  if (path.endsWith('/doorbell')) return {};
  if (path.endsWith('/bonds')) return {bonds: crowd ? ['Mac','Chainy','Victor Sausage','Axiom','Ron','Morgkorg','A very long bot name indeed','Aionoro','Kyle’s Agent'].map((name,i)=>({agent:{id:`x${i}`,name},strength:i+1,built:0})) : []};
  if (path.endsWith('/notify')) { if (options.method === 'PUT') return delayNotifySave ? new Promise(resolve => { delayedNotifySave = resolve; }) : {}; if (delayNotify && path.includes('/one/')) return new Promise(resolve => { delayedNotify = resolve; }); if (path.includes('/two/')) throw Error('Notify temporarily unavailable'); return {only_me:false,url:null}; }
  if (path.endsWith('/routine')) return options.method === 'PUT' ? {} : {days:['mon'],starts:'09:00',ends:'24:00',public_places:true};
  throw Error(`Unexpected ${path}`);
};
const panels = { add: (_name,_title,opts) => { options = opts; body = h('div'); return body; }, toggle: () => h('button'), open: () => options.onOpen() };
const pictures = [];
const canvas = () => ({width:0,height:0,getContext:()=>({imageSmoothingEnabled:true,drawImage(){}}),toDataURL:()=>'data:image/png;base64,HEAD'});
const figure = worn => { pictures.push(worn); return {width:26,height:36,getContext:()=>({getImageData:()=>({data:Uint8ClampedArray.from({length:26*36*4},(_,i)=>i%4===3&&i>=4*26*5?255:0)})})}; };
const listeners = {};
const context = {h,api,panels,document:{activeElement:null,body:h('body'),createElement:canvas},console,addEventListener:(type,fn)=>{listeners[type]=fn;},removeEventListener:(type,fn)=>{if(listeners[type]===fn)delete listeners[type];}};
vm.createContext(context);
vm.runInContext(fs.readFileSync('web/trust.js','utf8').replace('export function trustPanel','function trustPanel'),context);
const scheduled = [];
function build(extra = {}) { return context.trustPanel({h,api,panels,me:{name:'You',kind:'person'},crew:()=>{},schedule:(b)=>scheduled.push(b.name),...extra}); }
(async () => {
  const panel = build(); await panel.open(); await tick();
  assert.ok(text(body).includes('Fern')); assert.ok(findButton(body,'Wake-up settings'));
  // A slow settings read for one bot must not open a dialog for another bot.
  const dialogsBefore = context.document.body.children.length;
  delayNotify = true;
  const opening = findButton(body,'Wake-up settings').props.onclick(); await tick();
  await body.querySelector('[aria-label="Bot to manage"]').props.onchange({target:{value:'two'}}); await tick();
  delayedNotify({only_me:false,url:'https://fern.example/wake',trusted_bots:[]}); await opening;
  assert.equal(context.document.body.children.length,dialogsBefore,'stale settings must not open under another bot’s name');
  delayNotify = false;
  await body.querySelector('[aria-label="Bot to manage"]').props.onchange({target:{value:'one'}}); await tick();
  await findButton(body,'Wake-up settings').props.onclick();
  const boundDialog = context.document.body.children.at(-1);
  delayNotifySave = true;
  const saving = all(boundDialog).find(n=>n.tag==='form').props.onsubmit({preventDefault(){}}); await tick();
  assert.equal(requests.findLast(r=>r.method==='PUT').path,'/api/agents/one/notify');
  boundDialog.close();
  await body.querySelector('[aria-label="Bot to manage"]').props.onchange({target:{value:'two'}}); await tick();
  const healthReads = requests.filter(r=>r.path.endsWith('/monitoring')).length;
  delayedNotifySave({}); await saving;
  assert.equal(requests.filter(r=>r.path.endsWith('/monitoring')).length,healthReads,'an old save must not refresh another bot’s health');
  delayNotifySave = false;
  await body.querySelector('[aria-label="Bot to manage"]').props.onchange({target:{value:'one'}}); await tick();
  const initialHealth = deferred.at(-1);
  // Confirmed settings updates refresh health and invalidate an older in-flight snapshot.
  const pause = findButton(body,'Pause bot').props.onclick(); await tick();
  deferred.at(-1).resolve({latest_mention:null,connections:[{client_name:'Host',status:'active',scope:'agent:read',subscriptions:[{status:'active',hangout_id:null,cursor:1,last_delivery:{reason:'http_accepted',at:'2026-11-03T04:00:00Z',http_status:202}}]}],eligible_rooms:[],wake_paused:true}); await pause;
  initialHealth.resolve({connections:[],eligible_rooms:[],wake_paused:false}); await tick();
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
  deferred.findLast(r=>r.path.includes('/two/')).resolve({latest_mention:{event_id:'evt_42',recorded_at:'2026-11-03T05:00:00Z',hangout_id:'room'},connections:[{client_name:'Host',status:'active',scope:'agent:read',subscriptions:[{status:'expired',hangout_id:'room',cursor:42,last_delivery:{reason:'callback_failed',at:'2026-11-03',http_status:503}},{status:'expired',hangout_id:null,cursor:0},{status:'stopped',hangout_id:'hidden',cursor:7},{status:'expired',hangout_id:null,room_filter_hidden:true,cursor:8}]}],eligible_rooms:[{id:'room',name:'Moss room',covered:false}]}); await tick();
  findButton(body,'Monitoring').props.onclick();
  assert.ok(text(body).includes('Moss room')); assert.ok(!text(body).includes('STALE ROOM'));
  assert.ok(text(body).includes('Unknown to this server')); assert.ok(text(body).includes('No active subscription'));
  const hiddenSubscription = all(body).find(n=>n.className==='trust-subscription'&&text(n).includes('Cursor: 8'));
  assert.ok(text(hiddenSubscription).includes('Room filter: Private room (details hidden)'));
  assert.ok(!text(hiddenSubscription).includes('All eligible rooms'));
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
  permissionSelect.props.onchange({target:{value:'selected'}});
  const wakePermission = all(body).find(n=>n.className==='trust-permission');
  all(wakePermission).find(n=>n.tag==='input').props.onchange({target:{checked:true}});
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
  // The server removes inactive named grants; both the saved baseline and draft adopt that reply.
  const canonicalSelect = all(body).find(n=>n.tag==='select'&&n.props['aria-label']==='Wake it by mention');
  canonicalSelect.props.onchange({target:{value:'selected'}});
  assert.ok(all(all(body).find(n=>n.className==='trust-permission')).filter(n=>n.tag==='input').every(n=>!n.checked),'discarded grants must not reappear after saving Only me');
  canonicalSelect.props.onchange({target:{value:'owner'}});
  assert.equal(findButton(body,'Save permissions').disabled,true,'canonical policy is the saved baseline');
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
  // BT11: the graph opens fitted, with labelled zoom buttons; + then - returns to the same scale.
  const scaleOf = () => Number(all(body).find(n=>n.props.class==='trust-world').props['data-scale']);
  const inside = () => {
    const world = all(body).find(n=>n.props.class==='trust-world');
    const [ox,oy] = world.props.transform.match(/translate\(([-\d.e]+) ([-\d.e]+)\)/).slice(1).map(Number), k = scaleOf();
    return all(body).filter(n=>n.tag==='g'&&n.props.class?.includes('trust-node')).map(g=>{
      const [x,y] = g.props.transform.match(/-?[\d.]+/g).map(Number);
      // The frame with its ring, and the name under it: the whole of what is drawn for this node.
      const half = (g.props.class.includes(' you')?22:g.props.class.includes(' person')?17:g.props.class.includes(' mine')?19:16)+5;
      const label = all(g).find(n=>n.props.class==='label'); const w = [...text(label).replace(/\s+/g,' ').trim()].length*6.4+6;
      const lx = x+Number(label.props.x), ly = y+Number(label.props.y);
      const x0 = Math.min(x-half,lx-w/2), x1 = Math.max(x+half,lx+w/2), y0 = Math.min(y-half,ly-10), y1 = Math.max(y+half,ly+3);
      return {name:g.props['aria-label'], x:ox+x*k, y:oy+y*k, x0:ox+x0*k, x1:ox+x1*k, y0:oy+y0*k, y1:oy+y1*k};
    });
  };
  const wholeInView = where => { for (const p of inside()) assert.ok(p.x0>=0&&p.x1<=480&&p.y0>=0&&p.y1<=300,`${p.name} (ring and name) is inside ${where}: ${p.x0},${p.y0} to ${p.x1},${p.y1}`); };
  for (const [label,aria] of [['+','Zoom in'],['−','Zoom out'],['Fit','Fit the whole graph']]) assert.equal(findButton(body,label)?.props['aria-label'],aria,`${label} button is labelled`);
  assert.ok(placed.length>=10);
  assert.ok(scaleOf()>=0.5&&scaleOf()<=1.5,`fitted scale ${scaleOf()}`);
  wholeInView('the fitted view');
  const fitScale = scaleOf();
  findButton(body,'+').props.onclick(); assert.ok(scaleOf()>fitScale,'zoom in enlarges');
  findButton(body,'−').props.onclick(); assert.ok(Math.abs(scaleOf()-fitScale)<1e-3,'zoom out returns to the same scale');
  for (let i=0;i<30;i++) findButton(body,'+').props.onclick(); assert.equal(scaleOf(),4,'zoom stops at its top');
  for (let i=0;i<60;i++) findButton(body,'−').props.onclick(); assert.equal(scaleOf(),0.5,'zoom stops at its bottom');
  findButton(body,'Fit').props.onclick(); assert.equal(scaleOf(),fitScale,'Fit goes back to the fitted view');
  wholeInView('the view after Fit');
  // The keys do the same with the graph focused, and the wheel zooms around the pointer.
  const picture = all(body).find(n=>n.tag==='svg'&&n.props.class==='trust-graph');
  picture.props.onkeydown({key:'+',preventDefault(){}}); assert.ok(scaleOf()>fitScale);
  picture.props.onkeydown({key:'-',preventDefault(){}}); assert.ok(Math.abs(scaleOf()-fitScale)<1e-3);
  picture.props.onkeydown({key:'+',preventDefault(){}}); picture.props.onkeydown({key:'0',preventDefault(){}}); assert.equal(scaleOf(),fitScale);
  picture.getBoundingClientRect = () => ({left:0,top:0,width:480,height:300});
  // A plain wheel is left alone so the page scrolls; with Ctrl (a trackpad pinch) it zooms.
  let stopped = false;
  picture.props.onwheel({clientX:100,clientY:80,deltaY:-120,deltaMode:0,preventDefault(){stopped=true;}});
  assert.equal(stopped,false,'a plain wheel is not taken'); assert.equal(scaleOf(),fitScale,'a plain wheel does not zoom');
  picture.props.onwheel({clientX:100,clientY:80,deltaY:-120,deltaMode:0,ctrlKey:true,preventDefault(){stopped=true;}});
  assert.ok(stopped&&scaleOf()>fitScale,'Ctrl and the wheel zoom in');
  // Dragging the background pans, and the middle of the graph can't be dragged out of the picture.
  findButton(body,'Fit').props.onclick();
  const seen = () => inside().some(p=>p.x>=0&&p.x<=480&&p.y>=0&&p.y<=300);
  for (const [dx,dy] of [[5000,5000],[-5000,-5000],[5000,-5000],[-5000,5000]]) {
    picture.props.onpointerdown({pointerId:1,button:0,clientX:240,clientY:150,target:{}});
    listeners.pointermove({pointerId:1,clientX:240+dx,clientY:150+dy});
    assert.ok(seen(),`something of the graph is still in view after a drag of ${dx},${dy}`);
    listeners.pointerup({pointerId:1});
  }
  assert.equal(listeners.pointermove,undefined,'a finished pan leaves nothing listening');
  findButton(body,'Fit').props.onclick();
  // A drag in progress ends when the graph is drawn again.
  picture.props.onpointerdown({pointerId:2,button:0,clientX:240,clientY:150,target:{}});
  assert.ok(listeners.pointermove,'a pan listens');
  const first = all(body).find(n=>n.tag==='g'&&n.props.class?.includes('trust-node'));
  first.props.onpointerdown({button:0,clientX:240,clientY:150}); assert.ok(listeners.pointerup);
  data.looks.bots.one = {head:'wide'}; await busy.refreshLooks(); await tick();
  assert.equal(listeners.pointermove,undefined,'drawing again ends a drag or pan that was going on');
  assert.equal(listeners.pointerup,undefined);
  // A view zoomed by hand stays put when the same people are drawn again; a different set is fitted again.
  const redrawn = () => all(body).find(n=>n.tag==='svg'&&n.props.class==='trust-graph');
  for (let i=0;i<3;i++) findButton(body,'+').props.onclick();
  const hand = scaleOf(); assert.ok(hand>1.5,`zoomed by hand to ${hand}`);
  data.looks.bots.one = {head:'antenna'}; await busy.refreshLooks(); await tick();
  assert.equal(scaleOf(),hand,'a zoom made by hand survives a redraw');
  data.bots.push({id:'extra',name:'Zed',owner_id:'friend'}); data.friendships.push({kind:'bot',from_id:'one',to_id:'extra',status:'pending'});
  const ask = all(body).find(n=>n.tag==='select'&&n.props['aria-label']==='Bot to befriend'); ask.value = 'extra';
  await findButton(body,'Ask to be friends').props.onclick(); await tick();
  assert.ok(node('Zed'),'the new bot is drawn');
  assert.ok(scaleOf()<=1.5&&scaleOf()!==hand,`a different set of nodes is fitted again: ${scaleOf()}`);
  const refit = scaleOf(); findButton(body,'Fit').props.onclick(); assert.equal(scaleOf(),refit,'and that is the fit');
  wholeInView('the view fitted for the new set');
  crowd = false;
  // U22: a bot's owner sees the events handed to it each day beside its calls.
  const owned = build(); await owned.open(); await tick();
  findButton(body, 'Usage').props.onclick(); await tick(); await tick(); await tick();
  const ownUsage = text(body);
  assert.ok(ownUsage.includes('Events'), 'the bot table has an Events column');
  assert.ok(ownUsage.includes('2026-11-04 2 0 0 9') && ownUsage.includes('2026-11-03 5 1 0 3'), `each day carries its events: ${ownUsage}`);
  // U26: and what the bot was sent, in characters.
  assert.ok(ownUsage.includes('Sent (chars)') && ownUsage.includes('2026-11-04 2 0 0 9 41779') && ownUsage.includes('2026-11-03 5 1 0 3 700'), `each day carries what was sent: ${ownUsage}`);
  // U25: the cost the bot reported, in dollars, '-' on a day it said nothing, and the 30-day total marked as reported.
  assert.ok(ownUsage.includes('Cost (reported)'), 'the bot table has a Cost column');
  assert.ok(ownUsage.includes('2026-11-04 2 0 0 9 41779 $12.34') && ownUsage.includes('2026-11-03 5 1 0 3 700 -'), `each day carries its cost: ${ownUsage}`);
  assert.ok(ownUsage.includes('Reported cost, last 30 days: $12.34. Cost and tokens are reported by the bot itself.'), ownUsage);
  requests.length = 0;
  data.agents=[]; const empty=build(); await empty.open(); await tick();
  assert.ok(text(body).includes('Your people friendships')); assert.ok(findButton(body,'Accept')); assert.ok(findButton(body,'Open your crew'));
  // U21: someone who runs the server reaches the Usage tab with no bots; anyone else still has no tabs.
  assert.equal(findButton(body, 'Usage'), undefined);
  // S86: hosted in the My bots view, a person who owns none of the bots sees the same: no Connections,
  // Monitoring or Usage tabs, and nothing about another person's bot (Pine is a friend's).
  const host = h('section'); const hostedBody = () => host;
  const hosted = build({host, hostPanel:'bots'}); hosted.load(); await tick(); await tick();
  assert.equal(hosted.toggle, null, 'no button of its own');
  for (const label of ['Connections', 'Monitoring', 'Usage', 'Pause bot', 'Wake-up settings']) assert.equal(findButton(hostedBody(), label), undefined, `${label} shown to someone with no bots`);
  assert.ok(!text(hostedBody()).includes('Pine'), "another person's bot is not shown");
  assert.ok(text(hostedBody()).includes('Your people friendships'), 'the same empty state as the panel');
  gods = true; const operator = build({me:{name:'You',kind:'person',god_mode:false}}); await operator.open(); await tick();
  const usageTab = findButton(body, 'Usage'); assert.ok(usageTab, 'an operator with no bots has the Usage tab');
  usageTab.props.onclick(); await tick(); await tick();
  const shown = text(body);
  assert.ok(shown.includes('Median ms') && shown.includes('Last 24 hours') && shown.includes('Events delivered 7') && shown.includes('webhook POSTs 4 (1 failed)'));
  assert.ok(!requests.some(r => r.path.includes('/agents/') && r.path.endsWith('/usage')), 'no bot, so no bot usage is asked for');
  console.log('Trust UI selection, stale requests, permissions, friendship consent and empty states passed');
})().catch(error=>{console.error(error);process.exitCode=1;});
