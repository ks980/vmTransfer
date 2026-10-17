/* Deterministic predefined-profile rasterizer: no DOM dependency in renderFrame(). */
(function(root,factory) {
  if(typeof module==='object' && module.exports) module.exports=factory(require('../protocol/codec.js'));
  else (root.VMT=root.VMT||{}).render=factory(root.VMT.codec);
})(typeof globalThis!=='undefined'?globalThis:this,function(codec) {
  'use strict';
  const PALETTE=[[0,0,0],[255,0,0],[0,255,0],[0,0,255],[255,255,255]];
  /** @typedef {{width:number,height:number,stride:number,data:Uint8ClampedArray}} Bitmap
   * Owned, opaque RGBA8 image. The same bytes feed Canvas and PNG fixture generation.
   */
  /** @param {Uint8Array} frame @param {number} scale Integer pixels per symbol (1..16). */
  function renderFrame(frame,scale) {
    if(!Number.isInteger(scale)||scale<1||scale>16) throw new RangeError('scale must be 1..16');
    const profile=codec.PROFILES.find(p=>p.frameSize===frame.length);
    if(!profile)throw new Error('unsupported frame size');
    const n=profile.matrixSize,side=profile.logicalSize;
    const logical=new Uint8Array(side*side); logical.fill(4);
    function rect(x,y,w,h,color) { for(let j=y;j<y+h;j++) logical.fill(color,j*side+x,j*side+x+w); }
    for(const [x,y] of [[4,4],[n+21,4],[4,n+21]]) {
      rect(x,y,7,7,0);rect(x+1,y+1,5,5,4);rect(x+2,y+2,3,3,0);
    }
    for(let i=16;i<n+16;i++) { logical[12*side+i]=i%2?4:0;logical[i*side+12]=i%2?4:0; }
    for(let i=0;i<5;i++) rect(16+i*16,n+20,8,8,i);
    const symbols=codec.packSymbols(frame);
    for(let y=0;y<n;y++) logical.set(symbols.subarray(y*n,(y+1)*n),(y+16)*side+16);
    const width=side*scale,data=new Uint8ClampedArray(width*width*4);
    for(let y=0;y<width;y++) for(let x=0;x<width;x++) {
      const color=PALETTE[logical[Math.floor(y/scale)*side+Math.floor(x/scale)]],p=(y*width+x)*4;
      data[p]=color[0];data[p+1]=color[1];data[p+2]=color[2];data[p+3]=255;
    }
    return {width,height:width,stride:width*4,data};
  }
  /** Present one complete raster; VNC may still deliver partial screen updates. */
  function drawCanvas(canvas,bitmap) {
    canvas.width=bitmap.width;canvas.height=bitmap.height;
    const context=canvas.getContext('2d');context.imageSmoothingEnabled=false;
    context.putImageData(new ImageData(bitmap.data,bitmap.width,bitmap.height),0,0);
  }
  return {PALETTE,renderFrame,drawCanvas};
});
