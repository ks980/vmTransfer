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
  sha256:new Uint8Array(Buffer.from(c.sha256,'hex'))});

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
    const encoded=codec.encodeData(meta,i,input.subarray(i*codec.profileForSize(meta.matrixSize).blockSize,(i+1)*codec.profileForSize(meta.matrixSize).blockSize));
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
    assert.equal(render.renderFrame(frame,2).width,p.logicalSize*2);
  }
  assert.throws(()=>codec.encodeMetadata({...metadata(cases[1]),matrixSize:192}),/matrix size/);
});
