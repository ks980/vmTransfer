/* Monotonic, bounded playback. No feedback from the receiver or file channels. */
(function(root,factory) {
  if(typeof module==='object' && module.exports) {
    module.exports=factory(require('../session/transfer.js'),require('../render/canvas.js'));
  } else (root.VMT=root.VMT||{}).player=factory(root.VMT.transfer,root.VMT.render);
})(typeof globalThis!=='undefined'?globalThis:this,function(transferModule,renderer) {
  'use strict';
  const TARGET_RATES=Object.freeze([5,10,15,20,25,30]);

  /** Build a player for one immutable PreparedTransfer. Start resolves after
   * scheduling; asynchronous read/render errors are reported in PlaybackProgress.
   * Pause holds the visible frame and resumes at the next unshown descriptor.
   * Stop clears the canvas and rewinds, preserving the prepared transfer and ID.
   * An error is terminal: prepare/select the file again to create a fresh player.
   * @param {Object} transfer PreparedTransfer from prepareTransfer().
   * @param {HTMLCanvasElement} canvas
   * @param {function(Object):void} onProgress Non-throwing progress observer.
   * @param {{fps?:number,scale?:number,clock?:Object,paint?:function,clear?:function}} options
   * clock/paint/clear are optional development seams for deterministic tests.
   */
  function createPlayer(transfer,canvas,onProgress,options) {
    options=options||{};
    const clock=options.clock||{now:()=>performance.now(),setTimeout:(fn,delay)=>setTimeout(fn,Math.ceil(delay)),clearTimeout:id=>clearTimeout(id)};
    const paint=options.paint||((bytes,scale)=>renderer.drawCanvas(canvas,renderer.renderFrame(bytes,scale)));
    const clear=options.clear||((scale)=>{
      canvas.width=(32+(transfer.matrixSize||128))*scale;canvas.height=canvas.width;
      const context=canvas.getContext('2d');context.fillStyle='#ffffff';context.fillRect(0,0,canvas.width,canvas.height);
    });
    let fps=options.fps===undefined?5:options.fps,scale=options.scale===undefined?4:options.scale;
    validateRate(fps);validateScale(scale);
    let state='ready',error=null,generation=0,timer=null,controller=null;
    let reading=false,pending=null,currentBytes=null,displayed=null;
    let iterator,next,ordinal,loop,visibleFrames=0,deadline=0,lastPaint=null,times=[];
    const totalFrames=transfer.totalBlocks===0?1:transfer.totalBlocks+Math.ceil(transfer.totalBlocks/64);
    function resetCursor() {
      iterator=transferModule.cycleFrames(transfer.totalBlocks);next=iterator.next().value;ordinal=0;loop=1;
    }
    resetCursor();
    function validateRate(value) {
      if(!TARGET_RATES.includes(value))throw new RangeError('帧率必须为 5、10、15、20、25 或 30 FPS');
    }
    function validateScale(value) {
      if(!Number.isInteger(value)||value<1||value>16)throw new RangeError('每符号像素必须为 1 到 16 的整数');
    }
    function getProgress() {
      const span=times.length>1?times[times.length-1]-times[0]:0;
      return {state,error,requestedFps:fps,scale,totalFrames,totalBlocks:transfer.totalBlocks,
        loop:displayed?displayed.loop:0,frameIndex:displayed?displayed.ordinal:null,
        frameType:displayed?displayed.type:null,blockIndex:displayed&&displayed.type==='data'?displayed.index:null,
        visibleFrames,measuredLocalFps:span>0?(times.length-1)*1000/span:0,
        localFrameIntervalMs:times.length>1?times[times.length-1]-times[times.length-2]:null};
    }
    function emit() {if(onProgress)onProgress(getProgress());}
    function clearTimer() {if(timer!==null){clock.clearTimeout(timer);timer=null;}}
    function active(mine) {return state==='playing'&&mine===generation;}
    function invalidate() {
      ++generation;clearTimer();if(controller)controller.abort();
      controller=null;reading=false;pending=null;
    }
    function fail(reason) {
      invalidate();state='error';error=reason&&reason.message?reason.message:String(reason);times=[];currentBytes=null;
      try {clear(scale);}catch(_) { /* Keep the original read/render failure. */ }
      emit();
    }
    function advanceCursor() {
      let step=iterator.next();ordinal++;
      if(step.done){loop++;ordinal=0;iterator=transferModule.cycleFrames(transfer.totalBlocks);step=iterator.next();}
      next=step.value;
    }
    /** One encoded frame of read-ahead. Advance only after drawing, so a cancelled
     * read cannot silently skip a block on pause/resume. */
    function readAhead() {
      if(state!=='playing'||reading||pending)return;
      reading=true;
      const mine=generation,descriptor={type:next.type,index:next.index,ordinal,loop},signal=controller.signal;
      Promise.resolve().then(()=>{
        if(!active(mine))return null;
        return descriptor.type==='metadata'?transfer.metadataFrame:transfer.readFrame(descriptor.index,signal);
      }).then(bytes=>{
        if(!active(mine))return;
        reading=false;pending={bytes,descriptor};arm();
      },reason=>{if(active(mine))fail(reason);});
    }
    function arm() {
      clearTimer();if(state!=='playing')return;
      if(!pending){readAhead();return;}
      const mine=generation;
      timer=clock.setTimeout(()=>{timer=null;present(mine);},Math.max(0,deadline-clock.now()));
    }
    function present(mine) {
      if(!active(mine)||!pending)return;
      if(clock.now()<deadline){arm();return;}
      const ready=pending;pending=null;
      try {paint(ready.bytes,scale);}catch(reason){fail(reason);return;}
      if(!active(mine))return;
      currentBytes=ready.bytes;displayed=ready.descriptor;visibleFrames++;
      lastPaint=clock.now();times.push(lastPaint);if(times.length>32)times.shift();
      // Late tabs/reads never cause a catch-up burst or skip logical blocks.
      deadline=lastPaint+1000/fps;
      advanceCursor();emit();readAhead();
    }
    async function start() {
      if(state==='playing')return;
      if(state==='error')throw new Error(error||'请重新选择文件');
      state='playing';error=null;++generation;controller=new AbortController();times=[];
      deadline=lastPaint===null?clock.now():Math.max(clock.now(),lastPaint+1000/fps);
      emit();arm();
    }
    function pause() {
      if(state!=='playing')return;
      invalidate();state='paused';times=[];emit();
    }
    function stop() {
      invalidate();currentBytes=null;displayed=null;lastPaint=null;times=[];visibleFrames=0;resetCursor();
      if(state!=='error'){state='stopped';error=null;}
      clear(scale);emit();
    }
    function setRate(value) {
      validateRate(value);if(fps===value)return;
      fps=value;times=[];
      if(state==='playing') {deadline=lastPaint===null?clock.now():Math.max(clock.now(),lastPaint+1000/fps);arm();}
      emit();
    }
    function setScale(value) {
      validateScale(value);if(scale===value)return;
      // Redraw the same frame; do not advance sequence, loop count or FPS samples.
      try {if(currentBytes)paint(currentBytes,value);else clear(value);}catch(reason){fail(reason);return;}
      scale=value;
      if(state==='playing'){lastPaint=clock.now();deadline=lastPaint+1000/fps;arm();}
      emit();
    }
    return {start,pause,stop,setRate,setScale,getProgress};
  }
  return {TARGET_RATES,createPlayer};
});
