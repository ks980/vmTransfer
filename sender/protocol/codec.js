/* VMT1 binary codec. Browser globals and CommonJS share this exact implementation. */
(function (root, factory) {
  var api = factory();
  if (typeof module === 'object' && module.exports) module.exports = api;
  else (root.VMT = root.VMT || {}).codec = api;
})(typeof globalThis !== 'undefined' ? globalThis : this, function () {
  'use strict';
  // Legacy/default profile-1 constants; use PROFILES for other dimensions.
  const FRAME_SIZE = 4096, BLOCK_SIZE = 4028, MAX_FILE_SIZE = 1073741824;
  const MAX_BLOCKS = Math.ceil(MAX_FILE_SIZE / BLOCK_SIZE), META_INDEX = 0xffffffff;

  // Profiles are predefined so geometry can be located before parsing a header.
  const PROFILES=Object.freeze([128,160,256].map((matrixSize,i)=>Object.freeze({
    id:i+1,matrixSize,frameSize:matrixSize*matrixSize/4,blockSize:matrixSize*matrixSize/4-68,
    logicalSize:matrixSize+32
  })));
  function profileForSize(size) {
    const profile=PROFILES.find(p=>p.matrixSize===size);
    if(!profile)throw new RangeError('matrix size must be 128, 160 or 256');
    return profile;
  }
  function metadataProfile(meta) {return profileForSize(meta.matrixSize===undefined?128:meta.matrixSize);}
  /** @typedef {{id: Uint8Array, fileSize: number, name: string, sha256: Uint8Array, matrixSize?:128|160|256}} Metadata
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
    metadataProfile(meta);
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
  function frame(type, id, index, total, payload, profile) {
    const out = new Uint8Array(profile.frameSize), view = new DataView(out.buffer);
    out.set([86, 77, 84, 49, 1, type, profile.id, 0]);
    out.set(id, 8);
    view.setUint32(24, index, true); view.setUint32(28, total, true);
    view.setUint16(32, payload.length, true); out.set(payload, 64);
    view.setUint32(out.length-4, crc32(out.subarray(0, out.length-4)), true);
    return out;
  }
  /** @param {Metadata} meta @returns {Uint8Array} Canonical profile-sized metadata frame (default: 4096 bytes). */
  function encodeMetadata(meta) {
    const profile=metadataProfile(meta), blockSize=profile.blockSize;
    const name = validateMetadata(meta), payload = new Uint8Array(46 + name.length);
    const view = new DataView(payload.buffer);
    // v1 caps at 1 GiB: the upper u32 of the wire u64 is always zero.
    view.setUint32(0, meta.fileSize, true); view.setUint32(8, blockSize, true);
    payload.set(meta.sha256, 12); view.setUint16(44, name.length, true); payload.set(name, 46);
    return frame(1, meta.id, META_INDEX, Math.ceil(meta.fileSize / blockSize), payload, profile);
  }
  /** Encode one exact block. A short payload is legal only at the final index. */
  function encodeData(meta, index, payload) {
    validateMetadata(meta);
    const profile=metadataProfile(meta), blockSize=profile.blockSize;
    const total = Math.ceil(meta.fileSize / blockSize);
    integer(index, 0, total - 1, 'block index out of range');
    bytes(payload, Math.min(blockSize, meta.fileSize - index * blockSize), 'incorrect block length');
    return frame(2, meta.id, index, total, payload, profile);
  }
  /** Parse and CRC-check a frame; useful for tooling, independently implemented in Rust. */
  function decodeFrame(data) {
    bytes(data, undefined, 'frame bytes required');
    const profile=PROFILES.find(p=>p.frameSize===data.length);
    if(!profile)throw new Error('incorrect frame length');
    const blockSize=profile.blockSize,end=data.length-4;
    const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
    if (view.getUint32(end, true) !== crc32(data.subarray(0, end))) throw new Error('CRC mismatch');
    if ([86,77,84,49,1].some((v,i) => data[i] !== v) || data[6] !== profile.id || data[7] !== 0 ||
        data.subarray(34,64).some(v => v !== 0)) throw new Error('unsupported header');
    const type = data[5], index = view.getUint32(24,true), total = view.getUint32(28,true);
    const length = view.getUint16(32,true);
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
    bytes(data, undefined, 'frame bytes required');
    if(!PROFILES.some(p=>p.frameSize===data.length))throw new Error('incorrect frame length');
    const symbols = new Uint8Array(data.length*4);
    for (let i=0;i<data.length;i++) for(let j=0;j<4;j++) symbols[i*4+j]=(data[i] >>> (6-j*2)) & 3;
    return symbols;
  }
  /** Two-bit symbols -> bytes; rejects white/reserved symbols. */
  function unpackSymbols(symbols) {
    bytes(symbols, undefined, 'symbols required');
    if(!PROFILES.some(p=>p.matrixSize*p.matrixSize===symbols.length))throw new Error('incorrect symbol count');
    const data = new Uint8Array(symbols.length/4);
    for(let i=0;i<symbols.length;i++) {
      if(symbols[i]>3) throw new Error('invalid data symbol');
      data[i>>>2] |= symbols[i] << (6-(i%4)*2);
    }
    return data;
  }
  return {PROFILES, profileForSize, FRAME_SIZE, BLOCK_SIZE, MAX_FILE_SIZE, MAX_BLOCKS, META_INDEX,
    crc32, validateMetadata, encodeMetadata, encodeData, decodeFrame, packSymbols, unpackSymbols};
});
