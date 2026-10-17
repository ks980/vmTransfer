/* Sender UI. A generation guard prevents old preparations/players updating a
 * newly selected file. Receiver feedback is never an input to this application. */
(function() {
  'use strict';
  const el=id=>document.getElementById(id);
  let player=null,controller=null,generation=0,preparing=false,colorChecked=0;
  function controls(state) {
    el('cancel').disabled=!preparing;
    el('matrix').disabled=preparing||state==='playing'||state==='paused';
    el('color-mode').disabled=preparing||state==='playing'||state==='paused';
    el('start').disabled=!player||preparing||state==='playing'||state==='error';
    el('pause').disabled=state!=='playing';
    el('stop').disabled=state!=='playing'&&state!=='paused';
    el('start').textContent=state==='paused'?'继续播放':state==='stopped'?'从头播放':'开始播放';
  }
  function profile() {
    const value=el('matrix').value;return VMT.codec.profileForSize(value.indexOf('x')>=0?value:Number(value));
  }
  function syncScaleLimit() {el('scale').max=String(profile().maxScale);}
  function updateCanvasInfo() {
    const canvas=el('canvas');
    let clipped=false;
    if(canvas.getBoundingClientRect) {
      const bounds=canvas.getBoundingClientRect();
      if(bounds.right>window.innerWidth||bounds.bottom>window.innerHeight||bounds.left<0||bounds.top<0)
        clipped=true;
    }
    el('canvas-info').hidden=!clipped;
    el('canvas-info').textContent=clipped?'画面显示不完整，请缩小每符号像素或放大窗口。':'';
  }
  function clearCanvas() {
    const canvas=el('canvas'),p=profile(),scale=Number(el('scale').value);
    canvas.width=p.logicalWidth*scale;canvas.height=p.logicalHeight*scale;
    const context=canvas.getContext('2d');context.fillStyle='#fff';context.fillRect(0,0,canvas.width,canvas.height);
    updateCanvasInfo();
  }
  function visibilityNotice() {el('visibility-notice').hidden=!document.hidden;}
  function colorBar(id,colors) {
    const canvas=el(id),ctx=canvas.getContext('2d');
    ctx.fillStyle='#fff';ctx.fillRect(0,0,canvas.width,canvas.height);
    colors.forEach((rgb,i)=>{
      ctx.fillStyle='rgb('+rgb.join(',')+')';ctx.fillRect(i*16,0,16,20);
    });
  }
  function checkColors(bytes,scale) {
    if(colorChecked===2||(colorChecked===1&&bytes[5]!==2)||!VMT.render.compareCanvasColors)return;
    try {
      const result=VMT.render.compareCanvasColors(el('canvas'),bytes,scale);
      el('color-check').hidden=result.changed===0;
      if(result.changed) {
        colorBar('color-expected',result.expected);colorBar('color-observed',result.observed);
        el('color-check').open=true;
        el('color-summary').textContent='画面颜色与预设不一致';
        el('color-result').textContent='对比 '+result.expected.length+' 个颜色，'+result.changed+
          ' 个不同；单通道最大差值 '+result.maximum+'。';
      }
    } catch(error) {
      el('color-check').hidden=false;el('color-check').open=true;
      el('color-summary').textContent='无法检测画面颜色';
      el('color-result').textContent=error.message;
    }
    colorChecked=bytes[5]===2?2:1;
  }
  function renderProgress(progress) {
    const labels={ready:'准备完成，可以开始播放。',playing:'正在循环播放。',
      paused:'已暂停。',stopped:'',error:'播放失败：'};
    const status=labels[progress.state]+(progress.error?progress.error+'；请重新选择文件。':'');
    if(el('status').textContent!==status)el('status').textContent=status;
    el('status').hidden=progress.state==='stopped';
    const hasFrame=progress.frameIndex!==null;
    el('progress').hidden=!hasFrame;
    el('progress').textContent=hasFrame?'第 '+progress.loop+' 轮 · '+(progress.frameIndex+1)+'/'+progress.totalFrames+' 帧':'';
    if(progress.state==='error')el('file').value='';
    updateCanvasInfo();
    controls(progress.state);
  }
  async function prepareSelected() {
    const mine=++generation;
    if(controller)controller.abort();controller=null;
    const previous=player;player=null;if(previous)previous.stop();
    preparing=false;colorChecked=0;controls('ready');
    el('color-check').hidden=true;el('color-check').open=false;el('color-result').textContent='';
    syncScaleLimit();
    if(Number(el('scale').value)>profile().maxScale)el('scale').value=String(profile().maxScale);
    clearCanvas();
    el('progress').hidden=true;el('progress').textContent='';
    el('status').hidden=false;el('file-info').textContent='';
    const file=el('file').files[0];
    if(!file){el('status').textContent='尚未选择文件';return;}
    el('file-info').textContent=file.name+' · '+file.size+' 字节';
    preparing=true;controller=new AbortController();controls('ready');
    try {
      const prepared=await VMT.transfer.prepareTransfer(VMT.transfer.fileSource(file),{
        matrixSize:profile().matrixSize,colorMode:Number(el('color-mode').value),signal:controller.signal,onProgress:(done,total)=>{
          if(mine===generation)el('status').textContent='准备中：'+(total?done/total*100:100).toFixed(1)+'% · '+done+'/'+total+' 字节';
        }
      });
      if(mine!==generation)return;
      preparing=false;
      player=VMT.player.createPlayer(prepared,el('canvas'),progress=>{
        if(mine===generation)renderProgress(progress);
      },{fps:Number(el('rate').value),scale:Number(el('scale').value),paint:(bytes,scale)=>{
        VMT.render.drawCanvas(el('canvas'),VMT.render.renderFrame(bytes,scale));checkColors(bytes,scale);
      }});
      el('file-info').textContent=file.name+' · '+file.size+' 字节';
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
    syncScaleLimit();
    if(Number(el('scale').value)>profile().maxScale){el('scale').value=String(profile().maxScale);lastScale=profile().maxScale;}
    if(preparing||(player&&['playing','paused'].includes(player.getProgress().state)))return;
    return prepareSelected();
  };
  el('color-mode').onchange=()=>{
    if(preparing||(player&&['playing','paused'].includes(player.getProgress().state)))return;
    return prepareSelected();
  };
  el('cancel').onclick=()=>{if(controller)controller.abort();};
  el('start').onclick=async()=>{try{if(player)await player.start();}catch(error){el('status').hidden=false;el('status').textContent=error.message;}};
  el('pause').onclick=()=>{if(player)player.pause();};
  el('stop').onclick=()=>{if(player)player.stop();};
  el('rate').onchange=()=>{if(player)player.setRate(Number(el('rate').value));};
  let lastScale=Number(el('scale').value);
  el('scale').onchange=()=>{
    const scale=Number(el('scale').value);
    const max=profile().maxScale;
    if(!Number.isInteger(scale)||scale<1||scale>max) {
      el('scale').value=String(lastScale);el('status').textContent='每符号像素必须为 1 到 '+max+' 的整数。';return;
    }
    lastScale=scale;colorChecked=0;if(player)player.setScale(scale);else clearCanvas();
  };
  document.addEventListener('visibilitychange',visibilityNotice);
  window.addEventListener('pagehide',()=>{
    if(controller) {
      ++generation;controller.abort();controller=null;preparing=false;el('file').value='';
      el('status').hidden=false;el('status').textContent='准备已取消；请重新选择文件。';controls('ready');
    }
    if(player)player.stop();
  });
  el('progress').hidden=true;el('color-check').hidden=true;visibilityNotice();controls('ready');syncScaleLimit();clearCanvas();
})();
