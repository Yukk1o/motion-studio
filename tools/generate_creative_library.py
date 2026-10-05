"""Generate the original creative subset of the Motion Studio core library.

Sapphire names identify visual references in the handoff, not verified matchNames.
The checked-in package is built separately with effect_tool pack.
"""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LIB = ROOT / 'crates/aem-effects/library'


def param(id, name, value, minimum=0, maximum=1, units='', kind='float', **extra):
    values = list(value) if isinstance(value, (tuple, list)) else [value]
    return dict(id=id, name=name, kind=kind, default=(values + [0] * 4)[:4], min=minimum,
                max=maximum, step=1 if kind in ('bool', 'enum') else .01, units=units,
                animatable=True, **extra)


def center():
    return param('center', '中心', [0, 0], -32768, 32768, 'px', 'vec2', center_default=True)


def edge(default=0):
    return param('edge', '边缘采样', default, 0, 3, kind='enum', options=['透明', '钳制', '重复', '镜像'])


def padding(id):
    return dict(op='multiply', a=dict(op='abs', value=dict(op='parameter', id=id)),
                b=dict(op='parameter', id='expand'))


COMMON = r'''
// All coordinates are untransformed layer pixels; all loops have literal bounds.
fn luma(c: vec3<f32>) -> f32 { return dot(c, vec3(0.2126, 0.7152, 0.0722)); }
fn turn(p: vec2<f32>, a: f32) -> vec2<f32> {
    return vec2(cos(a)*p.x-sin(a)*p.y, sin(a)*p.x+cos(a)*p.y);
}
fn hash32(value: u32) -> u32 {
    var v = value; v = (v ^ (v >> 16u))*0x7feb352du;
    v = (v ^ (v >> 15u))*0x846ca68bu; return v ^ (v >> 16u);
}
fn noise(cell: vec2<i32>, epoch: i32, salt: u32) -> f32 {
    let h = hash32(bitcast<u32>(cell.x)*0x9e3779b9u ^ bitcast<u32>(cell.y)*0x85ebca6bu
        ^ bitcast<u32>(epoch)*0xc2b2ae35u ^ bitcast<u32>(fx.clock.w) ^ salt);
    return f32(h >> 8u)/16777216.0;
}
fn smooth_noise(time: f32, salt: u32) -> f32 {
    let epoch = i32(floor(time)); let f = fract(time); let w = f*f*(3.0-2.0*f);
    return mix(noise(vec2<i32>(0), epoch, salt), noise(vec2<i32>(0), epoch+1, salt), w)*2.0-1.0;
}
fn bright(c: vec4<f32>, threshold: f32) -> vec4<f32> {
    let level = luma(c.rgb/max(c.a, 0.000001));
    let weight = max(level-threshold, 0.0)/max(level, 0.000001);
    return c*weight;
}
fn edge_light(p: vec2<f32>, threshold: f32) -> vec4<f32> {
    let left = sample_input(p-vec2(1.0, 0.0)); let right = sample_input(p+vec2(1.0, 0.0));
    let up = sample_input(p-vec2(0.0, 1.0)); let down = sample_input(p+vec2(0.0, 1.0));
    let edge = clamp(length(vec2(luma(right.rgb-left.rgb), luma(down.rgb-up.rgb))), 0.0, 1.0);
    let c = sample_input(p); let rgb = max(max(left.rgb, right.rgb), max(up.rgb, down.rgb));
    let alpha = max(max(left.a, right.a), max(up.a, down.a));
    return vec4(rgb, alpha)*max(edge-threshold, 0.0);
}
fn combine_light(source: vec4<f32>, light: vec4<f32>, strength: f32, color: vec3<f32>, affect_alpha: f32) -> vec4<f32> {
    let glow = max(light.rgb*color*strength, vec3(0.0));
    let coverage = clamp(max(glow.r, max(glow.g, glow.b)), 0.0, 1.0)*affect_alpha;
    let alpha = source.a+(1.0-source.a)*coverage;
    // Premultiplied linear output, including emitted light outside the source.
    return vec4(min(source.rgb+glow, vec3(alpha)), alpha);
}
'''

EFFECTS = []


def add(id, name, english, reference, params, body, *, passes=1, pad=None, alpha='premultiplied', space='linear', category='光效', differences=()):
    params = params + [param('effect_opacity', '效果不透明度', 100, 0, 100, '%')]
    for index, p in enumerate(params):
        body = body.replace('$'+p['id']+'$', f'fx.params[{index}]')
    assert '$' not in body
    definition = dict(id=id, name=name, english_name=english, category=category, params=params,
        passes=[dict(shader=f'shaders/{id}.wgsl', entry='main_fx') for _ in range(passes)], resources=[],
        padding=pad or dict(op='constant', value=0), edge_mode='transparent',
        edge_param='edge' if any(p['id']=='edge' for p in params) else None,
        working_space=space, alpha_mode=alpha, compatibility='approximate',
        compatibility_profile='motion-creative-sapphire-inspired-v1', reference_match_name='',
        reference_version='Sapphire AE documentation 2026.5; implementation unverified',
        known_differences=[f'视觉参考 {reference}；独立算法与参数子集，未完成 Sapphire 实机对照。',
            '8 bpc 有界输出，不提供 HDR、Mocha、外部背景/蒙版或跨时间素材输入。', *differences],
        required_capabilities=['single_frame', 'multipass', 'dynamic_bounds', 'color_profile'])
    EFFECTS.append((definition, COMMON + '\nfn main_fx(p: vec2<f32>) -> vec4<f32> {\n'+body+'\n}\n', reference))


LIGHT_PARAMS = [param('strength', '亮度', 1, 0, 4), param('threshold', '高光阈值', .5),
    param('radius', '半径', 16, 0, 512, 'px'), param('color', '光色', [1,1,1,1], kind='color'),
    param('affect_alpha', '扩展透明度', 1), param('expand', '扩展边界', 1, kind='bool'), edge()]
GLOW = r'''
let c = sample_input(p); if $strength$.x == 0.0 { return c; }
let radius = $radius$.x;
let vertical = fx.clock.z > 0.5;
let axis = select(vec2(1.0, 0.0), vec2(0.0, 1.0), vertical);
let step = max(1.0, radius/32.0); let sigma = max(radius/3.0, 0.15);
var sum = vec4(0.0); var total = 0.0;
for(var i:i32=-32;i<=32;i=i+1) {
    let distance = f32(i)*step;
    if abs(distance) <= ceil(radius) {
        let weight = exp(-0.5*distance*distance/(sigma*sigma));
        var light = sample_input(p+axis*distance);
        if !vertical { light = bright(light, $threshold$.x); }
        sum += light*weight; total += weight;
    }
}
let light = sum/max(total, 0.000001);
if !vertical { return light; }
return combine_light(sample_source(p), light, $strength$.x, $color$.rgb, $affect_alpha$.x);
'''
add('glow', '柔光发光', 'Glow', 'S_Glow', LIGHT_PARAMS, GLOW, passes=2, pad=padding('radius'),
    differences=['线性预乘高光提取与两次65点高斯采样；不提供 RGB 独立宽度或大气模型。'])
GLOW_EDGES = GLOW.replace('let vertical = fx.clock.z > 0.5;',
    'if fx.clock.z < 0.5 { return edge_light(p, $threshold$.x); }\nlet vertical = fx.clock.z > 1.5;')
GLOW_EDGES = GLOW_EDGES.replace('if !vertical { light = bright(light, $threshold$.x); }', '')
add('glow_edges', '边缘发光', 'Glow Edges', 'S_GlowEdges', LIGHT_PARAMS, GLOW_EDGES,
    passes=3, pad=padding('radius'), differences=['中心差分边缘提取后进行两次高斯采样。'])

RAY_PARAMS = [param('strength', '亮度', 1, 0, 4), param('threshold', '高光阈值', .5), center(),
    param('length', '光束长度', .8, 0, 2), param('decay', '衰减', 2, 0, 8),
    param('color', '光色', [1,1,1,1], kind='color'), param('affect_alpha', '扩展透明度', 1), edge()]
RAYS = r'''
let c = sample_input(p); if $strength$.x == 0.0 { return c; }
var sum = vec4(0.0); var total = 0.0;
for(var i:i32=0;i<64;i=i+1) {
    let t = f32(i)/63.0; let weight = exp(-t*$decay$.x);
    let q = mix(p, $center$.xy, t*$length$.x);
    sum += bright(sample_input(q), $threshold$.x)*weight; total += weight;
}
return combine_light(sample_source(p), sum/max(total, 0.000001), $strength$.x, $color$.rgb, $affect_alpha$.x);
'''
add('rays', '放射光束', 'Rays', 'S_Rays', RAY_PARAMS, RAYS,
    differences=['64点径向积分；输出限于当前图层边界。'])
EDGE_RAYS = RAYS.replace('var sum = vec4(0.0);',
    'if fx.clock.z < 0.5 { return edge_light(p, $threshold$.x); }\nvar sum = vec4(0.0);')
EDGE_RAYS = EDGE_RAYS.replace('bright(sample_input(q), $threshold$.x)', 'sample_input(q)')
add('edge_rays', '边缘光束', 'Edge Rays', 'S_EdgeRays', RAY_PARAMS, EDGE_RAYS,
    passes=2, differences=['边缘提取后64点径向积分，输出限于当前图层边界。'])

STREAK_PARAMS = LIGHT_PARAMS + [param('angle', '角度', 0, -360, 360, 'deg')]
STREAK = r'''
let c = sample_input(p); if $strength$.x == 0.0 { return c; }
let angle = radians($angle$.x); let direction = vec2(cos(angle), sin(angle));
var sum = vec4(0.0); var total = 0.0;
for(var i:i32=-32;i<=32;i=i+1) {
    let t = f32(i)/32.0; let weight = exp(-abs(t)*3.0);
    sum += bright(sample_input(p+direction*t*$radius$.x), $threshold$.x)*weight; total += weight;
}
return combine_light(sample_source(p), sum/total, $strength$.x, $color$.rgb, $affect_alpha$.x);
'''
add('streaks', '高光光条', 'Streaks', 'S_Streaks', STREAK_PARAMS, STREAK, pad=padding('radius'),
    differences=['单方向65点指数光条；长半径采用固定采样数。'])
GLINT = STREAK.replace('var sum = vec4(0.0); var total = 0.0;',
    'var sum = vec4(0.0); var total = 0.0;\nfor(var ray:i32=0;ray<4;ray=ray+1) {\nlet a = angle+f32(ray)*0.78539816339; let direction = vec2(cos(a),sin(a));')
GLINT = GLINT.replace('for(var i:i32=-32;i<=32;i=i+1)', 'for(var i:i32=-24;i<=24;i=i+1)').replace('f32(i)/32.0', 'f32(i)/24.0')
GLINT = GLINT.replace('return combine_light', '}\nreturn combine_light')
add('glint', '高光星芒', 'Glint', 'S_Glint', STREAK_PARAMS, GLINT, pad=padding('radius'),
    differences=['4条等权轴形成8向星芒，共196次高光采样；无独立方向颜色。'])

add('light_leak', '镜头漏光', 'Light Leak', 'S_LightLeak',
    [param('amount', '强度', .5), center(), param('radius', '范围', 120, 1, 2048, 'px'),
     param('color', '光色', [1,.3,.05,1], kind='color'), param('speed', '移动速度', .25, 0, 10, 'Hz')], r'''
let c = sample_input(p); if $amount$.x == 0.0 || c.a == 0.0 { return c; }
let time = fx.clock.x*$speed$.x;
let position = $center$.xy + vec2(smooth_noise(time, 11u), smooth_noise(time, 29u))*$radius$.x*.6;
let q = (p-position)/$radius$.x;
let light = exp(-dot(q,q)*2.0)*(0.7+0.3*smooth_noise(time, 41u))*$amount$.x*$color$.rgb;
return vec4(1.0-(1.0-c.rgb)*(1.0-clamp(light,vec3(0.0),vec3(1.0))),c.a);
''', alpha='straight', space='srgb', differences=['种子驱动移动椭圆光斑，保留源Alpha；不模拟物理镜头或遮光片。'])

add('warp_chroma', '色差分离', 'Chromatic Warp', 'S_WarpChroma',
    [param('amount', '色差距离', 8, -256, 256, 'px'), center(),
     param('expand', '扩展边界', 1, kind='bool'), edge()], r'''
if $amount$.x == 0.0 { return sample_input(p); }
let offset = p-$center$.xy; let direction = offset/max(length(offset), 0.000001);
let r = sample_input(p-direction*$amount$.x); let g = sample_input(p); let b = sample_input(p+direction*$amount$.x);
return vec4(r.r,g.g,b.b,max(r.a,max(g.a,b.a)));
''', category='扭曲', pad=padding('amount'), differences=['红/蓝沿中心径向反向平移，绿色不动；Alpha取三次采样并集。'])

KALEIDO_PARAMS = [center(), param('segments', '分段数', 6, 2, 32, 'count'),
    param('angle', '旋转', 0, -360, 360, 'deg'), param('mix', '混合', 1), edge(3)]
KALEIDO = r'''
let c = sample_input(p); if $mix$.x == 0.0 { return c; }
let offset = p-$center$.xy; let width = 6.28318530718/round($segments$.x);
let angle = atan2(offset.y, offset.x)-radians($angle$.x);
let sector = (angle/width-floor(angle/width))*width;
let folded = min(sector,width-sector)+radians($angle$.x);
let radius = length(offset);
let q = $center$.xy+vec2(cos(folded),sin(folded))*radius;
return mix(c,sample_input(q),$mix$.x);
'''
add('kaleido', '镜面万花筒', 'Kaleidoscope', 'S_Kaleido', KALEIDO_PARAMS, KALEIDO,
    category='扭曲', differences=['等角扇区镜像，无外部图层或透视控制。'])
add('kaleido_polar', '极坐标万花筒', 'Polar Kaleidoscope', 'S_KaleidoPolar',
    KALEIDO_PARAMS+[param('ring_size', '环宽', 64, 1, 2048, 'px')],
    KALEIDO.replace('let radius = length(offset);', 'let radius = fract(length(offset)/$ring_size$.x)*$ring_size$.x;'),
    category='扭曲', differences=['角向镜像叠加径向重复，未实现Sapphire全部极坐标映射控制。'])

add('shake', '镜头抖动', 'Shake', 'S_Shake',
    [param('amount', '强度', 1), param('translation', '位移幅度', [12,12], 0, 512, 'px', 'vec2'),
     param('rotation', '旋转幅度', 1, 0, 30, 'deg'), param('zoom', '缩放幅度', .02, 0, .5),
     param('frequency', '频率', 6, 0, 60, 'Hz'), center(), edge(3)], r'''
let c = sample_input(p); if $amount$.x == 0.0 { return c; }
let time = fx.clock.x*$frequency$.x;
let shift = vec2(smooth_noise(time,13u),smooth_noise(time,37u))*$translation$.xy*$amount$.x;
let angle = radians(smooth_noise(time,59u)*$rotation$.x*$amount$.x);
let zoom = 1.0+smooth_noise(time,71u)*$zoom$.x*$amount$.x;
return sample_input($center$.xy+turn(p-$center$.xy-shift,-angle)/zoom);
''', category='运动', differences=['平滑整数哈希噪声驱动位移、旋转、缩放；固定画幅，默认镜像边缘，无时间素材取帧。'])

add('transform_blur', '变换运动模糊', 'Transform Motion Blur', 'S_BlurMoCurves',
    [center(), param('shift', '位移', [0,0], -8192, 8192, 'px', 'vec2'),
     param('rotation', '旋转', 0, -360, 360, 'deg'), param('zoom', '缩放', 1, .1, 10),
     param('translation_blur', '曝光位移', [16,0], -2048, 2048, 'px', 'vec2'),
     param('rotation_blur', '曝光旋转', 0, -180, 180, 'deg'), param('zoom_blur', '曝光缩放', 0, 0, 1), edge(3)], r'''
var sum = vec4(0.0);
for(var i:i32=0;i<32;i=i+1) {
    let t = f32(i)/31.0-0.5;
    let shift = $shift$.xy+$translation_blur$.xy*t;
    let angle = radians($rotation$.x+$rotation_blur$.x*t);
    let zoom = max(.05,$zoom$.x*(1.0+$zoom_blur$.x*t));
    sum += sample_input($center$.xy+turn(p-$center$.xy-shift,-angle)/zoom);
}
return sum/32.0;
''', category='运动', differences=['32点显式曝光路径积分，曝光位移/旋转/缩放由用户指定；不会自动读取动画曲线导数，不取相邻素材帧。'])

add('grain', '胶片颗粒', 'Film Grain', 'S_Grain',
    [param('amount', '颗粒强度', .08, 0, .5), param('size', '颗粒尺寸', 1, .25, 16, 'px'),
     param('monochrome', '单色', 1, kind='bool'), param('rate', '更新频率', 24, 0, 60, 'Hz')], r'''
let c = sample_input(p); if $amount$.x == 0.0 || c.a == 0.0 { return c; }
let cell = vec2<i32>(floor(p/$size$.x)); let epoch = i32(floor(fx.clock.x*$rate$.x));
let mono = noise(cell,epoch,11u)*2.0-1.0;
let colored = vec3(mono,noise(cell,epoch,29u)*2.0-1.0,noise(cell,epoch,41u)*2.0-1.0);
let grain = select(colored,vec3(mono),$monochrome$.x>.5)*$amount$.x;
return vec4(clamp(c.rgb+grain,vec3(0.0),vec3(1.0)),c.a);
''', category='风格化', alpha='straight', space='srgb', differences=['分块均匀整数哈希颗粒，无胶片库存响应、相关颗粒模型。'])

add('scan_lines', '扫描线', 'Scan Lines', 'S_ScanLines',
    [param('amount', '强度', .35), param('spacing', '线距', 4, 1, 128, 'px'),
     param('speed', '移动速度', 0, -1024, 1024, 'px/s'),
     param('direction', '方向', 0, 0, 1, kind='enum', options=['横线','竖线'])], r'''
let c = sample_input(p); let coordinate = select(p.y,p.x,$direction$.x>.5)+fx.clock.x*$speed$.x;
let stripe = .5+.5*cos(coordinate/$spacing$.x*6.28318530718);
return vec4(c.rgb*(1.0-stripe*$amount$.x),c.a);
''', category='风格化', alpha='straight', space='srgb', differences=['余弦明暗扫描线，无模拟电视色彩或交错场处理。'])

add('film_damage', '旧胶片损伤', 'Film Damage', 'S_FilmDamage',
    [param('amount', '强度', 1), param('grain', '颗粒', .04, 0, .5),
     param('scratches', '划痕', .25), param('dust', '灰尘', .2),
     param('flicker', '闪烁', .12), param('shake', '跳片位移', 1, 0, 64, 'px'),
     param('rate', '更新频率', 12, 0, 60, 'Hz'), edge(3)], r'''
let original = sample_input(p); if $amount$.x == 0.0 { return original; }
let time = fx.clock.x*$rate$.x; let epoch = i32(floor(time));
let offset = vec2(smooth_noise(time,11u),smooth_noise(time,29u))*$shake$.x*$amount$.x;
let c = sample_input(p+offset); if c.a == 0.0 { return c; }
let grain = (noise(vec2<i32>(floor(p)),epoch,41u)*2.0-1.0)*$grain$.x;
let cell = vec2<i32>(floor(p/12.0));
let dust_center = vec2(noise(cell,epoch,53u),noise(cell,epoch,67u))*.8+.1;
let dust = select(0.0,1.0-smoothstep(.05,.12,length(fract(p/12.0)-dust_center)),noise(cell,epoch,79u)<$dust$.x*.2);
let column = i32(floor(p.x/8.0));
let scratch = select(0.0,1.0-smoothstep(.025,.09,abs(fract(p.x/8.0)-noise(vec2<i32>(column,0),epoch/6,97u))),
    noise(vec2<i32>(column,0),epoch/6,101u)<$scratches$.x*.15);
let flicker = 1.0+smooth_noise(time,113u)*$flicker$.x*$amount$.x;
let rgb = (c.rgb+grain*$amount$.x)*flicker*(1.0-dust*$amount$.x*.7)+scratch*$amount$.x*.35;
return vec4(clamp(rgb,vec3(0.0),vec3(1.0)),c.a);
''', category='风格化', alpha='straight', space='srgb', differences=['颗粒、点状灰尘、竖向划痕、闪烁与跳片组合；未实现毛发、污渍、自动失焦。'])

add('digital_damage', '数字故障', 'Digital Damage', 'S_DigitalDamage',
    [param('amount', '强度', 1), param('density', '故障概率', .35),
     param('displacement', '错行位移', 32, 0, 1024, 'px'),
     param('block_height', '条带高度', 8, 1, 256, 'px'), param('chroma', '色差', 4, 0, 128, 'px'),
     param('rate', '更新频率', 12, 0, 60, 'Hz'), edge(3)], r'''
let c = sample_input(p); if $amount$.x == 0.0 { return c; }
let epoch = i32(floor(fx.clock.x*$rate$.x)); let band = vec2<i32>(i32(floor(p.y/$block_height$.x)),0);
let event_strength = select(0.0,$amount$.x,noise(band,epoch,13u)<$density$.x);
let offset = (noise(band,epoch,29u)*2.0-1.0)*$displacement$.x*event_strength;
let q = p+vec2(offset,0.0); let g = sample_input(q);
let r = sample_input(q+vec2($chroma$.x*event_strength,0.0)); let b = sample_input(q-vec2($chroma$.x*event_strength,0.0));
let color = vec3(r.r,g.g,b.b)*mix(1.0,.7+noise(band,epoch,41u)*.6,event_strength);
return vec4(select(vec3(0.0),clamp(color,vec3(0.0),vec3(1.0)),g.a>0.0),g.a);
''', category='风格化', alpha='straight', space='srgb', differences=['确定性条带错行、色差与亮度闪断；Alpha沿绿色采样，不提供数据MOSH或跨帧损坏。'])


def main():
    LIB.mkdir(parents=True, exist_ok=True)
    (LIB/'shaders').mkdir(exist_ok=True)
    old = json.loads((LIB/'manifest.json').read_text(encoding='utf-8'))
    definitions = [e for e in old['effects'] if e['compatibility_profile']=='ae2021-srgb8-v1']
    assert len(definitions) == 20, 'The original 20 core effects must be retained'
    for definition, shader, reference in EFFECTS:
        definitions.append(definition)
        shader = '\n'.join(line.rstrip() for line in shader.lstrip().splitlines())+'\n'
        (LIB/definition['passes'][0]['shader']).write_text(shader, encoding='utf-8', newline='\n')
    manifest = dict(format_version=1, sdk_version=1, id=old['id'], version='1.1.0',
        name='Motion Studio 核心效果', author='motion-studio contributors', license='MIT', effects=definitions)
    (LIB/'manifest.json').write_text(json.dumps(manifest, ensure_ascii=False, indent=2)+'\n', encoding='utf-8', newline='\n')
    print(f'Generated {len(definitions)} original effects; pack with effect_tool')


if __name__ == '__main__':
    main()
