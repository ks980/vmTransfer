/* JS bytes -> actual PNGs -> Rust CLI -> byte-for-byte file verification.
 * Every manifest contains ONLY image paths, never the file hash/name or geometry.
 */
const fs=require('node:fs');
const path=require('node:path');
const os=require('node:os');
const cp=require('node:child_process');
const assert=require('node:assert/strict');
const crypto=require('node:crypto');
const codec=require('../sender/protocol/codec.js');
const transfer=require('../sender/session/transfer.js');
const {renderFrame}=require('../sender/render/canvas.js');
const image=require('./image-tools.cjs');
const {FakeClock}=require('./fake-clock.cjs');
const {createPlayer}=require('../sender/player/player.js');
const root=path.resolve(__dirname,'..');
const binary=path.join(root,'receiver/target/release/vmtransfer');
const vectorRoot=path.join(root,'protocol/vectors');
const cases=JSON.parse(fs.readFileSync(path.join(vectorRoot,'cases.json')));
const outputRoot=path.join(root,'target');fs.mkdirSync(outputRoot,{recursive:true});
const work=fs.mkdtempSync(path.join(outputRoot,'e2e-'));
const results=[];

async function prepare(c,colorMode) {
  const input=fs.readFileSync(path.join(vectorRoot,c.input));
  const source={name:c.filename,size:input.length,async readAt(offset,length){return new Uint8Array(input.subarray(offset,offset+length));}};
  const session=await transfer.prepareTransfer(source,{id:new Uint8Array(Buffer.from(c.id,'hex')),matrixSize:c.matrix_size||128,colorMode:colorMode||c.color_mode});
  const frames=[session.metadataFrame];
  for(let i=0;i<session.totalBlocks;i++)frames.push(await session.readFrame(i));
  return {input,frames,meta:session.metadata,prepared:session};
}
function run(name,images,expected,options={}) {
  const dir=path.join(work,name);fs.mkdirSync(dir);
  const paths=images.map((bitmap,i)=>{
    const filename=`frame-${String(i).padStart(3,'0')}.png`;
    fs.writeFileSync(path.join(dir,filename),image.png(bitmap));return filename;
  });
  // Success must stop before trying this nonexistent extra capture.
  if(options.stopCheck)paths.push('must-not-be-consumed.png');
  fs.writeFileSync(path.join(dir,'images.json'),JSON.stringify(paths));
  const out=path.join(dir,'received');
  const result=cp.spawnSync(binary,['decode','--manifest',path.join(dir,'images.json'),'--output-dir',out],{encoding:'utf8',timeout:60000});
  assert.equal(result.status,options.status===undefined?0:options.status,`${name}: ${result.error||result.stderr}\n${result.stdout}`);
  const report=JSON.parse(result.stdout);
  assert.equal(report.complete,!options.status,`${name}: unexpected completion`);
  if(report.complete) {
    const files=fs.readdirSync(out);assert.equal(files.length,1);
    assert.deepEqual(fs.readFileSync(path.join(out,files[0])),expected,`${name}: recovered bytes differ`);
    assert.equal(crypto.createHash('sha256').update(fs.readFileSync(path.join(out,files[0]))).digest('hex'),crypto.createHash('sha256').update(expected).digest('hex'));
  } else assert.equal(fs.readdirSync(out).length,0,`${name}: incomplete file published or leaked`);
  const counters=report.metrics.counters;
  if(options.check)options.check(counters);
  results.push({name,...counters,elapsed_seconds:report.metrics.elapsed_seconds});
  console.log(`PASS ${name}: ${counters.unique_blocks}/${counters.total_blocks} blocks, CRC=${counters.crc_failures}, locate=${counters.locator_failures}, palette=${counters.palette_failures}`);
  return report;
}

async function main() {
  cp.execFileSync('cargo',['build','--offline','--locked','--release','--manifest-path','receiver/Cargo.toml'],{cwd:root,stdio:'inherit'});
  for(const c of cases) {
    const p=await prepare(c);
    const report=run('golden-'+c.case,p.frames.map(f=>renderFrame(f,3)),p.input,{stopCheck:true});
    const dims=codec.profileForSize(c.matrix_size||128);
    assert.deepEqual(report.visual_format,{matrix_width:dims.matrixWidth,matrix_height:dims.matrixHeight,
      color_mode:c.color_mode||4});
  }
  // VMT2 indexed and RGB888 modes use the same native PNG decoder and collector.
  for(const mode of [2,8,16,24]) {
    const q=await prepare(cases.find(c=>c.case==='random'),mode);
    run(`color-${mode}-random`,q.frames.map(f=>renderFrame(f,3)),q.input,{stopCheck:true});
    if(mode!==24)run(`color-${mode}-mild-distortion`,q.frames.map(f=>image.color(image.pad(renderFrame(f,3)))),q.input);
    const bad=q.frames[1].slice();bad[100]^=1;
    run(`color-${mode}-crc-loops`,[renderFrame(q.frames[0],3),renderFrame(bad,3),...q.frames.slice(1).map(f=>renderFrame(f,3))],q.input,
      {check:c=>assert.ok(c.crc_failures>=1)});
  }
  const rgbRectangle=await prepare(cases.find(c=>c.case==='profile-256x128-partial'),24);
  run('color-24-rectangle',rgbRectangle.frames.map(f=>renderFrame(f,2)),rgbRectangle.input);
  const rgbLossy=await prepare(cases.find(c=>c.case==='random'),24);
  run('color-24-lossy-rejected',rgbLossy.frames.map(f=>image.color(renderFrame(f,3))),rgbLossy.input,
    {status:2,check:c=>assert.ok(c.crc_failures+c.palette_failures>=1)});
  for(const n of [160,256,512,"1028x512","512x256","256x128"]) {
    const q=await prepare(cases.find(c=>c.case===`profile-${n}-partial`));
    const rasters=q.frames.map(f=>renderFrame(f,n===512||n==="1028x512"||n==="512x256"||n==="256x128"?2:4));
    const large=n===512||n==='1028x512'||n==='512x256'||n==='256x128';
    run(`profile-${n}-distortion`,rasters.map(b=>large?image.color(image.pad(image.resize(b,1.1,1.1,'bilinear'))):
      image.color(image.pad(image.rotate(image.resize(b,1.13,.875,'bilinear'),1)))),q.input);
    if(large)run(`profile-${n}-rotate`,rasters.map(b=>image.pad(image.rotate(b,1))),q.input);
    const bad=q.frames[1].slice();bad[200]^=1;
    run(`profile-${n}-loops`,[rasters[1],rasters[0],rasters[3],rasters[1],rasters[1],
      image.mix(rasters[1],rasters[2]),renderFrame(bad,n===512||n==="1028x512"||n==="512x256"||n==="256x128"?2:4),rasters[0],rasters[2]],q.input,
      {stopCheck:true,check:c=>{assert.equal(c.waiting_metadata,1);assert.equal(c.duplicates,1);assert.ok(c.crc_failures>=2);}});
  }
  const p=await prepare(cases.find(c=>c.case==='random'));
  const base=p.frames.map(f=>renderFrame(f,4));
  for(const scale of [.75,1.25,1.5,1.13])for(const method of ['nearest','bilinear']) {
    run(`scale-${scale}-${method}`,base.map(b=>image.pad(image.resize(b,scale,scale,method))),p.input);
  }
  for(const turns of [1,2,3])run('rotate-'+turns,base.map(b=>image.pad(image.rotate(b,turns))),p.input);
  run('anisotropic',base.map(b=>image.pad(image.resize(b,.875,1.25,'bilinear'))),p.input);
  run('color-distortion',base.map(b=>image.color(image.pad(b))),p.input);
  run('combined',base.map(b=>image.color(image.pad(image.rotate(image.resize(b,1.13,.875,'bilinear'),1)))),p.input);
  for(const scale of [1,2])run('small-symbol-'+scale,p.frames.map(f=>renderFrame(f,scale)),p.input);

  const badBytes=p.frames[1].slice();badBytes[200]^=1;
  const foreignMeta=codec.encodeMetadata({...p.meta,id:new Uint8Array(16).fill(99)});
  const mixed=image.mix(base[1],base[2]);
  const uncertain={...base[1],data:new Uint8ClampedArray(base[1].data)};
  // White inside the encoding region cannot be a valid data symbol.
  for(let y=100;y<104;y++)for(let x=100;x<104;x++)uncertain.data.set([255,255,255,255],(y*uncertain.width+x)*4);
  run('repeated-loops-with-loss',[base[1],base[0],base[4],base[2],base[2],mixed,
    renderFrame(badBytes,4),uncertain,renderFrame(foreignMeta,4),base[0],base[4],base[1],base[3]],p.input,
    {stopCheck:true,check:c=>{assert.equal(c.waiting_metadata,1);assert.equal(c.duplicates,2);assert.equal(c.foreign_frames,1);assert.ok(c.crc_failures>=2);assert.ok(c.palette_failures>=1);}});
  run('incomplete',[base[0],base[1],base[1]],p.input,{status:2});
  const white=image.bitmap(640,640);white.data.fill(255);
  run('missing-locators',[white,base[0],...base.slice(1)],p.input,{check:c=>assert.equal(c.locator_failures,1)});

  // Exercise the implemented automatic player, not a manually assembled cycle.
  for(const caseName of ['random','empty','profile-160-partial','profile-256-partial','profile-512-partial','profile-1028x512-partial','profile-512x256-partial','profile-256x128-partial']) {
    const prepared=await prepare(cases.find(c=>c.case===caseName));
    const clock=new FakeClock(),frames=[];
    const player=createPlayer(prepared.prepared,null,()=>{},{clock,scale:3,
      paint:(bytes,scale)=>frames.push(renderFrame(bytes,scale)),clear:()=>{}});
    await player.start();await clock.advance(caseName==='empty'?800:2400);
    const captured=caseName==='random'?[0,1,1,3,5,9,10,12].map(i=>frames[i]):caseName==='empty'?[frames[0]]:[0,1,1,3,4,6].map(i=>frames[i]);
    run('automatic-player-'+caseName,captured,prepared.input,{stopCheck:true,
      check:c=>{if(caseName==='random')assert.equal(c.duplicates,1);}});
    assert.equal(player.getProgress().state,'playing');
    const before=player.getProgress().visibleFrames;await clock.advance(400);
    assert.ok(player.getProgress().visibleFrames>before,'sender must continue after receiver completes');
    player.stop();
  }

  const summary={scope:'offline synthetic images only; not a VNC benchmark',
    environment:{node:process.version,platform:os.platform(),arch:os.arch(),cpu:os.cpus()[0].model},
    artifact_directory:path.relative(root,work),scenarios:results};
  fs.writeFileSync(path.join(work,'results.json'),JSON.stringify(summary,null,2)+'\n');
  fs.copyFileSync(path.join(work,'results.json'),path.join(outputRoot,'latest-e2e-results.json'));
  console.log(`\n${results.length} scenarios passed. Reproducible images/manifests and results: ${path.relative(root,work)}`);
}
main().catch(error=>{console.error(error);console.error('Artifacts retained at '+work);process.exitCode=1;});
