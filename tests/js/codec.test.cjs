const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const vm=require('node:vm');
const crypto=require('node:crypto');
const codec=require('../../sender/protocol/codec.js');
const render=require('../../sender/render/canvas.js');
const transfer=require('../../sender/session/transfer.js');
const vectors=path.resolve(__dirname,'../../protocol/vectors');
const cases=JSON.parse(fs.readFileSync(path.join(vectors,'cases.json')));
const read=p=>new Uint8Array(fs.readFileSync(path.join(vectors,p)));
const metadata=c=>({id:new Uint8Array(Buffer.from(c.id,'hex')),name:c.filename,matrixSize:c.matrix_size||128,fileSize:c.size,
  sha256:new Uint8Array(Buffer.from(c.sha256,'hex')),...(c.color_mode?{colorMode:c.color_mode}:{})});

test('CRC standard vector and symbol bit order',()=>{
  assert.equal(codec.crc32(Buffer.from('123456789')),0xcbf43926);
  const data=new Uint8Array(4096);data[0]=0x1b;
  const symbols=codec.packSymbols(data);assert.deepEqual(Array.from(symbols.slice(0,4)),[0,1,2,3]);
  assert.deepEqual(codec.unpackSymbols(symbols),data);
  symbols[0]=4;assert.throws(()=>codec.unpackSymbols(symbols));
});
for(const c of cases) test('independent Python golden: '+c.case,()=>{
  const meta=metadata(c),input=read(c.input);
  assert.equal(crypto.createHash('sha256').update(input).digest('hex'),c.sha256);
  assert.deepEqual(codec.encodeMetadata(meta),read(c.frames[0]));
  assert.deepEqual(codec.decodeFrame(read(c.frames[0])).meta,meta);
  c.frames.slice(1).forEach((p,i)=>{
    const blockSize=codec.layout(meta.matrixSize,meta.colorMode).blockSize;
    const encoded=codec.encodeData(meta,i,input.subarray(i*blockSize,(i+1)*blockSize));
    assert.deepEqual(encoded,read(p));assert.equal(codec.decodeFrame(encoded).index,i);
  });
});
test('malformed headers, CRC, padding and lengths are rejected',()=>{
  const original=read(cases[5].frames[1]);
  for(const offset of [0,5,8,24,28,32,34,64,4091,4092]) {
    const bad=original.slice();bad[offset]^=1;assert.throws(()=>codec.decodeFrame(bad),/CRC/);
  }
  for(const [offset,value] of [[4,2],[5,3],[6,2],[7,1],[34,1],[32,255],[33,255]]) {
    const bad=original.slice();bad[offset]=value;new DataView(bad.buffer).setUint32(4092,codec.crc32(bad.subarray(0,4092)),true);
    assert.throws(()=>codec.decodeFrame(bad));
  }
  assert.throws(()=>codec.encodeData(metadata(cases[5]),2,new Uint8Array(1)));
  assert.throws(()=>codec.encodeMetadata({...metadata(cases[0]),fileSize:2**30+1}));
  assert.throws(()=>codec.encodeMetadata({...metadata(cases[0]),name:'\ud800'}));
});
test('raster has fixed logical geometry and independent physical scale',()=>{
  const image=render.renderFrame(read(cases[0].frames[0]),3);
  assert.equal(image.width,480);assert.equal(image.data.length,480*480*4);
  const rgb=(x,y)=>Array.from(image.data.slice((y*image.width+x)*4,(y*image.width+x)*4+3));
  assert.deepEqual(rgb(0,0),[255,255,255]);assert.deepEqual(rgb(12,12),[0,0,0]);
  assert.deepEqual(rgb(100,450),[255,0,0]);
  assert.throws(()=>render.renderFrame(read(cases[0].frames[0]),1.5));
});
test('UTF-8 names preserve BOM and combining characters without normalization',()=>{
  for(const name of ['\ufeffname.bin','e\u0301.bin','😀.bin']) {
    const meta={...metadata(cases[0]),name};
    assert.equal(codec.decodeFrame(codec.encodeMetadata(meta)).meta.name,name);
  }
});
test('metadata parsing accepts Buffer and Uint8Array views with a nonzero offset',()=>{
  const expected=metadata(cases[8]),encoded=codec.encodeMetadata(expected);
  const buffer=Buffer.alloc(5000);buffer.set(encoded,123);
  assert.deepEqual(codec.decodeFrame(buffer.subarray(123,4219)).meta,expected);
  assert.deepEqual(codec.decodeFrame(new Uint8Array(buffer.buffer,buffer.byteOffset+123,4096)).meta,expected);
});
test('browser SHA implementation is checked without Node crypto fallback',()=>{
  const context={window:{},Uint8Array,ArrayBuffer,DataView};vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.resolve(__dirname,'../../sender/vendor/sha256.js'),'utf8'),context);
  for(const length of [0,1,55,56,63,64,65,127,128,1000000]) {
    const data=Buffer.alloc(length,0x61),hash=context.window.sha256.create();
    for(let offset=0;offset<length;offset+=37)hash.update(new Uint8Array(data.subarray(offset,offset+37)));
    assert.equal(hash.hex(),crypto.createHash('sha256').update(data).digest('hex'));
  }
});
test('preparation is bounded, cancellable, and frames stay identical across cycles',async()=>{
  const data=Buffer.alloc(2*1024*1024+13,0x80),reads=[];
  const source={name:'C:\\some\\binary.bin',size:data.length,async readAt(offset,length){reads.push(length);return new Uint8Array(data.subarray(offset,offset+length));}};
  const id=new Uint8Array(16),progress=[];
  const prepared=await transfer.prepareTransfer(source,{id,onProgress:(done)=>progress.push(done)});
  assert.equal(prepared.metadata.name,'binary.bin');assert.ok(Math.max(...reads)<=1024*1024);
  assert.equal(progress.at(-1),data.length);
  assert.equal(Buffer.from(prepared.metadata.sha256).toString('hex'),crypto.createHash('sha256').update(data).digest('hex'));
  assert.deepEqual(await prepared.readFrame(0),await prepared.readFrame(0));
  const cycle=Array.from(transfer.cycleFrames(130));
  assert.deepEqual(cycle.filter(f=>f.type==='metadata').map(f=>cycle.indexOf(f)),[0,65,130]);
  assert.deepEqual(cycle,Array.from(transfer.cycleFrames(130)));
  assert.equal(Array.from(transfer.cycleFrames(0)).length,1);
  const abort=new AbortController();
  await assert.rejects(transfer.prepareTransfer(source,{id,signal:abort.signal,onProgress:done=>{if(done)abort.abort();}}),{name:'AbortError'});
  await assert.rejects(transfer.prepareTransfer({...source,readAt:async()=>new Uint8Array(0)},{id}),/short file read/);
});
test('browser globals compose without CommonJS or Node hash APIs',async()=>{
  const context={Uint8Array,Uint8ClampedArray,ArrayBuffer,DataView,TextEncoder,TextDecoder,
    crypto:crypto.webcrypto,AbortController,setTimeout};
  context.window=context;vm.createContext(context);
  for(const file of ['vendor/sha256.js','protocol/codec.js','render/canvas.js','session/transfer.js']) {
    vm.runInContext(fs.readFileSync(path.resolve(__dirname,'../../sender',file),'utf8'),context,{filename:file});
  }
  const input=new Uint8Array([0,255,128,0]);
  const prepared=await context.VMT.transfer.prepareTransfer({name:'browser.bin',size:4,
    async readAt(offset,length){return input.slice(offset,offset+length);}});
  assert.equal(prepared.metadata.id.length,16);
  assert.equal(Buffer.from(prepared.metadata.sha256).toString('hex'),crypto.createHash('sha256').update(input).digest('hex'));
  const encoded=await prepared.readFrame(0);
  assert.deepEqual(codec.decodeFrame(encoded).payload,input);
  assert.equal(context.VMT.render.renderFrame(encoded,3).width,480);
});
test('profiles reject mismatched header/length/capacity and unsupported dimensions',()=>{
  for(const p of codec.PROFILES) {
    const meta={...metadata(cases[1]),matrixSize:p.matrixSize};
    const frame=codec.encodeMetadata(meta);
    assert.equal(frame.length,p.frameSize);assert.equal(frame[6],p.id);
    assert.deepEqual(codec.unpackSymbols(codec.packSymbols(frame)),frame);
    const bad=frame.slice();bad[6]=p.id===1?2:1;
    new DataView(bad.buffer).setUint32(bad.length-4,codec.crc32(bad.subarray(0,bad.length-4)),true);
    assert.throws(()=>codec.decodeFrame(bad),/header/);
    assert.equal(render.renderFrame(frame,2).width,p.logicalWidth*2);
    assert.equal(render.renderFrame(frame,2).height,p.logicalHeight*2);
  }
  assert.throws(()=>codec.encodeMetadata({...metadata(cases[1]),matrixSize:192}),/matrix size/);
});
test('wide profile uses CRC-covered high payload length without changing older profiles',()=>{
  const profile=codec.profileForSize('1028x512');
  assert.equal(profile.blockSize,131516);
  const c=cases.find(c=>c.case==='profile-1028x512-partial');
  const full=read(c.frames[1]);
  assert.equal(full[34],2);assert.equal(full[35],0);
  assert.equal(codec.decodeFrame(full).payload.length,131516);
  const short=full.slice();short[34]=0;
  new DataView(short.buffer).setUint32(short.length-4,codec.crc32(short.subarray(0,short.length-4)),true);
  assert.throws(()=>codec.decodeFrame(short),/padding/);
  const old=read(cases.find(c=>c.case==='profile-512-boundary').frames[1]);
  old[34]=1;
  new DataView(old.buffer).setUint32(old.length-4,codec.crc32(old.subarray(0,old.length-4)),true);
  assert.throws(()=>codec.decodeFrame(old),/header/);
});
test('512x256 reserves the high length bytes and uses profile ID 6',()=>{
  const p=codec.profileForSize('512x256');
  assert.equal(p.id,6);assert.equal(p.frameSize,32768);assert.equal(p.blockSize,32700);
  const frame=read(cases.find(c=>c.case==='profile-512x256-boundary').frames[1]);
  assert.equal(frame[6],6);assert.equal(frame[34],0);assert.equal(frame[35],0);
  assert.equal(codec.decodeFrame(frame).payload.length,32700);
  const bad=frame.slice();bad[34]=1;
  new DataView(bad.buffer).setUint32(bad.length-4,codec.crc32(bad.subarray(0,bad.length-4)),true);
  assert.throws(()=>codec.decodeFrame(bad),/header/);
});
test('256x128 uses profile ID 7 and keeps high length bytes reserved',()=>{
  const p=codec.profileForSize('256x128');
  assert.equal(p.id,7);assert.equal(p.frameSize,8192);assert.equal(p.blockSize,8124);
  const frame=read(cases.find(c=>c.case==='profile-256x128-boundary').frames[1]);
  assert.equal(frame[6],7);assert.equal(frame[34],0);assert.equal(frame[35],0);
  assert.equal(codec.decodeFrame(frame).payload.length,8124);
  const bad=frame.slice();bad[34]=1;
  new DataView(bad.buffer).setUint32(bad.length-4,codec.crc32(bad.subarray(0,bad.length-4)),true);
  assert.throws(()=>codec.decodeFrame(bad),/header/);
});
test('color modes pack exact frame bytes and keep VMT1 four-color vectors unchanged',()=>{
  const baseline=read(cases[5].frames[1]);
  assert.equal(baseline[4],1);assert.equal(baseline[7],0);
  for(const mode of [2,8,16,24]) {
    const spec=codec.layout(128,mode),meta={...metadata(cases[5]),colorMode:mode};
    const frame=codec.encodeMetadata(meta);
    assert.equal(frame[4],2);assert.equal(frame[6],1);assert.equal(frame[7],mode);
    assert.equal(frame.length,spec.frameSize);
    assert.deepEqual(codec.unpackSymbols(codec.packSymbols(frame),128,mode),frame);
    assert.deepEqual(codec.decodeFrame(frame).meta,meta);
    assert.equal(render.renderFrame(frame,2).width,320);
  }
});
test('sender local color comparison detects changed Canvas pixels',()=>{
  for(const mode of [2,8,16,24]) {
    const frame=codec.encodeMetadata({...metadata(cases[0]),colorMode:mode});
    const bitmap=render.renderFrame(frame,2),canvas={getContext:()=>({getImageData(x,y){
      const offset=(y*bitmap.width+x)*4;
      return {data:bitmap.data.slice(offset,offset+4)};
    }})};
    const clean=render.compareCanvasColors(canvas,frame,2);
    assert.equal(clean.changed,0);assert.equal(clean.maximum,0);
    const position=mode===24?[(19*2),(128+21)*2]:[(19*2),(128+21)*2];
    bitmap.data[(position[1]*bitmap.width+position[0])*4]^=255;
    const changed=render.compareCanvasColors(canvas,frame,2);
    assert.ok(changed.changed>=1);assert.ok(changed.maximum>0);
  }
});
