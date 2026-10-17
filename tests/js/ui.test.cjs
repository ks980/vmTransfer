const {test}=require('node:test');
const assert=require('node:assert/strict');
const vm=require('node:vm');
const fs=require('node:fs');
const path=require('node:path');
const {FakeClock}=require('../fake-clock.cjs');
const {cycleFrames}=require('../../sender/session/transfer.js');
const sender=path.resolve(__dirname,'../../sender');

function prepared(marker=255,readFrame) {
  return {totalBlocks:2,metadataFrame:new Uint8Array([marker]),metadata:{sha256:new Uint8Array(32)},
    readFrame:readFrame||((index)=>Promise.resolve(new Uint8Array([index])))};
}
function harness(prepare=async()=>prepared()) {
  const clock=new FakeClock(),elements={},draws=[],handlers={};
  for(const id of [...fs.readFileSync(path.join(sender,'index.html'),'utf8').matchAll(/id="([^"]+)"/g)].map(m=>m[1])) {
    elements[id]={value:'',files:[],textContent:'',disabled:false,hidden:false,events:{},
      addEventListener(name,fn){this.events[name]=fn;}};
  }
  elements.matrix.value='128';elements.rate.value='5';elements.scale.value='4';
  elements.canvas.getContext=()=>({fillRect:()=>draws.push({clear:true}),fillStyle:''});
  const document={hidden:false,getElementById:id=>{assert.ok(elements[id],id);return elements[id];},
    addEventListener:(event,fn)=>{handlers[event]=fn;}};
  const context={document,AbortController,Uint8Array,Uint8ClampedArray,Promise,
    performance:{now:clock.now},setTimeout:clock.setTimeout,clearTimeout:clock.clearTimeout,
    VMT:{transfer:{prepareTransfer:prepare,fileSource:file=>file,cycleFrames},
      render:{renderFrame:(bytes,scale)=>({bytes,scale}),drawCanvas:(canvas,image)=>draws.push({bytes:Array.from(image.bytes),scale:image.scale})}}};
  context.window=context;context.addEventListener=(event,fn)=>{handlers[event]=fn;};vm.createContext(context);
  for(const file of ['player/player.js','ui/app.js'])vm.runInContext(fs.readFileSync(path.join(sender,file),'utf8'),context,{filename:file});
  const choose=file=>{elements.file.files=[file];elements.file.value=file.name;return elements.file.events.change();};
  return {clock,elements,draws,handlers,document,choose};
}

test('UI prepares, plays, pauses, resumes, changes rate/scale and stops',async()=>{
  let preparations=0;const h=harness(async(source,options)=>{preparations++;options.onProgress(source.size,source.size);return prepared();});
  const {elements:e,clock,draws}=h;
  assert.ok(e.start.disabled);await h.choose({name:'binary.bin',size:4029});assert.equal(e.start.disabled,false);
  assert.match(e['file-info'].textContent,/binary.bin/);assert.match(e.status.textContent,/准备完成/);
  await e.start.onclick();await clock.advance(200);assert.deepEqual(draws.filter(d=>d.bytes).map(d=>d.bytes[0]),[255,0]);
  assert.ok(e.start.disabled);assert.equal(e.pause.disabled,false);assert.match(e.progress.textContent,/第 1 轮/);
  e.pause.onclick();const count=draws.length;await clock.advance(1000);assert.equal(draws.length,count);
  assert.equal(e.start.textContent,'继续播放');await e.start.onclick();await clock.advance(0);
  assert.equal(draws.at(-1).bytes[0],1);
  e.rate.value='10';e.rate.onchange();assert.match(e.timing.textContent,/目标 10 FPS/);
  e.scale.value='6';e.scale.onchange();assert.equal(draws.at(-1).scale,6);
  e.scale.value='1.5';e.scale.onchange();assert.equal(e.scale.value,'6');
  e.stop.onclick();assert.equal(draws.at(-1).clear,true);assert.equal(e.start.disabled,false);
  await e.start.onclick();await clock.advance(0);assert.equal(draws.at(-1).bytes[0],255);assert.equal(preparations,1);
  e.stop.onclick();
});
test('reselecting a file cancels preparation and ignores its late result',async()=>{
  let oldResolve,oldSignal;
  const h=harness((source,options)=>{
    if(source.name==='old.bin'){oldSignal=options.signal;return new Promise(resolve=>{oldResolve=resolve;});}
    return Promise.resolve(prepared(200));
  });
  const previous=h.choose({name:'old.bin',size:9});assert.equal(h.elements.cancel.disabled,false);
  await h.choose({name:'new.bin',size:3});assert.equal(oldSignal.aborted,true);
  oldResolve(prepared(99));await previous;
  assert.match(h.elements['file-info'].textContent,/new.bin/);await h.elements.start.onclick();await h.clock.advance(0);
  assert.equal(h.draws.at(-1).bytes[0],200);h.elements.stop.onclick();
});
test('new selection prevents old in-flight data from overwriting its canvas',async()=>{
  let oldResolve,oldSignal;
  const h=harness(async source=>source.name==='old.bin'?prepared(255,(index,signal)=>{
    oldSignal=signal;return new Promise(resolve=>{oldResolve=resolve;});
  }):prepared(200));
  await h.choose({name:'old.bin',size:4});await h.elements.start.onclick();await h.clock.advance(0);
  await h.choose({name:'new.bin',size:5});assert.equal(oldSignal.aborted,true);
  await h.elements.start.onclick();await h.clock.advance(0);
  oldResolve(new Uint8Array([99]));await h.clock.advance(0);
  assert.equal(h.draws.at(-1).bytes[0],200);assert.match(h.elements['file-info'].textContent,/new.bin/);
  h.elements.stop.onclick();
});
test('cancelled preparation resets selection and leaves start disabled',async()=>{
  const h=harness((source,options)=>new Promise((resolve,reject)=>{
    options.signal.addEventListener('abort',()=>{const error=Error('cancel');error.name='AbortError';reject(error);});
  }));
  const changing=h.choose({name:'cancel.bin',size:5});h.elements.cancel.onclick();await changing;
  assert.equal(h.elements.file.value,'');assert.ok(h.elements.start.disabled);assert.ok(h.elements.cancel.disabled);
  assert.match(h.elements.status.textContent,/已取消/);
});
test('background notice and pagehide keep a stopped player reusable on return',async()=>{
  const h=harness();await h.choose({name:'same.bin',size:5});await h.elements.start.onclick();await h.clock.advance(0);
  h.document.hidden=true;h.handlers.visibilitychange();assert.equal(h.elements['visibility-notice'].hidden,false);
  h.document.hidden=false;h.handlers.visibilitychange();assert.equal(h.elements['visibility-notice'].hidden,true);
  h.handlers.pagehide();assert.equal(h.clock.jobs.size,0);assert.equal(h.elements.start.disabled,false);
  await h.elements.start.onclick();await h.clock.advance(0);assert.match(h.elements.status.textContent,/正在循环播放/);
  h.elements.stop.onclick();
});
test('preparation and playback errors display messages without starting another transfer',async()=>{
  const h=harness(async()=>{throw Error('cannot read file');});await h.choose({name:'bad.bin',size:5});
  assert.match(h.elements.status.textContent,/cannot read file/);assert.ok(h.elements.start.disabled);
  const other=harness(async()=>prepared(255,()=>Promise.reject(Error('lost file'))));
  await other.choose({name:'bad.bin',size:5});await other.elements.start.onclick();await other.clock.advance(0);
  assert.match(other.elements.status.textContent,/lost file/);assert.ok(other.elements.start.disabled);assert.equal(other.elements.file.value,'');
});
test('matrix selector prepares the chosen profile and locks during playback/preparation',async()=>{
  const sizes=[];let release;
  const h=harness(async(source,options)=>{
    sizes.push(options.matrixSize);
    if(options.matrixSize===256)await new Promise(resolve=>{release=resolve;});
    return {...prepared(),matrixSize:options.matrixSize};
  });
  const e=h.elements;e.matrix.value='160';await e.matrix.onchange();
  assert.equal(e.canvas.width,192*4);assert.equal(e.matrix.disabled,false);
  await h.choose({name:'matrix.bin',size:5});assert.deepEqual(sizes,[160]);
  await e.start.onclick();await h.clock.advance(0);assert.equal(e.matrix.disabled,true);
  e.pause.onclick();assert.equal(e.matrix.disabled,true);
  e.stop.onclick();assert.equal(e.matrix.disabled,false);
  e.matrix.value='256';const preparing=e.matrix.onchange();assert.equal(e.matrix.disabled,true);
  release();await preparing;assert.deepEqual(sizes,[160,256]);
  assert.equal(e.matrix.disabled,false);assert.match(e['file-info'].textContent,/256×256/);
  await e.start.onclick();await h.clock.advance(0);e.stop.onclick();assert.equal(e.canvas.width,288*4);
});
