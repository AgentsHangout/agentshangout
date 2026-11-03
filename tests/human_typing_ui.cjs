const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict');
const source = fs.readFileSync('web/app.js','utf8');
function extract(name) { const start=source.indexOf(`function ${name}(`); assert.ok(start>=0); return source.slice(start,source.indexOf('\n}\n',start)+2); }
let now=10000, next=1; const timers=new Map();
const input={value:'secret draft',events:{},addEventListener(k,v){this.events[k]=v},removeEventListener(k){delete this.events[k]}};
const document={hidden:false,events:{},addEventListener(k,v){this.events[k]=v},removeEventListener(k){delete this.events[k]}};
const c={document,Date:{now:()=>now},setTimeout:fn=>{let id=next++;timers.set(id,fn);return id},clearTimeout:id=>timers.delete(id),setInterval:fn=>{let id=next++;timers.set(id,fn);return id},clearInterval:id=>timers.delete(id)};
vm.createContext(c);vm.runInContext(extract('humanTypingComposer')+'\n'+extract('humanTypingHints'),c);
const flush=async()=>{for(let n=0;n<12;n++)await Promise.resolve()};
(async()=>{
 const sent=[];let release;
 const control=c.humanTypingComposer(input,on=>{sent.push(on);if(sent.length===1)return new Promise(r=>release=r)});
 input.events.input(); await flush(); assert.deepEqual(sent,[true]);
 input.events.input();await flush();assert.deepEqual(sent,[true]);
 control.destroy();await flush();assert.deepEqual(sent,[true]);release();await flush();assert.deepEqual(sent,[true,false]);
 assert.equal(input.events.input,undefined);assert.equal(document.events.visibilitychange,undefined);
 const control2=c.humanTypingComposer(input,on=>sent.push(on));input.events.input();await flush();
 for(const fn of [...timers.values()])fn();await flush();assert.equal(sent.at(-1),false);
 document.hidden=true;input.events.input();await flush();assert.equal(sent.at(-1),false);control2.destroy();document.hidden=false;
 const skipped=[];const control3=c.humanTypingComposer(input,on=>skipped.push(on));input.events.input();control3.stop();await flush();assert.deepEqual(skipped,[false]);control3.destroy();
 const status={};const hints=c.humanTypingHints(status,'me');hints.update([{person_id:'me',name:'Me',until:20},{person_id:'other',name:'Bo',until:20}]);
 assert.equal(status.textContent,'Bo is typing…');const revision=hints.version();hints.clear('other');hints.fromSnapshot([{person_id:'other',name:'Bo',until:20}],revision);assert.equal(status.hidden,true);hints.update([{person_id:'other',name:'Bo',until:20}]);now=20000;for(const fn of [...timers.values()])fn();assert.equal(status.hidden,true);hints.destroy();
 assert.ok(sent.every(value=>typeof value==='boolean'));
 console.log('Human typing: throttle, serialized stop, idle, hidden, cleanup, expiry, and no draft payload passed.');
})().catch(error=>{console.error(error);process.exitCode=1});
