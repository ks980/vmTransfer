/* Sender UI. A generation guard prevents old preparations/players updating a
 * newly selected file. Receiver feedback is never an input to this application. */
(function() {
  'use strict';
  const el=id=>document.getElementById(id);
  let player=null,controller=null,generation=0,preparing=false;
  function controls(state) {
    el('cancel').disabled=!preparing;
    el('matrix').disabled=preparing||state==='playing'||state==='paused';
    el('start').disabled=!player||preparing||state==='playing'||state==='error';
    el('pause').disabled=state!=='playing';
    el('stop').disabled=state!=='playing'&&state!=='paused';
    el('start').textContent=state==='paused'?'继续播放':state==='stopped'?'从头播放':'开始播放';
  }
  function clearCanvas() {
    const canvas=el('canvas');canvas.width=canvas.height=(32+Number(el('matrix').value))*Number(el('scale').value);
    const context=canvas.getContext('2d');context.fillStyle='#fff';context.fillRect(0,0,canvas.width,canvas.height);
  }
  function visibilityNotice() {el('visibility-notice').hidden=!document.hidden;}
  function renderProgress(progress) {
    const labels={ready:'准备完成，点击开始播放。',playing:'正在循环播放，直到手动停止。',
      paused:'已暂停，当前画面保留。',stopped:'已停止，下次从头播放。',error:'播放失败：'};
    const status=labels[progress.state]+(progress.error?progress.error+'；请重新选择文件。':'');
    if(el('status').textContent!==status)el('status').textContent=status;
    const position=progress.frameIndex===null?'—':String(progress.frameIndex+1);
    const kind=progress.frameType==='data'?'数据块 '+(progress.blockIndex+1)+'/'+progress.totalBlocks:
      progress.frameType==='metadata'?'元数据':'尚未播放';
    el('progress').textContent='第 '+progress.loop+' 轮 · 帧 '+position+'/'+progress.totalFrames+' · '+kind;
    const interval=progress.localFrameIntervalMs===null?'—':progress.localFrameIntervalMs.toFixed(1)+' ms';
    el('timing').textContent='目标 '+progress.requestedFps+' FPS · 本地绘制 '+progress.measuredLocalFps.toFixed(1)+
      ' FPS · 最近间隔 '+interval;
    if(progress.state==='error')el('file').value='';
    controls(progress.state);
  }
  async function prepareSelected() {
    const mine=++generation;
    if(controller)controller.abort();controller=null;
    const previous=player;player=null;if(previous)previous.stop();
    preparing=false;controls('ready');clearCanvas();
    el('progress').textContent='尚未播放';el('timing').textContent='本地绘制频率不代表 VNC 接收频率。';
    el('hash').textContent='';el('file-info').textContent='';
    const file=el('file').files[0];
    if(!file){el('status').textContent='尚未选择文件';return;}
    el('file-info').textContent=file.name+' · '+file.size+' 字节';
    preparing=true;controller=new AbortController();controls('ready');
    try {
      const prepared=await VMT.transfer.prepareTransfer(VMT.transfer.fileSource(file),{
        matrixSize:Number(el('matrix').value),signal:controller.signal,onProgress:(done,total)=>{
          if(mine===generation)el('status').textContent='准备中：'+(total?done/total*100:100).toFixed(1)+'% · '+done+'/'+total+' 字节';
        }
      });
      if(mine!==generation)return;
      preparing=false;
      player=VMT.player.createPlayer(prepared,el('canvas'),progress=>{
        if(mine===generation)renderProgress(progress);
      },{fps:Number(el('rate').value),scale:Number(el('scale').value)});
      el('file-info').textContent=file.name+' · '+file.size+' 字节 · '+prepared.totalBlocks+' 个数据块 · '+el('matrix').value+'×'+el('matrix').value;
      el('hash').textContent=Array.from(prepared.metadata.sha256,b=>b.toString(16).padStart(2,'0')).join('');
      renderProgress(player.getProgress());
    } catch(error) {
      if(mine===generation) {
        el('status').textContent=error.name==='AbortError'?'已取消准备；请重新选择文件。':'准备失败：'+error.message;
        el('file').value='';
      }
    } finally {
      if(mine===generation){preparing=false;controller=null;controls(player?player.getProgress().state:'ready');}
    }
  }
  el('file').addEventListener('change',prepareSelected);
  el('matrix').onchange=()=>{
    if(preparing||(player&&['playing','paused'].includes(player.getProgress().state)))return;
    return prepareSelected();
  };
  el('cancel').onclick=()=>{if(controller)controller.abort();};
  el('start').onclick=async()=>{try{if(player)await player.start();}catch(error){el('status').textContent=error.message;}};
  el('pause').onclick=()=>{if(player)player.pause();};
  el('stop').onclick=()=>{if(player)player.stop();};
  el('rate').onchange=()=>{if(player)player.setRate(Number(el('rate').value));};
  let lastScale=Number(el('scale').value);
  el('scale').onchange=()=>{
    const scale=Number(el('scale').value);
    if(!Number.isInteger(scale)||scale<1||scale>16) {
      el('scale').value=String(lastScale);el('status').textContent='每符号像素必须为 1 到 16 的整数。';return;
    }
    lastScale=scale;if(player)player.setScale(scale);else clearCanvas();
  };
  document.addEventListener('visibilitychange',visibilityNotice);
  window.addEventListener('pagehide',()=>{
    if(controller) {
      ++generation;controller.abort();controller=null;preparing=false;el('file').value='';
      el('status').textContent='准备已取消；请重新选择文件。';controls('ready');
    }
    if(player)player.stop();
  });
  visibilityNotice();controls('ready');clearCanvas();
})();
