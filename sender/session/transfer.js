/* File/session logic, independent of playback timers and browser UI. */
(function(root,factory) {
  if(typeof module==='object' && module.exports) module.exports=factory(require('../protocol/codec.js'),require('../vendor/sha256.js'));
  else (root.VMT=root.VMT||{}).transfer=factory(root.VMT.codec,root.sha256);
})(typeof globalThis!=='undefined'?globalThis:this,function(codec,sha256) {
  'use strict';
  /** @typedef {{name:string,size:number,readAt:function(number,number,AbortSignal=):Promise<Uint8Array>}} ByteSource
   * readAt(offset,length,signal) must return exactly length bytes or reject. It must
   * represent one immutable local file. Network-backed sources are not allowed.
   */
  /** @typedef {{metadata:Object,metadataFrame:Uint8Array,totalBlocks:number,matrixSize:128|160|256,readFrame:function(number,AbortSignal=):Promise<Uint8Array>}} PreparedTransfer */
  function checkAbort(signal) { if(signal && signal.aborted) { const e=new Error('preparation cancelled');e.name='AbortError';throw e; } }
  /** Wrap a browser File using bounded Blob slices and abortable FileReader. */
  function fileSource(file) {
    return {name:file.name,size:file.size,readAt(offset,length,signal) {
      return new Promise((resolve,reject)=> {
        try { checkAbort(signal); } catch(e) { reject(e);return; }
        const reader=new FileReader(), abort=()=>reader.abort();
        const cleanup=()=>{ if(signal) signal.removeEventListener('abort',abort); };
        reader.onload=()=>{ cleanup();resolve(new Uint8Array(reader.result)); };
        reader.onerror=()=>{ cleanup();reject(reader.error || new Error('file read failed')); };
        reader.onabort=()=>{ cleanup();const e=new Error('file read cancelled');e.name='AbortError';reject(e); };
        if(signal) signal.addEventListener('abort',abort,{once:true});
        reader.readAsArrayBuffer(file.slice(offset,offset+length));
      });
    }};
  }
  /** Hash before playback, yielding after each 1 MiB. No duplicate whole-file buffer.
   * options.matrixSize selects a predefined profile, immutable for this transfer.
   * options.id is only for deterministic fixtures; production uses crypto.getRandomValues.
   * @param {ByteSource} source
   * @param {{id?:Uint8Array,matrixSize?:128|160|256,signal?:AbortSignal,onProgress?:function(number,number)}} options
   * @returns {Promise<PreparedTransfer>}
   */
  async function prepareTransfer(source,options) {
    options=options||{}; checkAbort(options.signal);
    const id=options.id ? new Uint8Array(options.id) : crypto.getRandomValues(new Uint8Array(16));
    const name=source.name.split(/[\\/]/).pop() || 'received.bin';
    const profile=codec.profileForSize(options.matrixSize===undefined?128:options.matrixSize),blockSize=profile.blockSize;
    const metadata={id,name,matrixSize:profile.matrixSize,fileSize:source.size,sha256:new Uint8Array(32)};
    codec.validateMetadata(metadata);
    const hash=sha256.create(); // create/update uses the JS incremental implementation, also in Node.
    if(options.onProgress) options.onProgress(0,source.size);
    for(let offset=0;offset<source.size;) {
      checkAbort(options.signal);
      const length=Math.min(1024*1024,source.size-offset),data=await source.readAt(offset,length,options.signal);
      checkAbort(options.signal);
      if(!(data instanceof Uint8Array)||data.length!==length) throw new Error('short file read');
      hash.update(data);offset+=length;
      if(options.onProgress) options.onProgress(offset,source.size);
      await new Promise(resolve=>setTimeout(resolve,0));
    }
    checkAbort(options.signal);metadata.sha256=new Uint8Array(hash.array());
    const metadataFrame=codec.encodeMetadata(metadata),totalBlocks=Math.ceil(source.size/blockSize);
    return {metadata,metadataFrame,totalBlocks,matrixSize:profile.matrixSize,async readFrame(index,signal) {
      checkAbort(signal);
      if(!Number.isInteger(index)||index<0||index>=totalBlocks) throw new RangeError('block index out of range');
      const length=Math.min(blockSize,source.size-index*blockSize);
      const payload=await source.readAt(index*blockSize,length,signal);checkAbort(signal);
      return codec.encodeData(metadata,index,payload);
    }};
  }
  /** One deterministic cycle; callers restart it indefinitely, without feedback.
   * @yields {{type:'metadata'|'data',index:number}} Metadata index is 0xffffffff.
   */
  function* cycleFrames(totalBlocks) {
    if(!Number.isInteger(totalBlocks)||totalBlocks<0||totalBlocks>codec.MAX_BLOCKS) throw new RangeError('invalid block count');
    if(totalBlocks===0) { yield {type:'metadata',index:codec.META_INDEX};return; }
    for(let index=0;index<totalBlocks;index++) {
      if(index%64===0) yield {type:'metadata',index:codec.META_INDEX};
      yield {type:'data',index};
    }
  }
  return {fileSource,prepareTransfer,cycleFrames};
});
