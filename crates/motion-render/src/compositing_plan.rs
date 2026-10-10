//! Shared separable transfer shader used by native preview and GLES output.
use std::sync::{Arc, OnceLock};
pub const BLEND: &str = r#"
fn transfer(b:vec3<f32>,s:vec3<f32>,mode:u32)->vec3<f32> {
    if mode==1u{return min(b+s,vec3(1.0));}
    if mode==2u{return b*s;}
    if mode==3u{return b+s-b*s;}
    if mode==4u{return select(vec3(1.0)-2.0*(vec3(1.0)-b)*(vec3(1.0)-s),2.0*b*s,b<=vec3(0.5));}
    if mode==5u{return min(b,s);}
    if mode==6u{return max(b,s);}
    if mode==7u{return abs(b-s);}
    if mode==8u{return b+s-2.0*b*s;}
    if mode==9u{return max(b-s,vec3(0.0));}
    if mode==10u{return select(min(b/max(s,vec3(0.000001)),vec3(1.0)),vec3(1.0),s==vec3(0.0));}
    if mode==11u {let c=select(min(b/max(vec3(1.0)-s,vec3(0.000001)),vec3(1.0)),vec3(1.0),s==vec3(1.0));return select(c,vec3(0.0),b==vec3(0.0));}
    if mode==12u {let c=select(vec3(1.0)-min((vec3(1.0)-b)/max(s,vec3(0.000001)),vec3(1.0)),vec3(0.0),s==vec3(0.0));return select(c,vec3(1.0),b==vec3(1.0));}
    if mode==13u{return select(vec3(1.0)-2.0*(vec3(1.0)-b)*(vec3(1.0)-s),2.0*b*s,s<=vec3(0.5));}
    if mode==14u {let d=select(sqrt(b),((16.0*b-12.0)*b+4.0)*b,b<=vec3(0.25));return select(b+(2.0*s-1.0)*(d-b),b-(1.0-2.0*s)*b*(1.0-b),s<=vec3(0.5));}
    return s;
}
fn encode_srgb(c:vec3<f32>)->vec3<f32>{return select(1.055*pow(max(c,vec3(0.0)),vec3(1.0/2.4))-0.055,c*12.92,c<=vec3(0.0031308));}
fn decode_srgb(c:vec3<f32>)->vec3<f32>{return select(pow(max((c+0.055)/1.055,vec3(0.0)),vec3(2.4)),c/12.92,c<=vec3(0.04045));}
fn main_fx(p:vec2<f32>)->vec4<f32> {
    let source=sample_source(p);let backdrop=sample_input(p);
    let sa=clamp(source.a,0.0,1.0);let ba=clamp(backdrop.a,0.0,1.0);
    var s=select(vec3(0.0),source.rgb/max(sa,0.000001),sa>0.0);
    var b=select(vec3(0.0),backdrop.rgb/max(ba,0.000001),ba>0.0);
    if fx.params[0].y>0.5 {s=encode_srgb(s);b=encode_srgb(b);}
    let alpha=sa+ba-sa*ba;
    var color=(1.0-sa)*b*ba+(1.0-ba)*s*sa+sa*ba*transfer(clamp(b,vec3(0.0),vec3(1.0)),clamp(s,vec3(0.0),vec3(1.0)),u32(fx.params[0].x));
    if fx.params[0].y>0.5 {color=decode_srgb(color/max(alpha,0.000001))*alpha;}
    return vec4(color,alpha);
}
"#;
pub fn shader() -> Result<&'static Arc<motion_effects::shader::CompiledShader>, String> {
    static SHADER: OnceLock<Result<Arc<motion_effects::shader::CompiledShader>, String>> =
        OnceLock::new();
    SHADER
        .get_or_init(|| {
            motion_effects::shader::compile(BLEND, "main_fx")
                .map(Arc::new)
                .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(Clone::clone)
}
pub fn plane_programs() -> Result<&'static Vec<motion_effects::shader::GlslShader>, String> {
    static PROGRAMS: OnceLock<Result<Vec<motion_effects::shader::GlslShader>, String>> =
        OnceLock::new();
    PROGRAMS
        .get_or_init(|| {
            ["fragment_main", "alpha_matte", "luma_matte"]
                .into_iter()
                .map(|entry| {
                    motion_effects::shader::compile_host_program(
                        include_str!("plane_matte.wgsl"),
                        "vertex_main",
                        entry,
                    )
                    .map_err(|e| e.to_string())
                })
                .collect()
        })
        .as_ref()
        .map_err(Clone::clone)
}
