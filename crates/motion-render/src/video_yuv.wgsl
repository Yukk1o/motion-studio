struct Params { size_rotation_standard: vec4<u32>, range_phase: vec4<u32>, output_size: vec4<u32> }
@group(0) @binding(0) var y_plane: texture_2d<u32>;
@group(0) @binding(1) var uv_plane: texture_2d<u32>;
@group(0) @binding(2) var<uniform> params: Params;

@vertex fn vertex_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
}
fn convert_pixel(pos: vec4<f32>) -> vec4<f32> {
    let size = params.size_rotation_standard.xy;
    let rotated = params.size_rotation_standard.z == 90u || params.size_rotation_standard.z == 270u;
    let display_size = select(size, size.yx, rotated);
    let dst = min(vec2<u32>(pos.xy * vec2<f32>(display_size) / vec2<f32>(params.output_size.xy)), display_size - vec2<u32>(1u));
    var src = dst;
    switch params.size_rotation_standard.z {
        case 90u: { src = vec2<u32>(dst.y, size.y - 1u - dst.x); }
        case 180u: { src = size - vec2<u32>(1u) - dst; }
        case 270u: { src = vec2<u32>(size.x - 1u - dst.y, dst.x); }
        default: {}
    }
    let yy = i32(textureLoad(y_plane, vec2<i32>(src), 0).x);
    let uv_pos = (src + params.range_phase.yz) / 2u;
    var samples: vec2<u32>;
    if params.range_phase.w == 2u {
        let ux = uv_pos.x;
        let vx = textureDimensions(uv_plane).x + ux;
        let u_pair = textureLoad(uv_plane, vec2<i32>(i32(ux / 2u), i32(uv_pos.y)), 0).xy;
        let v_pair = textureLoad(uv_plane, vec2<i32>(i32(vx / 2u), i32(uv_pos.y)), 0).xy;
        samples = vec2<u32>(select(u_pair.x, u_pair.y, (ux & 1u) != 0u),
                           select(v_pair.x, v_pair.y, (vx & 1u) != 0u));
    } else {
        samples = textureLoad(uv_plane, vec2<i32>(uv_pos), 0).xy;
        if params.range_phase.w == 1u { samples = samples.yx; }
    }
    let uv = vec2<i32>(samples) - vec2<i32>(128);
    let u = uv.x; let v = uv.y;
    var rgb: vec3<i32>;
    if params.range_phase.x == 1u {
        if params.size_rotation_standard.w == 1u {
            rgb = vec3<i32>(256 * yy + 403 * v, 256 * yy - 48 * u - 120 * v, 256 * yy + 475 * u);
        } else {
            rgb = vec3<i32>(256 * yy + 359 * v, 256 * yy - 88 * u - 183 * v, 256 * yy + 454 * u);
        }
    } else {
        let c = 298 * (yy - 16);
        if params.size_rotation_standard.w == 1u {
            rgb = vec3<i32>(c + 459 * v, c - 55 * u - 136 * v, c + 541 * u);
        } else {
            rgb = vec3<i32>(c + 409 * v, c - 100 * u - 208 * v, c + 516 * u);
        }
    }
    rgb = clamp((rgb + vec3<i32>(128)) >> vec3<u32>(8u), vec3<i32>(0), vec3<i32>(255));
    return vec4<f32>(vec3<f32>(rgb) / 255.0, 1.0);
}
@fragment fn fragment_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    return convert_pixel(pos);
}
// Backends without format-reinterpreting views write to an sRGB attachment.
// Linearize here so its automatic encoding preserves the same RGB bytes.
@fragment fn fragment_srgb(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let c = convert_pixel(pos);
    let linear = select(pow((c.rgb + 0.055) / 1.055, vec3<f32>(2.4)), c.rgb / 12.92, c.rgb <= vec3<f32>(0.04045));
    return vec4<f32>(linear, c.a);
}
