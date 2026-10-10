struct Out { @builtin(position) position:vec4<f32>, @location(0) color:vec4<f32> }
@vertex fn vs(@location(0) p:vec2<f32>,@location(1) c:vec4<f32>)->Out {
    var o:Out; o.position=vec4<f32>(p,0.0,1.0);o.color=c;return o;
}
@fragment fn fs(o:Out)->@location(0) vec4<f32>{return o.color;}
