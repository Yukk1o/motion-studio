struct Draw { mvp:mat4x4<f32>, color:vec4<f32>, extent_opacity:vec4<f32>, uv_scale:vec4<f32> };
@group(0) @binding(0) var<uniform> draw:Draw;
@group(1) @binding(0) var image:texture_2d<f32>;
@group(1) @binding(1) var image_sampler:sampler;
@group(2) @binding(0) var mask:texture_2d<f32>;
@group(2) @binding(1) var mask_sampler:sampler;
@group(3) @binding(0) var matte:texture_2d<f32>;
@group(3) @binding(1) var matte_sampler:sampler;
struct Out { @builtin(position) position:vec4<f32>, @location(0) uv:vec2<f32> };
@vertex fn vertex_main(@location(0) position:vec3<f32>,@location(1) uv:vec2<f32>)->Out {
    var o:Out;o.position=draw.mvp*vec4(position,1.0);o.uv=uv*draw.uv_scale.xy;return o;
}
fn pixel(o:Out)->vec4<f32> {
    let flags=u32(draw.extent_opacity.w);
    var coverage=1.0;
    if (flags&1u)!=0u {coverage*=textureSample(mask,mask_sampler,o.uv).r;}
    if (flags&2u)!=0u {
        var uv=(o.position.xy-draw.extent_opacity.xy)/draw.uv_scale.zw;
        if (flags&8u)!=0u {uv.y=1.0-uv.y;}
        var value=textureSample(matte,matte_sampler,uv).r;
        if (flags&4u)!=0u {value=1.0-value;}coverage*=value;
    }
    let c=textureSample(image,image_sampler,o.uv);let opacity=draw.color.a*draw.extent_opacity.z*coverage;
    return vec4(c.rgb*draw.color.rgb*opacity,c.a*opacity);
}
@fragment fn fragment_main(o:Out)->@location(0) vec4<f32>{return pixel(o);}
@fragment fn alpha_matte(o:Out)->@location(0) vec4<f32>{let c=pixel(o);return vec4(c.a,c.a,c.a,1.0);}
fn encode(c:vec3<f32>)->vec3<f32>{return select(1.055*pow(max(c,vec3(0.0)),vec3(1.0/2.4))-0.055,c*12.92,c<=vec3(0.0031308));}
@fragment fn luma_matte(o:Out)->@location(0) vec4<f32>{let c=pixel(o);let straight=select(vec3(0.0),c.rgb/max(c.a,0.000001),c.a>0.0);let value=dot(encode(straight),vec3(0.2126,0.7152,0.0722))*c.a;return vec4(value,value,value,1.0);}
