// Exercise the shipped Needs You card handlers, not copied implementations.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const source = fs.readFileSync('web/app.js', 'utf8');
function extract(name, nested = false) {
  const indent = nested ? '  ' : '';
  const start = source.indexOf(`${indent}function ${name}(`);
  assert.ok(start >= 0);
  return source.slice(start, source.indexOf(`\n${indent}}\n`, start) + indent.length + 2);
}
const calls = [];
const h = (tag, props = {}, ...children) => ({tag, props, children: children.flat(Infinity).filter(x => x != null)});
const context = {
  h, item: (...children) => h('article', {}, ...children),
  formError: () => h('span'), roomLink: room => room.name,
  whenText: value => `time:${value}`, durationText: value => `${value} minutes`,
  act: (_error, run) => run, done: run => run, refresh: async () => {},
  api: async (path, options) => { calls.push({path, ...options}); },
  yesNo: (_error, path) => h('button', {onclick: () => context.api(path, {method:'POST', body:{response:'yes'}})}, 'Yes'),
  CHECK_IN_ENDS: {go:'said go', hold:'said hold'},
};
vm.createContext(context);
vm.runInContext([extract('passOffer'), ...['passItem','sceneItem','answeredCheckIn','requestItem'].map(n => extract(n,true))].join('\n'), context);
const all = node => typeof node === 'object' ? [node, ...node.children.flatMap(all)] : [];
const text = node => typeof node === 'object' ? node.children.map(text).join(' ') : String(node);
const button = (node, label) => all(node).find(n => n.tag === 'button' && text(n) === label);
(async () => {
  const scene = context.sceneItem({id:'scene-1', scene_name:'Movie', house_id:'house-1', house_name:'Ann home', asked_by:'Fern', expires_at:99});
  for (const expected of ['Ann home','Fern','Movie','time:99','house AI']) assert.ok(text(scene).includes(expected));
  await button(scene,'Yes').props.onclick();
  assert.equal(calls.at(-1).path, '/api/scene-requests/scene-1/answer');
  const pass = context.passItem({id:'pass-1', house_id:'house-2', house_name:'Bo home', agent_name:'Fern', granted_by:'Bo', max_minutes:45});
  for (const expected of ['Bo home','Fern','Bo','45 minutes','no one to ask','cancel']) assert.ok(text(pass).includes(expected));
  await button(pass,'No thanks').props.onclick();
  assert.equal(calls.at(-1).path, '/api/passes/pass-1');
  assert.equal(calls.at(-1).method, 'DELETE');
  await button(pass,'Accept').props.onclick();
  assert.equal(calls.at(-1).path, '/api/passes/pass-1/accept');
  const check = {id:'check-1',status:'go',asker:'Fern',about:'Do it',answered_by:'Spruce',room:{name:'Room'},can_answer:true,handled_at:100};
  const card = context.answeredCheckIn(check);
  assert.ok(button(card,'Go'));
  await button(card,'Hold').props.onclick();
  assert.equal(calls.at(-1).path, '/api/check-ins/check-1/answer');
  assert.equal(calls.at(-1).body.response, 'hold');
  assert.equal(button(context.answeredCheckIn({...check,can_answer:false}),'Hold'), undefined);
  // PJ21, PJ22, PJ24: a join request says who asks to join where, and answers on the floor's route.
  const join = context.requestItem({id:'join-1', request:'join', floor_id:'floor-1', place:'Porch', asked_by:'Ann', bot:null, note:'I fix docs'});
  for (const expected of ['Ann','asks to join','Porch','I fix docs']) assert.ok(text(join).includes(expected));
  assert.ok(!text(join).includes('let'));
  await button(join,'Yes').props.onclick();
  assert.equal(calls.at(-1).path, '/api/floors/floor-1/join_requests/join-1');
  assert.equal(calls.at(-1).body.approve, true);
  await button(join,'No').props.onclick();
  assert.equal(calls.at(-1).body.approve, false);
  const botJoin = context.requestItem({id:'join-2', request:'join', floor_id:'floor-1', place:'Porch', asked_by:'Ann', bot:'Fern', note:''});
  assert.ok(text(botJoin).includes('asks for Fern to join'));
  // S51: a request to let a bot in is unchanged.
  const letIn = context.requestItem({id:'req-1', asked_by:'Bo', bot:'Moss', place:'Den'});
  assert.ok(text(letIn).includes('asks to let Moss into Den'));
  await button(letIn,'Let in').props.onclick();
  assert.equal(calls.at(-1).path, '/api/place-requests/req-1/answer');
  console.log('Needs You consent context and override handlers passed');
})().catch(error => {console.error(error); process.exitCode = 1;});
