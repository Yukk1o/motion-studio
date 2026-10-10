fn main_fx(p: vec2<f32>) -> vec4<f32> {
if fx.params[3].x == 0.0 || fx.params[4].x == 0.0 { return vec4(0.0); }
let tile = max(vec2(1.0),fx.input_region.zw * vec2(fx.params[1].x,fx.params[2].x) * .01);
var cell = (p - fx.params[0].xy) / tile + .5;
let phase = fx.params[6].x / 360.0;
if fx.params[7].x > .5 { cell.x -= (floor(cell.y) - 2.0*floor(floor(cell.y)*.5))*phase; }
else { cell.y -= (floor(cell.x) - 2.0*floor(floor(cell.x)*.5))*phase; }
var uv = fract(cell);
if fx.params[5].x > .5 {
    let parity = floor(cell) - 2.0*floor(floor(cell)*.5);
    uv = select(uv,1.0-uv,parity > vec2(.5));
}
let q = fx.input_region.xy + uv * fx.input_region.zw;
if fx.params[1].x == 0.0 || fx.params[2].x == 0.0 {
    var sum=vec4(0.0);
    for(var i=0;i<64;i=i+1) {
        var probe=q;
        if fx.params[1].x == 0.0 && fx.params[2].x == 0.0 {
            probe=fx.input_region.xy+(vec2(f32(i%8),f32(i/8))+.5)/8.0*fx.input_region.zw;
        } else {
            if fx.params[1].x == 0.0 { probe.x=fx.input_region.x+(f32(i)+.5)/64.0*fx.input_region.z; }
            if fx.params[2].x == 0.0 { probe.y=fx.input_region.y+(f32(i)+.5)/64.0*fx.input_region.w; }
        }
        let v=sample_input(probe);sum+=vec4(v.rgb*v.a,v.a);
    }
    let avg=sum/64.0;return vec4(select(vec3(0.0),avg.rgb/max(avg.a,.000001),avg.a > .000001),avg.a);
}
return sample_input(q);
}
