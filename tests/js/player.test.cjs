const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const {FakeClock}=require('../fake-clock.cjs');
const {createPlayer,TARGET_RATES}=require('../../sender/player/player.js');
const codec=require('../../sender/protocol/codec.js');
const {prepareTransfer}=require('../../sender/session/transfer.js');

function fixture(totalBlocks=3,readFrame){
  const clock=new FakeClock(),painted=[],events=[],cleared=[];
  const transfer={totalBlocks,metadataFrame:new Uint8Array([255]),
    readFrame:readFrame||((index)=>Promise.resolve(new Uint8Array([index])))};
  const player=createPlayer(transfer,null,p=>events.push(p),{clock,
    paint:(bytes,scale)=>painted.push({at:clock.now(),bytes:Array.from(bytes),scale}),clear:scale=>cleared.push(scale)});
  return {clock,player,painted,events,cleared,transfer};
}
test('cycles repeat byte-identical metadata and sequential blocks forever',async()=>{
  const {clock,player,painted}=fixture();
  assert.equal(player.getProgress().state,'ready');assert.equal(painted.length,0);
  await player.start();await clock.advance(2400);
  assert.deepEqual(painted.slice(0,12).map(f=>f.bytes[0]),[255,0,1,2,255,0,1,2,255,0,1,2]);
  assert.equal(player.getProgress().state,'playing');assert.equal(player.getProgress().loop,4);
  assert.equal(player.getProgress().frameIndex,0);assert.equal(player.getProgress().totalFrames,4);
  assert.equal(player.getProgress().measuredLocalFps,5);
  player.stop();assert.equal(clock.jobs.size,0);
});
test('metadata is repeated before each group of at most 64 blocks',async()=>{
  const {clock,player,painted}=fixture(130);player.setRate(30);await player.start();await clock.advance(4500);
  const values=painted.slice(0,133).map(f=>f.bytes[0]);
  assert.deepEqual(values,[255,...Array.from({length:64},(_,i)=>i),255,
    ...Array.from({length:64},(_,i)=>i+64),255,128,129]);
  assert.equal(player.getProgress().totalFrames,133);player.stop();
});
test('empty input repeats only metadata without attempting reads',async()=>{
  const {clock,player,painted}=fixture(0,()=>{throw Error('must not read');});
  await player.start();await clock.advance(1000);assert.equal(painted.length,6);
  assert.ok(painted.every(f=>f.bytes[0]===255));assert.equal(player.getProgress().loop,6);player.stop();
});
test('all six discrete target rates are scheduled and measured independently',async()=>{
  for(const fps of TARGET_RATES) {
    const {clock,player,painted}=fixture();player.setRate(fps);await player.start();await clock.advance(1100);
    assert.ok(painted.length>=fps);
    for(let i=1;i<painted.length;i++)assert.ok(painted[i].at-painted[i-1].at>=1000/fps-1e-8);
    assert.ok(Math.abs(player.getProgress().measuredLocalFps-fps)<1e-8);player.stop();
  }
  const {player}=fixture();for(const rate of [0,6,29,60,NaN,'5'])assert.throws(()=>player.setRate(rate));
  for(const scale of [0,1.2,17,NaN])assert.throws(()=>player.setScale(scale));
});
test('pause holds pixels and resume does not skip its prefetched next block',async()=>{
  const {clock,player,painted,cleared}=fixture();await player.start();await clock.advance(200);
  player.pause();await clock.advance(5000);assert.equal(painted.length,2);assert.equal(cleared.length,0);
  assert.equal(clock.jobs.size,0);assert.equal(player.getProgress().state,'paused');
  await player.start();await clock.advance(0);assert.equal(painted.at(-1).bytes[0],1);player.stop();
});
test('stop clears pixels and a new start rewinds to the same metadata',async()=>{
  const {clock,player,painted,cleared}=fixture();await player.start();await clock.advance(400);
  player.stop();assert.equal(player.getProgress().state,'stopped');assert.equal(cleared.length,1);
  const count=painted.length;await clock.advance(1000);assert.equal(painted.length,count);
  await player.start();await clock.advance(0);assert.deepEqual(painted.at(-1).bytes,[255]);
  assert.equal(player.getProgress().loop,1);assert.equal(player.getProgress().frameIndex,0);player.stop();
});
test('late timers never catch up in a burst or skip blocks',async()=>{
  const {clock,player,painted}=fixture();await player.start();await clock.advance(0);
  clock.jump(2000);await clock.advance(0);
  assert.deepEqual(painted.map(f=>f.bytes[0]),[255,0]);assert.deepEqual(painted.map(f=>f.at),[0,2000]);
  await clock.advance(199);assert.equal(painted.length,2);
  await clock.advance(1);assert.equal(painted.at(-1).bytes[0],1);
  assert.ok(player.getProgress().measuredLocalFps<5);player.stop();
});
test('an early timer cannot present the next frame before its deadline',async()=>{
  const {clock,player,painted}=fixture();await player.start();await clock.advance(0);
  const [id,job]=Array.from(clock.jobs)[0];clock.jobs.delete(id);clock.jump(199);job.callback();
  assert.equal(painted.length,1);await clock.advance(1);assert.equal(painted.length,2);player.stop();
});
test('slow reads hold the previous frame and keep only one active read',async()=>{
  const reads=[];let running=0,maximum=0;
  const {clock,player,painted}=fixture(3,index=>new Promise(resolve=>{
    running++;maximum=Math.max(maximum,running);reads.push(()=>{running--;resolve(new Uint8Array([index]));});
  }));
  await player.start();await clock.advance(1000);assert.equal(painted.length,1);assert.equal(reads.length,1);
  reads.shift()();await clock.advance(0);assert.equal(painted.at(-1).bytes[0],0);
  assert.equal(reads.length,1);await clock.advance(400);assert.equal(painted.length,2);
  reads.shift()();await clock.advance(0);assert.equal(painted.at(-1).bytes[0],1);assert.equal(maximum,1);player.stop();
});
test('cancelled old reads cannot repaint or report errors after a new run',async()=>{
  const reads=[];
  const {clock,player,painted}=fixture(2,(index,signal)=>new Promise((resolve,reject)=>reads.push({index,signal,resolve,reject})));
  await player.start();await clock.advance(0);assert.equal(reads.length,1);
  player.pause();assert.equal(reads[0].signal.aborted,true);
  await player.start();await clock.advance(0);assert.equal(reads.length,2);
  reads[0].resolve(new Uint8Array([99]));await clock.advance(500);assert.equal(painted.length,1);
  reads[1].resolve(new Uint8Array([0]));await clock.advance(0);assert.equal(painted.at(-1).bytes[0],0);
  const pending=reads.at(-1);player.stop();pending.reject(Error('stale failure'));await clock.advance(1000);
  assert.equal(player.getProgress().state,'stopped');assert.ok(!painted.some(f=>f.bytes[0]===99));
});
test('rate and scale changes preserve transfer order and enforce a new hold period',async()=>{
  const {clock,player,painted}=fixture();await player.start();await clock.advance(50);
  player.setRate(10);await clock.advance(50);assert.equal(painted.at(-1).bytes[0],0);assert.equal(painted.at(-1).at,100);
  const visible=player.getProgress().visibleFrames;
  await clock.advance(25);player.setScale(6);assert.equal(painted.at(-1).scale,6);
  assert.equal(player.getProgress().visibleFrames,visible);assert.equal(player.getProgress().blockIndex,0);
  await clock.advance(99);assert.equal(player.getProgress().visibleFrames,visible);
  await clock.advance(1);assert.equal(painted.at(-1).bytes[0],1);
  player.setRate(5);await clock.advance(199);assert.equal(painted.at(-1).bytes[0],1);
  await clock.advance(1);assert.equal(painted.at(-1).bytes[0],2);player.stop();
});
test('active read/render failures clear pixels and terminate without automatic restart',async()=>{
  const {clock,player,cleared}=fixture(1,()=>Promise.reject(Error('file unreadable')));
  await player.start();await clock.advance(1000);assert.equal(player.getProgress().state,'error');
  assert.equal(player.getProgress().error,'file unreadable');assert.equal(clock.jobs.size,0);assert.equal(cleared.length,1);
  player.stop();await assert.rejects(player.start(),/unreadable/);
  const otherClock=new FakeClock();let didClear=false;
  const other=createPlayer({totalBlocks:0,metadataFrame:new Uint8Array(1)},null,()=>{},
    {clock:otherClock,paint:()=>{throw Error('canvas failed');},clear:()=>{didClear=true;}});
  await other.start();await otherClock.advance(0);assert.equal(other.getProgress().state,'error');assert.ok(didClear);
});
test('real prepared transfer retains ID, hash and byte-identical data over loops and stop',async()=>{
  const input=fs.readFileSync(path.resolve(__dirname,'../../protocol/vectors/random/input.bin'));
  const prepared=await prepareTransfer({name:'random.bin',size:input.length,
    async readAt(offset,length){return new Uint8Array(input.subarray(offset,offset+length));}},{id:new Uint8Array(16).fill(7)});
  const clock=new FakeClock(),frames=[];
  const player=createPlayer(prepared,null,()=>{},{clock,paint:bytes=>frames.push(bytes.slice()),clear:()=>{}});
  await player.start();await clock.advance(1800);
  assert.equal(frames.length,10);
  for(let i=0;i<5;i++)assert.deepEqual(frames[i],frames[i+5]);
  const metadata=codec.decodeFrame(frames[0]).meta;
  assert.deepEqual(metadata.id,new Uint8Array(16).fill(7));
  player.stop();await player.start();await clock.advance(0);assert.deepEqual(frames.at(-1),frames[0]);player.stop();
});
