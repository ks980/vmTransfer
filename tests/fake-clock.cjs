/* Development-only deterministic monotonic clock. advance() executes browser-like
 * timers and Promise microtasks; jump() simulates a suspended/background event loop. */
class FakeClock {
  constructor(){this.time=0;this.serial=0;this.jobs=new Map();}
  now=()=>this.time;
  setTimeout=(callback,delay)=>{const id=++this.serial;this.jobs.set(id,{at:this.time+Math.max(0,delay),callback});return id;};
  clearTimeout=id=>{this.jobs.delete(id);};
  async flush(){for(let i=0;i<12;i++)await Promise.resolve();}
  jump(milliseconds){this.time+=milliseconds;}
  async advance(milliseconds){
    const end=this.time+milliseconds;await this.flush();
    for(let guard=0;guard<100000;guard++) {
      const next=Array.from(this.jobs).sort((a,b)=>a[1].at-b[1].at||a[0]-b[0])[0];
      if(!next||next[1].at>end){this.time=end;await this.flush();return;}
      this.time=Math.max(this.time,next[1].at);this.jobs.delete(next[0]);next[1].callback();await this.flush();
    }
    throw Error('timer runaway');
  }
}
module.exports={FakeClock};
