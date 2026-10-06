"""Build core 1.3 from immutable 1.2 bytes and captured AE 18 metadata.

No reference images, AE installation, or ignored directory is needed to rebuild.
Run effect_tool pack after this generator. Historical generators retain their versions.
"""
import json
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LIB = ROOT / 'crates/aem-effects/library'
CAPTURE = json.loads((ROOT/'tools/effect_metadata/ae18-tiling-ranges.json').read_text(encoding='utf-8'))
META = {e['id']: e for e in CAPTURE['effects']}
EFFECTS, AUDIT = [], []


def param(effect, id, number, kind='float', options=None, units=None, relative=None, implemented=True):
    match = META[effect]['matchName']+'-'+str(number).zfill(4)
    raw = next(p for p in META[effect]['params'] if p['matchName'] == match)
    value = raw['defaultValue']
    default = (value if isinstance(value, list) else [value]) + [0]*4
    lo = raw['min'] if raw['min'] is not None else -1e6
    hi = raw['max'] if raw['max'] is not None else 1e6
    note = 'AE 原生有效范围。'
    if raw['min'] is None or raw['max'] is None:
        note = 'AE API 无硬边界；宿主有限值范围 ±1000000，不代表 AE 原生限制。'
    if kind == 'color':
        lo, hi = 0, 1
        note = '8 bpc sRGB 宿主归一化 RGBA；AE API 的 HDR 数值范围见原始采集。'
    if not implemented:
        note += ' 首期仅允许默认值，界面禁用此控件。'
    p = dict(id=id,name=raw['name'],kind=kind,default=default[:4],min=lo,max=hi,
             step=1 if kind in ('bool','enum') else .01,units=units if units is not None else raw['units'],
             animatable=raw['animatable'],center_default=False,implemented=implemented,
             options=options or [],reference_match_name=match)
    if relative is not None:
        p['relative_default'] = relative
    AUDIT.append(dict(effect=effect,parameter=id,ae=raw,host_min=lo,host_max=hi,note=note))
    return p


def c(v): return dict(op='constant',value=v)
def p(id): return dict(op='parameter',id=id)
def add(a,b): return dict(op='add',a=a,b=b)
def mul(a,b): return dict(op='multiply',a=a,b=b)
def maximum(a,b): return dict(op='max',a=a,b=b)
def size(axis): return dict(op='input_size',component=axis)
def origin(axis): return dict(op='input_origin',component=axis)


def effect(id, english, name, category, params, body, differences, bounds=None, padding=None, passes=1):
    params.append(dict(id='effect_opacity',name='效果不透明度',kind='float',default=[100,0,0,0],
                       min=0,max=100,step=.01,units='%',animatable=True,implemented=True,options=[]))
    for index, definition in enumerate(params):
        body = body.replace('$'+definition['id']+'$',f'fx.params[{index}]')
    definition = dict(id=id,english_name=english,name=name,category=category,params=params,
                      passes=[dict(shader=f'shaders/{id}.wgsl',entry='main_fx') for _ in range(passes)],resources=[],
                      padding=padding or c(0),edge_mode='transparent',edge_param=None,working_space='srgb',alpha_mode='straight',
                      compatibility='approximate',compatibility_profile='ae18-tiling-srgb8-v1',
                      reference_match_name=META[id]['matchName'],reference_version='18.0.1',known_differences=differences,
                      required_capabilities=['single_frame','color_profile'] + (['rect_bounds'] if bounds else []) + (['multipass'] if passes > 1 else []))
    if bounds:
        definition['output_bounds'] = bounds
    shader = 'fn main_fx(p: vec2<f32>) -> vec4<f32> {\n'+body.strip()+'\n}\n'
    EFFECTS.append((definition,shader))


id = 'motion_tile'
dimensions = [mul(size(i),mul(p('output_width' if i == 0 else 'output_height'),c(.01))) for i in range(2)]
rect = dict(x=add(origin(0),mul(add(size(0),mul(dimensions[0],c(-1))),c(.5))),
            y=add(origin(1),mul(add(size(1),mul(dimensions[1],c(-1))),c(.5))),
            width=maximum(c(1),dimensions[0]),height=maximum(c(1),dimensions[1]))
effect(id,'Motion Tile','动态拼贴','风格化',[
    param(id,'center',1,'vec2',units='px',relative=[.5,.5]),
    param(id,'tile_width',2,units='%'),param(id,'tile_height',3,units='%'),
    param(id,'output_width',4,units='%'),param(id,'output_height',5,units='%'),
    param(id,'mirror',6,'bool'),param(id,'phase',7,units='deg'),param(id,'horizontal_phase',8,'bool')],r'''
if $output_width$.x == 0.0 || $output_height$.x == 0.0 { return vec4(0.0); }
let tile = max(vec2(1.0),fx.input_region.zw * vec2($tile_width$.x,$tile_height$.x) * .01);
var cell = (p - $center$.xy) / tile + .5;
let phase = $phase$.x / 360.0;
if $horizontal_phase$.x > .5 { cell.x -= (floor(cell.y) - 2.0*floor(floor(cell.y)*.5))*phase; }
else { cell.y -= (floor(cell.x) - 2.0*floor(floor(cell.x)*.5))*phase; }
var uv = fract(cell);
if $mirror$.x > .5 {
    let parity = floor(cell) - 2.0*floor(floor(cell)*.5);
    uv = select(uv,1.0-uv,parity > vec2(.5));
}
let q = fx.input_region.xy + uv * fx.input_region.zw;
if $tile_width$.x == 0.0 || $tile_height$.x == 0.0 {
    var sum=vec4(0.0);
    for(var i=0;i<64;i=i+1) {
        var probe=q;
        if $tile_width$.x == 0.0 && $tile_height$.x == 0.0 {
            probe=fx.input_region.xy+(vec2(f32(i%8),f32(i/8))+.5)/8.0*fx.input_region.zw;
        } else {
            if $tile_width$.x == 0.0 { probe.x=fx.input_region.x+(f32(i)+.5)/64.0*fx.input_region.z; }
            if $tile_height$.x == 0.0 { probe.y=fx.input_region.y+(f32(i)+.5)/64.0*fx.input_region.w; }
        }
        let v=sample_input(probe);sum+=vec4(v.rgb*v.a,v.a);
    }
    let avg=sum/64.0;return vec4(select(vec3(0.0),avg.rgb/max(avg.a,.000001),avg.a > .000001),avg.a);
}
return sample_input(q);
''', ['运动模糊关闭；亚像素拼贴边缘的抗锯齿可能与 AE 不同。',
       '以当前效果输入矩形为拼贴素材；前序效果外扩后采用该输入矩形。',
       '拼贴宽/高为零时用 64 样本平均到单像素列/行；输出宽/高为零时透明，内部保留至少 1 像素矩形。'], bounds=rect)

id = 'optics_compensation'
effect(id,'Optics Compensation','光学补偿','扭曲',[
    param(id,'fov',1,units='deg'),param(id,'reverse',2,'bool'),
    param(id,'orientation',3,'enum',options=['水平','垂直','对角线']),
    param(id,'center',4,'vec2',units='px',relative=[.5,.5]),
    param(id,'optimal_pixels',5,'bool',implemented=False),
    param(id,'resize',6,'enum',options=['关闭','最大 2 倍','最大 4 倍','无限制'],implemented=False)],r'''
if $fov$.x == 0.0 { return sample_input(p); }
let extent = select(select(fx.size.x,fx.size.y,$orientation$.x == 2.0),length(fx.size.xy),$orientation$.x == 3.0)*.5;
let angle = radians($fov$.x*.5);
let focal = extent / max(tan(angle),.000001);
let d = p-$center$.xy;let r=length(d);
if r < .000001 { return sample_input(p); }
let n=r/focal;var factor=0.0;
if $reverse$.x > .5 {
    factor=inverseSqrt(1.0+n*n);
} else {
    if n >= 1.0 {return vec4(0.0);}
    factor=inverseSqrt(1.0-n*n);
}
return sample_input($center$.xy+d*factor);
''', ['独立球面/平面径向映射；亚像素边缘与 AE 采样核存在差异。',
       '最佳像素、调整大小仅支持默认值；不静默接受外扩选项。'])

id = 'spherize'
effect(id,'Spherize','球面化','扭曲',[
    param(id,'radius',1,units='px'),param(id,'center',2,'vec2',units='px',relative=[.5,.5])],r'''
let radius=$radius$.x;let d=p-$center$.xy;let distance=length(d);
if radius == 0.0 || distance >= radius || distance < .000001 { return sample_input(p); }
let mapped = asin(clamp(distance/radius,0.0,1.0))*radius*.6366197724;
return sample_input($center$.xy+d*mapped/distance);
''', ['球面径向投影，双线性采样；AE 曲率及边界抗锯齿需更多参考用例。'])

id = 'cc_lens'
effect(id,'CC Lens','CC 镜头','扭曲',[
    param(id,'center',1,'vec2',units='px',relative=[.5,.5]),param(id,'size',2,units='%'),
    param(id,'convergence',3,units='%')],r'''
let radius = length(fx.size.xy)*$size$.x*.005;
let d=p-$center$.xy;let r=length(d);
if radius == 0.0 || r >= radius { return vec4(0.0); }
let n=r/radius;
let k=$convergence$.x*.01;
let factor=1.0-k*n*n;
let pixel=sample_input($center$.xy+d*factor);
let coverage=clamp((radius-r)*fx.output_mode.w+.5,0.0,1.0);
return vec4(pixel.rgb,pixel.a*coverage);
''', ['独立圆形镜头映射；圆外透明，未采用 Cycore 私有采样模型。'])

id = 'cc_radial_fast_blur'
effect(id,'CC Radial Fast Blur','CC 径向快速模糊','模糊与锐化',[
    param(id,'center',1,'vec2',units='px',relative=[.5,.5]),param(id,'amount',2,units='%'),
    param(id,'zoom',3,'enum',options=['标准','最亮','最暗'])],r'''
let c=sample_input(p);if $amount$.x == 0.0 { return c; }
var sum=vec4(0.0);var peak=vec4(0.0);var trough=vec4(1.0);
for(var i=0;i<64;i=i+1) {
    let q=$center$.xy+(p-$center$.xy)*(1.0-$amount$.x*.01*f32(i)/63.0);
    let v=sample_input(q);let weighted=vec4(v.rgb*v.a,v.a);
    sum+=weighted;peak=max(peak,weighted);trough=min(trough,weighted);
}
var result=sum/64.0;
if $zoom$.x == 2.0 {result=peak;} if $zoom$.x == 3.0 {result=trough;}
return vec4(select(vec3(0.0),result.rgb/max(result.a,.000001),result.a > .000001),result.a);
''', ['64 次径向取样；最亮/最暗为预乘通道极值，Cycore 内部核及权重不同。'])

id = 'simple_choker'
effect(id,'Simple Choker','简单阻塞工具','遮罩',[
    param(id,'view',1,'enum',options=['最终输出','遮罩']),param(id,'choke',2,units='px')],r'''
let c=sample_input(p);let source=sample_source(p);let amount=$choke$.x;
var inner=c;var outer=c;
let radius=abs(amount);let base=floor(radius);let axis=select(vec2(1.0,0.0),vec2(0.0,1.0),fx.clock.z > .5);
for(var i=-100;i<=100;i=i+1) {
    if abs(f32(i)) <= ceil(radius) {
        let v=sample_input(p+axis*f32(i));
        if select(v.a < outer.a,v.a > outer.a,amount < 0.0) {outer=v;}
        if abs(f32(i)) <= base && select(v.a < inner.a,v.a > inner.a,amount < 0.0) {inner=v;}
    }
}
let matte=mix(inner,outer,fract(radius));
if fx.clock.z < .5 {return matte;}
if $view$.x == 2.0 {return vec4(vec3(matte.a),1.0);}
return vec4(select(matte.rgb,source.rgb,source.a > 0.0),matte.a);
''', ['可分离方形 Alpha 收缩/扩张，分数像素在相邻核间插值；与 AE 亚像素形态核存在差异。',
       '负值外扩边界和透明区颜色由极值样本传播，采用两 pass。'],
       padding=maximum(c(0),mul(p('choke'),c(-1))),passes=2)

id = 'solid_composite'
effect(id,'Solid Composite','固态层合成','通道',[
    param(id,'source_opacity',1,units='%'),param(id,'color',2,'color'),param(id,'opacity',3,units='%'),
    param(id,'blend_mode',4,'enum',options=['正常']+[f'模式 {i}（未支持）' for i in range(2,22)],implemented=False)],r'''
let c=sample_input(p);let a=c.a*$source_opacity$.x*.01;let b=$opacity$.x*.01*$color$.a;
let alpha=a+b*(1.0-a);
let rgb=c.rgb*a+$color$.rgb*b*(1.0-a);
return vec4(select(vec3(0.0),rgb/max(alpha,.000001),alpha > .000001),alpha);
''', ['正常模式：源图像叠加到颜色背景上；其他 20 种原生混合模式仅允许默认值。',
       '颜色使用宿主 RGBA，颜色 Alpha 与背景不透明度相乘。'])


def main():
    with zipfile.ZipFile(LIB/'legacy/core-effects-1.2.0.msfx') as archive:
        manifest = json.loads(archive.read('manifest.json'))
        assert len(manifest['effects']) == 52
        for path in archive.namelist():
            if path != 'manifest.json':
                (LIB/path).write_bytes(archive.read(path))
    manifest['version'], manifest['sdk_version'] = '1.3.0', 3
    for definition, shader in EFFECTS:
        manifest['effects'].append(definition)
        (LIB/definition['passes'][0]['shader']).write_text(shader,encoding='utf-8',newline='\n')
    (LIB/'manifest.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n',encoding='utf-8',newline='\n')
    audit = dict(ae_version=CAPTURE['version'],profile=CAPTURE['profile'],parameters=AUDIT)
    (ROOT/'docs/effects/tiling-range-audit.json').write_text(json.dumps(audit,ensure_ascii=False,indent=2)+'\n',encoding='utf-8',newline='\n')
    print(f'Core {manifest["version"]}: {len(manifest["effects"])} effects; {len(AUDIT)} captured controls')


if __name__ == '__main__':
    main()
