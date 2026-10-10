"""Build core 1.4 spatial contracts from the pinned 1.3 package; pack with effect_tool."""
import json
import math
from pathlib import Path
import zipfile

ROOT = Path(__file__).resolve().parents[1]
LIB = ROOT / 'crates/motion-effects/library'


def c(value): return dict(op='constant', value=value)
def p(id, component=0): return dict(op='parameter', id=id, component=component)
def size(component): return dict(op='input_size', component=component)
def origin(component): return dict(op='input_origin', component=component)
def binary(op, a, b): return dict(op=op, a=a, b=b)
def add(a, b): return binary('add', a, b)
def mul(a, b): return binary('multiply', a, b)
def minimum(a, b): return binary('min', a, b)
def maximum(a, b): return binary('max', a, b)
def absolute(value): return dict(op='abs', value=value)
def negate(value): return mul(c(-1), value)
def choose(condition, a, b): return dict(op='select', condition=condition, a=a, b=b)


def radius(center):
    # L1 radius bounds every source corner without squaring large coordinates.
    spans = []
    for axis in range(2):
        distance = add(origin(axis), negate(p(center, axis)))
        spans.append(maximum(absolute(distance), absolute(add(distance, size(axis)))))
    return add(*spans)


def padded(x, y):
    return dict(x=add(origin(0), negate(x)), y=add(origin(1), negate(y)),
                width=add(size(0), mul(c(2), x)), height=add(size(1), mul(c(2), y)))


def build():
    with zipfile.ZipFile(LIB / 'legacy/core-effects-1.3.0.msfx') as archive:
        manifest = json.loads(archive.read('manifest.json'))
        shaders = {name: archive.read(name).decode('utf-8') for name in archive.namelist()
                   if name.startswith('shaders/')}
    manifest['version'], manifest['sdk_version'] = '1.4.0', 4
    effects = {e['id']: e for e in manifest['effects']}
    amplitude = p('amount')
    scale = mul(p('zoom'), amplitude)
    angle = mul(mul(p('rotation'), amplitude), c(math.pi / 360))
    motion = mul(radius('center'), add(scale, mul(c(2), dict(op='sin', value=angle))))
    effects['shake']['output_bounds'] = padded(
        add(mul(p('translation', 0), amplitude), motion),
        add(mul(p('translation', 1), amplitude), motion))

    angle = minimum(c(180), add(absolute(p('rotation')), mul(c(.5), absolute(p('rotation_blur')))))
    scale = add(absolute(add(p('zoom'), c(-1))), mul(mul(p('zoom'), p('zoom_blur')), c(.5)))
    motion = mul(radius('center'), add(scale, mul(c(2), dict(op='sin', value=mul(angle, c(math.pi / 360))))))
    effects['transform_blur']['output_bounds'] = padded(*[
        add(add(absolute(p('shift', axis)), mul(c(.5), absolute(p('translation_blur', axis)))), motion)
        for axis in range(2)])

    # One angular turn, radius up to the furthest corner. The unwrapped tail is
    # below the original rectangle, so its origin must not be recentered.
    diagonal = binary('hypot', size(0), size(1))
    extent = mul(size(1), binary('divide', diagonal, minimum(size(0), size(1))))
    active = choose(add(p('p0002'), c(-1.5)), extent, size(1))
    effects['polar_coordinates']['output_bounds'] = dict(
        x=origin(0), y=origin(1), width=size(0), height=choose(p('p0001'), active, size(1)))
    for id in ['shake', 'transform_blur', 'polar_coordinates']:
        effect = effects[id]
        effect['padding'] = c(0)
        for capability in ['rect_bounds', 'spatial_bounds']:
            if capability not in effect['required_capabilities']:
                effect['required_capabilities'].append(capability)
    for id in ['shake', 'transform_blur']:
        effects[id]['edge_param'] = 'edge'
        effects[id]['known_differences'].append(
            '完整输入平面的位移、旋转与缩放外扩；边界按参数幅度保守计算，属性与锚点不变。边缘模式仅影响有限平面边缘的重采样。')
    effects['polar_coordinates']['known_differences'].append(
        '使用当前效果输入矩形；极坐标到直角坐标输出覆盖一周角度和输入对角半径，可延伸到原图层下方。插值仍为反向坐标近似，未经 AE 像素验收。')

    shaders['shaders/shake.wgsl'] = shaders['shaders/shake.wgsl'].replace(
        'return sample_input(fx.params[5].xy+turn(p-fx.params[5].xy-shift,-angle)/zoom);',
        'let q = fx.params[5].xy+turn(p-fx.params[5].xy-shift,-angle)/zoom;\n'
        'if any(q<fx.input_region.xy)||any(q>fx.input_region.xy+fx.input_region.zw) {return vec4(0.0);}\n'
        'return sample_input(q);')
    shaders['shaders/transform_blur.wgsl'] = shaders['shaders/transform_blur.wgsl'].replace(
        'sum += sample_input(fx.params[0].xy+turn(p-fx.params[0].xy-shift,-angle)/zoom);',
        'let q = fx.params[0].xy+turn(p-fx.params[0].xy-shift,-angle)/zoom;\n'
        '    if all(q>=fx.input_region.xy)&&all(q<=fx.input_region.xy+fx.input_region.zw) {sum += sample_input(q);}')
    polar = shaders['shaders/polar_coordinates.wgsl']
    begin = polar.index('fn main_fx')
    shaders['shaders/polar_coordinates.wgsl'] = polar[:begin] + '''fn main_fx(p:vec2<f32>)->vec4<f32>{
let uv=(p-fx.input_region.xy)/fx.input_region.zw;
let center=fx.input_region.xy+fx.input_region.zw*0.5;
let radius=max(min(fx.input_region.z,fx.input_region.w)*0.5,0.0001);
let v=(p-center)/radius;
var q=fx.input_region.xy+vec2((atan2(v.y,v.x)/6.28318530718+0.5)*fx.input_region.z,length(v)*fx.input_region.w);
if fx.params[2].x>1.5{let angle=(uv.x-0.5)*6.28318530718;q=center+vec2(cos(angle),sin(angle))*uv.y*radius;}
return sample_input(mix(p,q,fx.params[1].x));
}
'''
    # Preserve readable existing metadata; compact repeated expression trees so
    # the unchanged 256 KiB manifest limit still protects imported packages.
    text = json.dumps(manifest, ensure_ascii=False, indent=2)
    for id in ['shake', 'transform_blur', 'polar_coordinates']:
        block = json.dumps(effects[id]['output_bounds'], ensure_ascii=False, indent=2)
        indented = '\n'.join('      ' + line if i else line for i, line in enumerate(block.splitlines()))
        compact = json.dumps(effects[id]['output_bounds'], separators=(',', ':'))
        text = text.replace('"output_bounds": ' + indented, '"output_bounds": ' + compact)
    encoded = (text + '\n').encode('utf-8')
    assert len(encoded) <= 256 * 1024, len(encoded)
    (LIB / 'manifest.json').write_bytes(encoded)
    for path, source in shaders.items():
        (LIB / path).write_bytes(source.encode('utf-8'))
    print(f'Core 1.4.0: {len(manifest["effects"])} effects, {len(encoded)} manifest bytes')


if __name__ == '__main__':
    build()
