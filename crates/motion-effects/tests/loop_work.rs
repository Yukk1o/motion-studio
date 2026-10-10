use motion_effects::shader;

#[test]
fn compiled_loop_metric_keeps_the_existing_validation_product() {
    for (body, expected) in [
        ("", 1),
        ("for(var i:i32=0;i<0;i=i+1){c+=vec4(0.0);}", 1),
        ("for(var i:i32=0;i<3;i=i+1){c+=vec4(0.01);}", 3),
        (
            "for(var i:i32=0;i<3;i=i+1){c+=vec4(0.01);}for(var j:i32=0;j<5;j=j+1){c+=vec4(0.01);}",
            15,
        ),
        (
            "for(var i:i32=0;i<3;i=i+1){for(var j:i32=0;j<5;j=j+1){c+=vec4(0.01);}}",
            15,
        ),
        ("for(var i:i32=0;i<1024;i=i+1){c+=vec4(0.01);}", 1024),
        (
            "for(var i:i32=0;i<256;i=i+1){for(var j:i32=0;j<256;j=j+1){c+=vec4(0.01);}}",
            65536,
        ),
    ] {
        let source =
            format!("fn main_fx(p:vec2<f32>)->vec4<f32>{{var c=vec4(0.0);{body}return c;}}");
        let compiled = shader::compile(&source, "main_fx").unwrap();
        assert_eq!(compiled.loop_work, expected);
    }
}

#[test]
fn exported_metric_does_not_relax_loop_rejections() {
    for body in [
        "for(var i:i32=0;i<1025;i=i+1){c+=vec4(0.01);}",
        "for(var i:i32=0;i<257;i=i+1){for(var j:i32=0;j<257;j=j+1){c+=vec4(0.01);}}",
        "for(var i:i32=0;i<3;i=i+1){i=i+1;}",
        "while true {c+=vec4(0.01);}",
    ] {
        let source =
            format!("fn main_fx(p:vec2<f32>)->vec4<f32>{{var c=vec4(0.0);{body}return c;}}");
        assert!(shader::compile(&source, "main_fx").is_err());
    }
    let sprite = shader::compile_mode(
        "fn main_fx(p:vec2<f32>,c:vec4<f32>,s:vec4<f32>)->vec4<f32>{return c;}",
        "main_fx",
        true,
        false,
    )
    .unwrap();
    assert_eq!(sprite.loop_work, 1);
}
