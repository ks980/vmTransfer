/* Test-only PNG writer and deterministic pixel impairments. No runtime dependencies. */
const zlib=require('node:zlib');
const codec=require('../sender/protocol/codec.js');
function png(bitmap) {
  const header=Buffer.alloc(13);header.writeUInt32BE(bitmap.width,0);header.writeUInt32BE(bitmap.height,4);
  header[8]=8;header[9]=6;
  const raw=Buffer.alloc((bitmap.width*4+1)*bitmap.height);
  for(let y=0;y<bitmap.height;y++) Buffer.from(bitmap.data.buffer,bitmap.data.byteOffset+y*bitmap.stride,bitmap.width*4).copy(raw,y*(bitmap.width*4+1)+1);
  function chunk(type,bytes) {
    const body=Buffer.concat([Buffer.from(type),bytes]),head=Buffer.alloc(4),tail=Buffer.alloc(4);
    head.writeUInt32BE(bytes.length);tail.writeUInt32BE(codec.crc32(body));return Buffer.concat([head,body,tail]);
  }
  return Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]),chunk('IHDR',header),chunk('IDAT',zlib.deflateSync(raw)),chunk('IEND',Buffer.alloc(0))]);
}
function bitmap(width,height) {return {width,height,stride:width*4,data:new Uint8ClampedArray(width*height*4)};}
function resize(input,sx,sy=sx,method='nearest') {
  const out=bitmap(Math.round(input.width*sx),Math.round(input.height*sy));
  const px=out.width/input.width,py=out.height/input.height;
  function at(x,y,c) {return input.data[(Math.max(0,Math.min(input.height-1,y))*input.width+Math.max(0,Math.min(input.width-1,x)))*4+c];}
  for(let y=0;y<out.height;y++)for(let x=0;x<out.width;x++) {
    const fx=(x+.5)/px-.5,fy=(y+.5)/py-.5;
    for(let c=0;c<4;c++) {
      let value;
      if(method==='nearest') value=at(Math.round(fx),Math.round(fy),c);
      else {
        const ix=Math.floor(fx),iy=Math.floor(fy),dx=fx-ix,dy=fy-iy;
        value=at(ix,iy,c)*(1-dx)*(1-dy)+at(ix+1,iy,c)*dx*(1-dy)+at(ix,iy+1,c)*(1-dx)*dy+at(ix+1,iy+1,c)*dx*dy;
      }
      out.data[(y*out.width+x)*4+c]=Math.round(value);
    }
  }
  return out;
}
function rotate(input,turns) {
  let out=input;
  for(let i=0;i<turns;i++) {
    const next=bitmap(out.height,out.width);
    for(let y=0;y<out.height;y++)for(let x=0;x<out.width;x++)next.data.set(out.data.subarray((y*out.width+x)*4,(y*out.width+x)*4+4),(x*next.width+out.height-1-y)*4);
    out=next;
  }
  return out;
}
function pad(input,left=37,top=21,right=53,bottom=31) {
  const out=bitmap(left+input.width+right,top+input.height+bottom);
  for(let y=0;y<out.height;y++)for(let x=0;x<out.width;x++) {
    const value=((Math.floor(x/11)+Math.floor(y/13))%2)?150:190;
    out.data.set([value,value,value,255],(y*out.width+x)*4);
  }
  for(let y=0;y<input.height;y++)out.data.set(input.data.subarray(y*input.width*4,(y+1)*input.width*4),((top+y)*out.width+left)*4);
  return out;
}
function color(input,seed=57) {
  const out={...input,data:new Uint8ClampedArray(input.data)};
  for(let i=0;i<out.data.length;i+=4)for(let c=0;c<3;c++) {
    seed=(Math.imul(seed,1664525)+1013904223)>>>0;
    const noise=(seed%25)-12;
    out.data[i+c]=Math.round(input.data[i+c]*[.92,1.05,.97][c]+[8,3,12][c]+noise);
  }
  return out;
}
/** Copy the lower portion of a newer frame, simulating a partial VNC update. */
function mix(a,b,split=.5) {
  if(a.width!==b.width||a.height!==b.height)throw Error('mix size mismatch');
  const out={...a,data:new Uint8ClampedArray(a.data)},start=Math.floor(a.height*split)*a.stride;
  out.data.set(b.data.subarray(start),start);return out;
}
module.exports={png,bitmap,resize,rotate,pad,color,mix};
