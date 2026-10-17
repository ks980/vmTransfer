/* Deterministic predefined-profile rasterizer: no DOM dependency in renderFrame(). */
(function(root,factory) {
  if(typeof module==='object' && module.exports) module.exports=factory(require('../protocol/codec.js'));
  else (root.VMT=root.VMT||{}).render=factory(root.VMT.codec);
})(typeof globalThis!=='undefined'?globalThis:this,function(codec) {
  'use strict';
  const PALETTE=[[0,0,0],[255,0,0],[0,255,0],[0,0,255],[255,255,255]];
  const PALETTES={
    2:[[0,0,0],[255,255,255]],
    4:PALETTE.slice(0,4),
    8:[[0,0,0],[255,0,0],[0,255,0],[0,0,255],[255,255,0],[255,0,255],[0,255,255],[255,255,255]],
    16:[[0,0,0],[255,0,0],[0,255,0],[0,0,255],[255,255,0],[255,0,255],[0,255,255],[255,255,255],
      [128,0,0],[0,128,0],[0,0,128],[128,128,0],[128,0,128],[0,128,128],[128,128,128],[255,128,0]]
  };
  const WHITE=0x1000000,BLACK=0x1000001,RAW=0x2000000;
  function swatches(mode) {
    if(mode===4)return PALETTE;
    if(mode===24)return PALETTE;
    return PALETTES[mode].concat([[0,0,0],[255,255,255]]);
  }
  /** @typedef {{width:number,height:number,stride:number,data:Uint8ClampedArray}} Bitmap
   * Owned, opaque RGBA8 image. The same bytes feed Canvas and PNG fixture generation.
   */
  /** @param {Uint8Array} frame @param {number} scale Integer pixels per symbol (bounded by profile and 32 Mi pixels). */
  function renderFrame(frame,scale) {
    const spec=codec.layoutForFrame(frame),profile=spec.profile,mode=spec.mode;
    if(!Number.isInteger(scale)||scale<1||scale>profile.maxScale) throw new RangeError('scale exceeds profile image limit');
    const n=profile.matrixWidth,m=profile.matrixHeight,side=profile.logicalWidth,height=profile.logicalHeight;
    const logical=new Uint32Array(side*height); logical.fill(WHITE);
    function rect(x,y,w,h,color) { for(let j=y;j<y+h;j++) logical.fill(color,j*side+x,j*side+x+w); }
    for(const [x,y] of [[4,4],[n+21,4],[4,m+21]]) {
      rect(x,y,7,7,BLACK);rect(x+1,y+1,5,5,WHITE);rect(x+2,y+2,3,3,BLACK);
    }
    for(let i=16;i<n+16;i++) { logical[12*side+i]=i%2?WHITE:BLACK; }
    for(let i=16;i<m+16;i++) logical[i*side+12]=i%2?WHITE:BLACK;
    if(spec.version===2) {
      const code={2:1,8:2,16:3,24:4}[mode];
      for(let repeat=0;repeat<4;repeat++)for(let bit=0;bit<3;bit++)
        logical[13*side+16+repeat*3+bit]=(code&(1<<(2-bit)))?BLACK:WHITE;
      swatches(mode).forEach((rgb,i)=>rect(16+(i%9)*16,m+18+Math.floor(i/9)*7,6,6,
        RAW|(rgb[0]<<16)|(rgb[1]<<8)|rgb[2]));
    } else for(let i=0;i<5;i++)rect(16+i*16,m+20,8,8,RAW|(PALETTE[i][0]<<16)|(PALETTE[i][1]<<8)|PALETTE[i][2]);
    const symbols=codec.packSymbols(frame);
    for(let y=0;y<m;y++) logical.set(symbols.subarray(y*n,(y+1)*n),(y+16)*side+16);
    const width=side*scale,physicalHeight=height*scale,data=new Uint8ClampedArray(width*physicalHeight*4);
    for(let y=0;y<physicalHeight;y++) for(let x=0;x<width;x++) {
      const value=logical[Math.floor(y/scale)*side+Math.floor(x/scale)],p=(y*width+x)*4;
      let color;
      if(value===WHITE)color=[255,255,255];
      else if(value===BLACK)color=[0,0,0];
      else if(value>=RAW)color=[(value>>>16)&255,(value>>>8)&255,value&255];
      else if(mode===24)color=[(value>>>16)&255,(value>>>8)&255,value&255];
      else color=PALETTES[mode][value];
      data[p]=color[0];data[p+1]=color[1];data[p+2]=color[2];data[p+3]=255;
    }
    return {width,height:physicalHeight,stride:width*4,data};
  }
  /** Present one complete raster; VNC may still deliver partial screen updates. */
  function drawCanvas(canvas,bitmap) {
    canvas.width=bitmap.width;canvas.height=bitmap.height;
    const context=canvas.getContext('2d');context.imageSmoothingEnabled=false;
    context.putImageData(new ImageData(bitmap.data,bitmap.width,bitmap.height),0,0);
  }
  /** Compare selected pixels in the actual local Canvas with the protocol colors. */
  function compareCanvasColors(canvas,frame,scale) {
    const spec=codec.layoutForFrame(frame),mode=spec.mode,m=spec.profile.matrixHeight;
    const context=canvas.getContext('2d'),references=swatches(mode).map((rgb,i)=>({
      x:spec.version===1?20+i*16:19+(i%9)*16,
      y:spec.version===1?m+24:m+21+Math.floor(i/9)*7,rgb
    }));
    if(mode===24) {
      const symbols=codec.packSymbols(frame),n=spec.profile.matrixWidth;
      for(let i=0;i<13;i++) {
        const position=Math.floor((i+1)*symbols.length/14),value=symbols[position];
        references.push({x:16+(position%n)+.5,y:16+Math.floor(position/n)+.5,
          rgb:[(value>>>16)&255,(value>>>8)&255,value&255]});
      }
    }
    let maximum=0,changed=0;
    const observed=references.map(ref=>{
      const pixel=context.getImageData(Math.floor(ref.x*scale),Math.floor(ref.y*scale),1,1).data;
      const rgb=[pixel[0],pixel[1],pixel[2]];
      const delta=Math.max(...rgb.map((v,i)=>Math.abs(v-ref.rgb[i])));
      maximum=Math.max(maximum,delta);if(delta)changed++;
      return rgb;
    });
    return {expected:references.map(r=>r.rgb),observed,maximum,changed};
  }
  return {PALETTE,PALETTES,swatches,renderFrame,drawCanvas,compareCanvasColors};
});
