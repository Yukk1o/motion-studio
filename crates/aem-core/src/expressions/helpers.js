// Independently implemented host functions. This file runs inside each expression call.
const __msD = JSON.parse(globalThis.__msData);
var time = __msD.time, value = __msD.value, index = __msD.index, numKeys = __msD.keys.length;
var thisComp = Object.freeze(__msD.comp), thisLayer = __msD.layer;
const __msArray = Array.isArray;
function __msBinary(a,b,f) {
    if (!__msArray(a) && !__msArray(b)) return f(a,b);
    const n = Math.max(__msArray(a)?a.length:0, __msArray(b)?b.length:0);
    if (n > 4) throw Error("vectors must have at most four components");
    return Array.from({length:n}, (_,i)=>f(__msArray(a)?(a[i]??0):a,__msArray(b)?(b[i]??0):b));
}
function __msAdd(a,b) { return __msBinary(a,b,(x,y)=>x+y); }
function __msSub(a,b) { return __msBinary(a,b,(x,y)=>x-y); }
function __msMul(a,b) { return __msBinary(a,b,(x,y)=>x*y); }
function __msDiv(a,b) { return __msBinary(a,b,(x,y)=>x/y); }
function __msMod(a,b) { return __msBinary(a,b,(x,y)=>x%y); }
function __msNeg(a) { return __msArray(a)?a.map(x=>-x):-a; }
var add=__msAdd, sub=__msSub, mul=__msMul, div=__msDiv;
function clamp(v,lo,hi) { return __msBinary(__msBinary(v,lo,Math.max),hi,Math.min); }
function linear(t,a,b,c,d) {
    if(arguments.length===3) { c=a;d=b;a=0;b=1; }
    return add(c,mul(sub(d,c),clamp((t-a)/(b-a),0,1)));
}
function __msEase(t,a,b,c,d,kind) {
    t=clamp((t-a)/(b-a),0,1);
    t=kind===1?t*t:kind===2?1-(1-t)*(1-t):t*t*(3-2*t);
    return add(c,mul(sub(d,c),t));
}
function ease(t,a,b,c,d) { if(arguments.length===3){c=a;d=b;a=0;b=1;} return __msEase(t,a,b,c,d,0); }
function easeIn(t,a,b,c,d) { if(arguments.length===3){c=a;d=b;a=0;b=1;} return __msEase(t,a,b,c,d,1); }
function easeOut(t,a,b,c,d) { if(arguments.length===3){c=a;d=b;a=0;b=1;} return __msEase(t,a,b,c,d,2); }
function length(a,b) { if(b!==undefined)a=sub(a,b); return __msArray(a)?Math.hypot(...a):Math.abs(a); }
function normalize(a) { var n=length(a); if(!n)throw Error("cannot normalize zero"); return div(a,n); }
function dot(a,b) { return mul(a,b).reduce((s,v)=>s+v,0); }
function cross(a,b) { return [(a[1]??0)*(b[2]??0)-(a[2]??0)*(b[1]??0),(a[2]??0)*b[0]-a[0]*(b[2]??0),a[0]*b[1]-a[1]*b[0]]; }
function degreesToRadians(x) { return x*Math.PI/180; }
function radiansToDegrees(x) { return x*180/Math.PI; }
function framesToTime(f,fps=thisComp.frameRate) { return f/fps; }
function timeToFrames(t=time,fps=thisComp.frameRate,isDuration=false) { return isDuration?Math.sign(t)*Math.ceil(Math.abs(t*fps)):Math.floor(t*fps); }
function valueAtTime(t) { if(!Number.isFinite(t))throw Error("invalid sample time"); return JSON.parse(globalThis.__msSample(t)); }
function velocityAtTime(t) { const h=thisComp.frameDuration/100; return div(sub(valueAtTime(t+h),valueAtTime(t-h)),2*h); }
function speedAtTime(t) { return length(velocityAtTime(t)); }
function key(n) { if(typeof n!=="number"||!Number.isInteger(n)||n<1||n>numKeys)throw Error("key index outside 1..numKeys"); return {...__msD.keys[n-1],index:n}; }
function nearestKey(t) { if(!numKeys)throw Error("property has no keys"); var n=1; for(var i=2;i<=numKeys;i++)if(Math.abs(key(i).time-t)<Math.abs(key(n).time-t))n=i; return key(n); }
function __msLoop(type,count,out) {
    if(!["cycle","pingpong","offset","continue"].includes(type))throw Error("unsupported loop type");
    if(!Number.isInteger(count)||count<0)throw Error("invalid loop key count");
    if(numKeys<2) return value;
    const n=count===0?numKeys-1:Math.min(count,numKeys-1);
    const a=key(out?numKeys-n:1).time,b=key(out?numKeys:1+n).time,d=b-a;
    if(out?time<=b:time>=a) return value;
    if(type==="continue") {
        const t=out?b:a,h=thisComp.frameDuration/100;
        const v=div(sub(valueAtTime(out?t:t+h),valueAtTime(out?t-h:t)),h);
        return add(valueAtTime(t),mul(v,time-t));
    }
    var cycles=Math.floor((time-a)/d),u=((time-a)%d+d)%d;
    if(type==="pingpong" && Math.abs(cycles%2)===1)u=d-u;
    var result=valueAtTime(a+u);
    return type==="offset"?add(result,mul(sub(valueAtTime(b),valueAtTime(a)),cycles)):result;
}
function loopOut(type="cycle",numKeyframes=0) { return __msLoop(type,numKeyframes,true); }
function loopIn(type="cycle",numKeyframes=0) { return __msLoop(type,numKeyframes,false); }
function __msHash(x) { x=Math.imul(x^(x>>>16),0x7feb352d); x=Math.imul(x^(x>>>15),0x846ca68b); return (x^(x>>>16))>>>0; }
let __msSeed=0, __msWiggleSeed=__msD.seed;
function seedRandom(offset=0,timeless=false) { __msWiggleSeed=__msHash(__msD.seed^(offset|0)); __msSeed=__msHash(__msWiggleSeed^(timeless?0:__msHash(Math.floor(time*1000000)))); }
function __msUnit() { __msSeed=(__msSeed+0x9e3779b9)>>>0; return __msHash(__msSeed)/4294967296; }
function random(a=1,b) { if(b===undefined){b=a;a=__msArray(b)?b.map(()=>0):0;} return add(a,mul(sub(b,a),__msArray(b)||__msArray(a)?Array.from({length:Math.max(a.length??0,b.length??0)},__msUnit):__msUnit())); }
seedRandom(); Math.random=__msUnit;
function __msNoise(t,s) { const k=Math.floor(t),u=t-k,v=u*u*(3-2*u); return ((__msHash(k^s)/4294967296)*(1-v)+(__msHash((k+1)^s)/4294967296)*v)*2-1; }
function wiggle(freq,amp,octaves=1,ampMult=.5,t=time) {
    if(![freq,amp,ampMult,t].every(Number.isFinite)||freq<0||!Number.isInteger(octaves)||octaves<1||octaves>8)throw Error("invalid wiggle arguments");
    var base=valueAtTime(t),n=__msArray(base)?base.length:1;
    var delta=Array.from({length:n},(_,c)=>{var x=0,f=freq,a=amp;for(var o=0;o<octaves;o++){x+=a*__msNoise(t*f,__msHash(__msWiggleSeed^((c+1)*131+o*977)));f*=2;a*=ampMult;}return x;});
    return add(base,__msArray(base)?delta:delta[0]);
}
var thisProperty = Object.freeze({value, numKeys, key, nearestKey, valueAtTime, velocityAtTime, speedAtTime, loopIn, loopOut, wiggle,
    get velocity(){return velocityAtTime(time);}, get speed(){return speedAtTime(time);}});
Object.assign(thisLayer,{add,sub,mul,div,clamp,linear,ease,easeIn,easeOut}); Object.freeze(thisLayer);
