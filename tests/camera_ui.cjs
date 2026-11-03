const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict');
const source=fs.readFileSync('web/app.js','utf8');const start=source.indexOf('  async function cameraPermissionDialog(');assert.ok(start>=0);const end=source.indexOf('\n  }\n',start)+4;
let closeCount=0,fail=false,form,box,calls=[];
const h=(tag,props={},...children)=>{const el={tag,...props,children};if(tag==='form')form=el;if(tag==='input')box=el;return el};
const c={h,formError:()=>({}),submitter:(_error,run)=>run,modal:()=>({close(){closeCount++}}),api:async(path,options)=>{calls.push({path,options});if(options&&fail)throw Error('refused');return {allowed:true}}};
vm.createContext(c);vm.runInContext(source.slice(start,end),c);
(async()=>{await c.cameraPermissionDialog({id:'bot-one',name:'Bot'});assert.equal(box.checked,true);box.checked=false;await form.onsubmit();assert.equal(calls.at(-1).path,'/api/agents/bot-one/camera');assert.equal(calls.at(-1).options.body.allowed,false);assert.equal(closeCount,1);await c.cameraPermissionDialog({id:'bot-one',name:'Bot'});fail=true;await assert.rejects(form.onsubmit());assert.equal(closeCount,1);console.log('Camera consent UI: on by default, turning off saves for the correct bot, and failed-save stays open passed');})().catch(e=>{console.error(e);process.exitCode=1});
