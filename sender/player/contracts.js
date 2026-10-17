/** Implemented by player.js.
 * Keep playback separate from PreparedTransfer and the pure renderer.
 * @typedef {5|10|15|20|25|30} TargetFps
 * @typedef {'ready'|'playing'|'paused'|'stopped'|'error'} PlaybackState
 * @typedef {{state:PlaybackState,loop:number,frameIndex:number|null,totalFrames:number,
 *   frameType:'metadata'|'data'|null,blockIndex:number|null,totalBlocks:number,
 *   requestedFps:TargetFps,scale:number,visibleFrames:number,measuredLocalFps:number,
 *   localFrameIntervalMs:number|null,error:string|null}} PlaybackProgress
 * loop is 1-based when a frame is shown, otherwise 0; frameIndex is 0-based within
 * the complete cycle, including metadata before each group of at most 64 blocks.
 * @typedef {Object} FramePlayer
 * @property {function():Promise<void>} start Schedule playback; resume after pause, rewind after stop, preserve ID.
 * @property {function():void} pause Stop scheduling without destroying transfer state.
 * @property {function():void} stop Cancel scheduling/in-flight reads; sender never auto-stops.
 * @property {function(TargetFps):void} setRate Validate the six discrete rates.
 * @property {function(number):void} setScale Set integer pixels/symbol, never logical grid size.
 * @property {function():PlaybackProgress} getProgress Return a fresh progress snapshot.
 *
 * Factory signature:
 * createPlayer(transfer: PreparedTransfer, canvas: HTMLCanvasElement,
 *   onProgress: (PlaybackProgress) => void, options?: {fps?:TargetFps,scale?:number}): FramePlayer
 *
 * Implementation rules: use performance.now() deadlines, never equate rAF with VNC
 * delivery; hold a completed frame until its deadline; skip overdue timer ticks,
 * not logical data blocks; emit metadata according to cycleFrames(); bound read-ahead;
 * cancel old asynchronous work using a generation token. Report tab throttling.
 * Errors are terminal: prepare the file again. Stop clears the image, pause holds
 * it. Local FPS counts scheduled frame presentations, excluding scale-only redraws.
 */
