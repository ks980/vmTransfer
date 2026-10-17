/* VMT1 binary codec. Browser globals and CommonJS share this exact implementation. */
(function (root, factory) {
  var api = factory();
  if (typeof module === 'object' && module.exports) module.exports = api;
  else (root.VMT = root.VMT || {}).codec = api;
})(typeof globalThis !== 'undefined' ? globalThis : this, function () {
  'use strict';
  // Legacy/default profile-1 constants; use PROFILES for other dimensions.
  const FRAME_SIZE = 4096, BLOCK_SIZE = 4028, MAX_FILE_SIZE = 1073741824;
  const MAX_BLOCKS = Math.ceil(MAX_FILE_SIZE / (128*128/8-68)), META_INDEX = 0xffffffff;
  const COLOR_MODES=Object.freeze([2,4,8,16,24]); // 24 means one exact RGB888 symbol.

  // Profiles are predefined so geometry can be located before parsing a header.
  const PROFILES=Object.freeze([[128,128],[160,160],[256,256],[512,512],[1028,512],[512,256],[256,128]]
    .map(([matrixWidth,matrixHeight],i)=>Object.freeze({
      id:i+1,matrixWidth,matrixHeight,
      matrixSize:matrixWidth===matrixHeight?matrixWidth:matrixWidth+'x'+matrixHeight,
      frameSize:matrixWidth*matrixHeight/4,blockSize:matrixWidth*matrixHeight/4-68,
      logicalWidth:matrixWidth+32,logicalHeight:matrixHeight+32,
      logicalSize:matrixWidth===matrixHeight?matrixWidth+32:null,
      maxScale:Math.min(16,Math.floor(8192/(matrixWidth+32)),Math.floor(8192/(matrixHeight+32)),
        Math.floor(Math.sqrt((32*1024*1024)/((matrixWidth+32)*(matrixHeight+32)))))
    })));
  function profileForSize(size) {
    const profile=PROFILES.find(p=>String(p.matrixSize)===String(size));
    if(!profile)throw new RangeError('matrix size must be 128, 160, 256, 512, 1028x512, 512x256 or 256x128');
    return profile;
  }
  function layout(matrixSize,colorMode) {
    const profile=profileForSize(matrixSize),mode=colorMode===undefined?4:Number(colorMode);
    if(!COLOR_MODES.includes(mode))throw new RangeError('unsupported color mode');
    const frameSize=profile.matrixWidth*profile.matrixHeight*(mode===24?3:Math.log2(mode)/8);
    return {profile,mode,version:mode===4?1:2,frameSize,blockSize:frameSize-68};
  }
  function layoutForFrame(data) {
    bytes(data,undefined,'frame bytes required');
    if(data.length<68)throw new Error('incorrect frame length');
    const p=PROFILES.find(p=>p.id===data[6]);
    if(!p)throw new Error('unsupported header profile');
    if(data[4]===1&&data[7]!==0)throw new Error('unsupported header flags');
    const mode=data[4]===1?4:data[4]===2?data[7]:0;
    const result=layout(p.matrixSize,mode);
    if(result.version!==data[4]||result.frameSize!==data.length)throw new Error('unsupported header frame length/version');
    return result;
  }
  function metadataLayout(meta) {return layout(meta.matrixSize===undefined?128:meta.matrixSize,meta.colorMode);}
  /** @typedef {{id: Uint8Array, fileSize: number, name: string, sha256: Uint8Array, matrixSize?:128|160|256|512|"1028x512"|"512x256"|"256x128",colorMode?:2|4|8|16|24}} Metadata
   * ID and digest are raw bytes, not hexadecimal text. fileSize is an exact integer.
   */
  function integer(value, min, max, label) {
    if (!Number.isSafeInteger(value) || value < min || value > max) throw new RangeError(label);
  }
  function bytes(value, length, label) {
    if (!(value instanceof Uint8Array) || (length !== undefined && value.length !== length)) {
      throw new TypeError(label);
    }
  }
  /** CRC-32/ISO-HDLC of precisely the supplied bytes. */
  function crc32(data) {
    let crc = 0xffffffff;
    for (const byte of data) {
      crc ^= byte;
      for (let bit = 0; bit < 8; bit++) crc = (crc >>> 1) ^ ((crc & 1) ? 0xedb88320 : 0);
    }
    return (crc ^ 0xffffffff) >>> 0;
  }
  /** Validate metadata before encoding or performing file-size-derived allocations. */
  function validateMetadata(meta) {
    metadataLayout(meta);
    bytes(meta.id, 16, 'transfer ID must have 16 bytes');
    bytes(meta.sha256, 32, 'SHA-256 must have 32 bytes');
    integer(meta.fileSize, 0, MAX_FILE_SIZE, 'file exceeds v1 size limit');
    if (typeof meta.name !== 'string') throw new TypeError('filename must be a string');
    const name = new TextEncoder().encode(meta.name);
    // Reject isolated UTF-16 surrogates instead of silently changing the filename.
    if (new TextDecoder('utf-8', {fatal: true, ignoreBOM: true}).decode(name) !== meta.name) throw new Error('invalid filename Unicode');
    integer(name.length, 1, 1024, 'filename must contain 1..1024 UTF-8 bytes');
    return name;
  }
  /** Internal canonical frame writer; unused payload bytes and reserved bytes stay zero. */
  function frame(type, id, index, total, payload, spec) {
    const out = new Uint8Array(spec.frameSize), view = new DataView(out.buffer);
    out.set([86, 77, 84, 49, spec.version, type, spec.profile.id, spec.version===1?0:spec.mode]);
    out.set(id, 8);
    view.setUint32(24, index, true); view.setUint32(28, total, true);
    view.setUint16(32, payload.length & 0xffff, true);
    if(spec.version===2||spec.profile.id===5)view.setUint16(34, payload.length >>> 16, true);
    out.set(payload, 64);
    view.setUint32(out.length-4, crc32(out.subarray(0, out.length-4)), true);
    return out;
  }
  /** @param {Metadata} meta @returns {Uint8Array} Canonical profile-sized metadata frame (default: 4096 bytes). */
  function encodeMetadata(meta) {
    const spec=metadataLayout(meta), blockSize=spec.blockSize;
    const name = validateMetadata(meta), payload = new Uint8Array(46 + name.length);
    const view = new DataView(payload.buffer);
    // v1 caps at 1 GiB: the upper u32 of the wire u64 is always zero.
    view.setUint32(0, meta.fileSize, true); view.setUint32(8, blockSize, true);
    payload.set(meta.sha256, 12); view.setUint16(44, name.length, true); payload.set(name, 46);
    return frame(1, meta.id, META_INDEX, Math.ceil(meta.fileSize / blockSize), payload, spec);
  }
  /** Encode one exact block. A short payload is legal only at the final index. */
  function encodeData(meta, index, payload) {
    validateMetadata(meta);
    const spec=metadataLayout(meta), blockSize=spec.blockSize;
    const total = Math.ceil(meta.fileSize / blockSize);
    integer(index, 0, total - 1, 'block index out of range');
    bytes(payload, Math.min(blockSize, meta.fileSize - index * blockSize), 'incorrect block length');
    return frame(2, meta.id, index, total, payload, spec);
  }
  /** Parse and CRC-check a frame; useful for tooling, independently implemented in Rust. */
  function decodeFrame(data) {
    bytes(data,undefined,'frame bytes required');
    if(data.length<68)throw new Error('incorrect frame length');
    const expectedCrc=new DataView(data.buffer,data.byteOffset,data.byteLength).getUint32(data.length-4,true);
    if(expectedCrc!==crc32(data.subarray(0,data.length-4)))throw new Error('CRC mismatch');
    const spec=layoutForFrame(data),profile=spec.profile;
    const blockSize=spec.blockSize,end=data.length-4;
    const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
    if ([86,77,84,49].some((v,i) => data[i] !== v) ||
        data.subarray(spec.version===2||profile.id===5?36:34,64).some(v => v !== 0)) throw new Error('unsupported header');
    const type = data[5], index = view.getUint32(24,true), total = view.getUint32(28,true);
    const length = view.getUint16(32,true)+(spec.version===2||profile.id===5?view.getUint16(34,true)*65536:0);
    if (total > Math.ceil(MAX_FILE_SIZE/blockSize) || length > blockSize || data.subarray(64+length,end).some(v => v !== 0)) {
      throw new Error('invalid length/count/padding');
    }
    // Copy into plain Uint8Arrays: Buffer.slice() would retain a nonzero byteOffset.
    const payload = new Uint8Array(data.subarray(64,64+length)), id = new Uint8Array(data.subarray(8,24));
    if (type === 1) {
      if (index !== META_INDEX || length < 47) throw new Error('invalid metadata');
      const p = new DataView(payload.buffer);
      if (p.getUint32(4,true) !== 0 || p.getUint32(8,true) !== blockSize || p.getUint16(44,true) !== length-46) {
        throw new Error('invalid metadata fields');
      }
      const meta = {id, fileSize:p.getUint32(0,true), sha256:payload.slice(12,44),
        matrixSize:profile.matrixSize, name:new TextDecoder('utf-8',{fatal:true,ignoreBOM:true}).decode(payload.subarray(46))};
      if(spec.version===2)meta.colorMode=spec.mode;
      validateMetadata(meta);
      if (Math.ceil(meta.fileSize/blockSize) !== total) throw new Error('incorrect block count');
      return {type, id, index, total, payload, meta};
    }
    if (type !== 2 || total === 0 || index >= total || length === 0 || (index+1 < total && length !== blockSize)) {
      throw new Error('invalid data header');
    }
    return {type, id, index, total, payload};
  }
  /** Bytes -> two-bit symbols, most significant pair first. */
  function packSymbols(data) {
    let spec;
    try {spec=layoutForFrame(data);} catch(error) {
      // Raw legacy frames are used by standalone symbol tests/tooling.
      const p=PROFILES.find(p=>p.frameSize===data.length);if(!p)throw error;
      spec=layout(p.matrixSize,4);
    }
    const bits=spec.mode===24?24:Math.log2(spec.mode);
    const symbols = bits===24?new Uint32Array(spec.profile.matrixWidth*spec.profile.matrixHeight):
      new Uint8Array(spec.profile.matrixWidth*spec.profile.matrixHeight);
    if(bits===24)for(let i=0;i<symbols.length;i++)symbols[i]=(data[i*3]<<16)|(data[i*3+1]<<8)|data[i*3+2];
    else for(let i=0;i<symbols.length;i++) {
      let value=0;
      for(let j=0;j<bits;j++){const bit=i*bits+j;value=(value<<1)|((data[bit>>>3]>>>(7-(bit&7)))&1);}
      symbols[i]=value;
    }
    return symbols;
  }
  /** Indexed or RGB symbols -> bytes; mode and geometry come from fixed visual bootstrap. */
  function unpackSymbols(symbols,matrixSize,colorMode) {
    if(!(symbols instanceof Uint8Array||symbols instanceof Uint32Array))throw new TypeError('symbols required');
    const spec=layout(matrixSize===undefined?PROFILES.find(p=>p.matrixWidth*p.matrixHeight===symbols.length)?.matrixSize:matrixSize,colorMode);
    if(spec.profile.matrixWidth*spec.profile.matrixHeight!==symbols.length)throw new Error('incorrect symbol count');
    const bits=spec.mode===24?24:Math.log2(spec.mode),data=new Uint8Array(spec.frameSize);
    for(let i=0;i<symbols.length;i++) {
      if(symbols[i]>=Math.pow(2,bits)) throw new Error('invalid data symbol');
      if(bits===24){data[i*3]=symbols[i]>>>16;data[i*3+1]=(symbols[i]>>>8)&255;data[i*3+2]=symbols[i]&255;}
      else for(let j=0;j<bits;j++){const bit=i*bits+j;data[bit>>>3]|=((symbols[i]>>>(bits-1-j))&1)<<(7-(bit&7));}
    }
    return data;
  }
  return {PROFILES, COLOR_MODES, profileForSize, layout, layoutForFrame, FRAME_SIZE, BLOCK_SIZE, MAX_FILE_SIZE, MAX_BLOCKS, META_INDEX,
    crc32, validateMetadata, encodeMetadata, encodeData, decodeFrame, packSymbols, unpackSymbols};
});
