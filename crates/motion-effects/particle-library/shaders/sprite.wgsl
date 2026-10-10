fn main_sprite(uv:vec2<f32>,c:vec4<f32>,style:vec4<f32>)->vec4<f32>{
    if style.x > 5.5 { return c * textureSampleLevel(input_image,input_sampler,uv,0.0); }
    let p=(uv-0.5)*2.0;let r=length(p);var coverage=0.0;
    if style.x<0.5 {coverage=exp(-r*r*6.0)*(1.0-smoothstep(0.8,1.0,r));}
    else if style.x<1.5 {coverage=exp(-pow((r-0.65)*18.0,2.0));}
    else if style.x<2.5 {coverage=(1.0-smoothstep(0.65,0.85,r))*0.35;}
    else if style.x<3.5 {coverage=exp(-p.y*p.y*120.0)*(1.0-smoothstep(0.1,1.0,abs(p.x)));}
    else {let angle=atan2(p.y,p.x);let spokes=pow(abs(cos(angle*style.y*0.5)),24.0);coverage=exp(-r*4.0)*spokes*(1.0-smoothstep(0.8,1.0,r));}
    let fringe=vec3(1.0+style.z*p.x,1.0,1.0-style.z*p.x);
    return vec4(c.rgb*fringe*coverage,c.a*coverage);
}
