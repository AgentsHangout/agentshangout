// Trusted, bounded Canvas-to-SVG adapter for the server camera. No DOM, network, or user code.
// The drawing functions above come unchanged from room.js; this adds labels to the still.
const cameraEscape = (text) => String(text).replace(/[^\u0009\u000A\u000D\u0020-\uD7FF\uE000-\uFFFD\u{10000}-\u{10FFFF}]/gu, '\uFFFD').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&apos;'}[c]));
class CameraCanvas {
  constructor() { this.parts=[];this.defs=[];this.fillStyle='#000';this.globalAlpha=1;this.matrix=[1,0,0,1,0,0];this.stack=[];this.path=[]; }
  add(text) { if(this.parts.length>=40000)throw Error('camera drawing limit');this.parts.push(text); }
  style() { const fill=typeof this.fillStyle==='string'?this.fillStyle:`url(#${this.fillStyle.id})`;return `fill="${cameraEscape(fill)}" opacity="${Math.max(0,Math.min(1,this.globalAlpha))}" transform="matrix(${this.matrix.join(' ')})"`; }
  fillRect(x,y,w,h) { if(w<0){x+=w;w=-w}if(h<0){y+=h;h=-h}this.add(`<rect x="${x}" y="${y}" width="${w}" height="${h}" ${this.style()}/>`); }
  save() { this.stack.push({matrix:[...this.matrix],fillStyle:this.fillStyle,globalAlpha:this.globalAlpha}); }
  restore() { const saved=this.stack.pop();if(saved)Object.assign(this,saved); }
  multiply(n) { const [a,b,c,d,e,f]=this.matrix,[g,h,i,j,k,l]=n;this.matrix=[a*g+c*h,b*g+d*h,a*i+c*j,b*i+d*j,a*k+c*l+e,b*k+d*l+f]; }
  translate(x,y) { this.multiply([1,0,0,1,x,y]); }
  scale(x,y) { this.multiply([x,0,0,y,0,0]); }
  rotate(t) { this.multiply([Math.cos(t),Math.sin(t),-Math.sin(t),Math.cos(t),0,0]); }
  beginPath() { this.path=[]; }
  arc(x,y,r,start,end) { this.ellipse(x,y,r,r,0,start,end); }
  ellipse(x,y,rx,ry,rotation,start,end) { if(Math.abs(end-start-Math.PI*2)>0.001||rotation!==0)throw Error('unsupported camera path');this.path.push(`<ellipse cx="${x}" cy="${y}" rx="${rx}" ry="${ry}" ${this.style()}/>`); }
  fill() { for(const part of this.path)this.add(part); }
  createLinearGradient(x1,y1,x2,y2) { const id=`camera-gradient-${this.defs.length}`;const stops=[];this.defs.push({id,x1,y1,x2,y2,stops});return {id,addColorStop(at,color){stops.push({at,color})}}; }
  label(name,x,y,maxChars) {
    const lines=[];let line='';
    for(const word of String(name).split(/\s+/u)) {
      const chars=[...word];
      while(chars.length>maxChars) { if(line){lines.push(line);line='';}lines.push(chars.splice(0,maxChars).join('')); }
      const rest=chars.join('');
      if([...line].length+[...rest].length+1>maxChars&&line){lines.push(line);line='';}
      line+=(line?' ':'')+rest;
    }
    if(line||!lines.length)lines.push(line);
    const width=Math.max(...lines.map(l=>[...l].length),1)*5.5+8;
    this.add(`<rect x="${x-width/2}" y="${y-6}" width="${width}" height="${lines.length*7+3}" rx="2" fill="#182033" opacity="0.94"/>`);
    lines.forEach((line,i)=>this.add(`<text x="${x}" y="${y+i*7}" font-family="DejaVu Sans" font-size="5.5" text-anchor="middle" fill="#ffffff">${cameraEscape(line)}</text>`));
  }
  svg(frame) {
    const defs=this.defs.map(d=>`<linearGradient id="${d.id}" gradientUnits="userSpaceOnUse" x1="${d.x1}" y1="${d.y1}" x2="${d.x2}" y2="${d.y2}">${d.stops.map(s=>`<stop offset="${s.at}" stop-color="${cameraEscape(s.color)}"/>`).join('')}</linearGradient>`).join('');
    return `<svg xmlns="http://www.w3.org/2000/svg" width="960" height="504" viewBox="${frame.x} ${frame.y} ${frame.width} ${frame.height}"><defs>${defs}</defs><g shape-rendering="crispEdges">${this.parts.join('')}</g></svg>`;
  }
}
// The room's items as the page has them, with the spots bots use them from.
function cameraPlaced(scene) {
  const placed=scene.items.filter(i=>Object.hasOwn(ITEMS,i.item)).map(standing);
  for(const item of placed)item.spots=spotsOf(item);
  return placed;
}
// #344, CP8, CP9: each bot is where the page puts it, if that spot is free and inside the frame: one
// working through another tool at a desk, one using an item at it, and an active one (or a working
// one with no desk) in a seat at the table. Bots come sorted by id, so the same room always gives
// the same places. Everyone else stands in one grid, and CP2 puts the people who had the floor open
// in it with the bots.
function cameraFigures(scene,placed) {
  const frame=scene.frame,taken=new Set(),look=scene.look;
  // A whole figure has to fit, so a spot right at the frame's edge doesn't count.
  const inFrame=s=>s.x>=frame.x+10&&s.x<=frame.x+frame.width-10&&s.y>=frame.y+24&&s.y<=frame.y+frame.height-10;
  const claim=list=>{const spot=(list||[]).find(s=>!taken.has(s)&&inFrame(s));if(spot)taken.add(spot);return spot||null;};
  const seats=shiftedSeats(tableShift(look.table));
  const placeBot=b=>{
    let spot=null,at=null;
    // As on the page: the desk, or with the desk hidden, whatever serves as one (I66).
    if(b.working){spot=claim((look.hidden||[]).includes('desk')?deskSpotsOf(placed):DESKS);if(spot)at='desk';}
    if(!spot&&b.using){spot=claim(placed.find(i=>i.id===b.using)?.spots);if(spot)at='item';}
    if(!spot&&(b.state==='active'||b.working)){spot=claim(seats);if(spot)at='table';}
    return {spot,at};
  };
  const who=[...scene.bots.map(b=>({...b,person:false,...placeBot(b)})),...(scene.people||[]).map(p=>({...p,person:true,spot:null,at:null}))];
  const loose=who.filter(f=>!f.spot);
  const cols=Math.min(frame.width<480?3:4,Math.max(1,loose.length)),rows=Math.ceil(loose.length/cols);
  return who.map(f=>{
    const worn=f.person?{look:personLook(f.look)}:{wears:botLook(f.look)};
    if(f.spot)return {...f,...worn,path:[],pose:f.spot.pose,facing:f.spot.facing||'front',phase:0,x:f.spot.x,y:f.spot.y};
    const i=loose.indexOf(f);
    return {...f,...worn,path:[],pose:'stand',facing:'front',phase:0,
      x:frame.x+frame.width*(0.08+(i%cols+0.5)*0.84/cols),
      y:frame.y+frame.height*0.43+(Math.floor(i/cols)+0.5)*(frame.height*0.43)/Math.max(1,rows)};
  });
}
function cameraSvg(scene) {
  const g=new CameraCanvas(),look=scene.look,frame=scene.frame;
  paintRoom(g,12,look);paintBehind(g,look.hidden,0);
  const placed=cameraPlaced(scene);
  for(const item of placed)if(item.flat)paintItem(g,item,0);
  const figures=cameraFigures(scene,placed);
  const layers=[...furnitureScene(g,look.hidden,0,[],[],look.table),
    ...placed.filter(i=>!i.flat).map(i=>({y:i.y,draw:()=>paintItem(g,i,0)})),
    ...figures.map(f=>({y:f.spot?.over||f.y,draw:()=>f.person?drawPerson(g,f,0):drawBot(g,f,0)}))];
  layers.sort((a,b)=>a.y-b.y);for(const layer of layers)layer.draw();
  for(const f of figures)f.spot?.busy?.(g,f,0);
  for(const f of figures)g.label(f.name,f.x,f.y+8,frame.width<480?14:16);
  // Where each bot was drawn, so the photo's metadata says only what the picture shows.
  const where=at=>figures.filter(f=>!f.person&&f.at===at).map(f=>f.id);
  return {svg:g.svg(frame),at:where('item'),table:where('table'),desk:where('desk')};
}
JSON.stringify(cameraSvg(JSON.parse(__camera_scene)));
