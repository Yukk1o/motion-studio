"""Build core 1.2 from immutable 1.1 bytes and checked-in AE parameter metadata.

Algorithms are original WGSL implementations, classified approximate. Captured
parameter ranges are not evidence of AE image equivalence. No local AE/reference
directory is required to rebuild. Run effect_tool pack after this generator.
"""
import json
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LIB = ROOT / 'crates/aem-effects/library'
CAPTURE = json.loads((ROOT / 'tools/effect_metadata/ae18-ranges.json').read_text(encoding='utf-8'))
META = {e['id']: e for e in CAPTURE['effects']}
EFFECTS = []
AUDIT = []


def param(effect, id, suffix, *, kind='float', options=None, units=None, bounds=None,
          relative=None, implemented=True):
    e = META[effect]
    raw = next(p for p in e['params'] if p['matchName'] == e['matchName'] + '-' + suffix)
    value = raw['defaultValue']
    values = value if isinstance(value, list) else [value]
    minimum = raw['min'] if raw['min'] is not None else -1000000
    maximum = raw['max'] if raw['max'] is not None else 1000000
    note = 'AE 18.0.1 API hard range'
    if raw['min'] is None or raw['max'] is None:
        note = 'AE has no bound; Motion Studio finite numeric limit ±1,000,000'
    if kind == 'color':
        minimum, maximum = 0, 1
        note = 'Motion Studio normalized RGBA8 contract; AE API exposes a wider HDR range'
    if bounds:
        minimum, maximum = bounds
        note = 'Motion Studio supported parameter subset; raw AE range retained in this audit'
    p = dict(id=id, name=raw['name'], kind=kind, default=values + [0] * (4-len(values)),
             min=minimum, max=maximum, step=1 if kind in ('bool','enum') else .01,
             units=raw['units'] if units is None else units, animatable=raw['animatable'],
             center_default=False, implemented=implemented, options=options or [],
             reference_match_name=raw['matchName'])
    if relative:
        p['relative_default'] = relative
    AUDIT.append(dict(effect=effect, param=id, ae=raw, host=p, range_source=note))
    return p


def add(id, english, name, category, params, body, *, differences=()):
    params += [dict(id='effect_opacity', name='效果不透明度', kind='float', default=[100,0,0,0],
                    min=0, max=100, step=.01, units='%', animatable=True,
                    center_default=False, implemented=True, options=[], reference_match_name='')]
    for index, p in enumerate(params):
        body = body.replace('$' + p['id'] + '$', f'fx.params[{index}]')
    assert '$' not in body
    included = {p['reference_match_name'] for p in params}
    omitted = [p['name'] for p in META[id]['params'] if p['matchName'] not in included]
    definition = dict(id=id, name=name, english_name=english, category=category, params=params,
        passes=[dict(shader=f'shaders/{id}.wgsl', entry='main_fx')], resources=[],
        padding=dict(op='constant',value=0), edge_mode='clamp', edge_param=None,
        working_space='srgb', alpha_mode='straight', compatibility='approximate',
        compatibility_profile='ae18-common-srgb8-v1', reference_match_name=META[id]['matchName'],
        reference_version='18.0.1', required_capabilities=['single_frame','color_profile'],
        known_differences=['独立 WGSL 算法；参数已实采核对，尚未通过完整 AE 图像对照验收。',
                          '8 bpc sRGB；RGB 截断至 0～1；调色/生成保留源 Alpha，扭曲搬移 Alpha，擦除效果乘以覆盖率。',
                          *differences] + (['尚未提供 AE 控件：' + '、'.join(omitted)] if omitted else []))
    EFFECTS.append((definition, COMMON + '\nfn main_fx(p: vec2<f32>) -> vec4<f32> {\n' + body + '\n}\n'))


COMMON = r'''
fn luma(c:vec3<f32>)->f32 { return dot(c,vec3(.299,.587,.114)); }
fn rotate(p:vec2<f32>,a:f32)->vec2<f32> { return vec2(cos(a)*p.x-sin(a)*p.y,sin(a)*p.x+cos(a)*p.y); }
fn finish(c:vec4<f32>,rgb:vec3<f32>)->vec4<f32> {
    return vec4(select(clamp(rgb,vec3(0.0),vec3(1.0)),vec3(0.0),c.a==0.0),c.a);
}
fn hash(cell:vec2<i32>,salt:u32)->f32 {
    var v=bitcast<u32>(cell.x)*0x9e3779b9u ^ bitcast<u32>(cell.y)*0x85ebca6bu ^ salt;
    v=(v^(v>>16u))*0x7feb352du;v=(v^(v>>15u))*0x846ca68bu;v=v^(v>>16u);
    return f32(v>>8u)/16777216.0;
}
fn noise(q:vec2<f32>,phase:f32,seed:u32,kind:f32)->f32 {
    let cell=vec2<i32>(floor(q));var f=fract(q);
    if kind==1.0 {f=vec2(0.0);} else if kind>=3.0 {f=f*f*(3.0-2.0*f);}
    let t=sin(phase)*.5+.5;
    let a=mix(hash(cell,seed),hash(cell,seed^0x1234567u),t);
    let b=mix(hash(cell+vec2<i32>(1,0),seed),hash(cell+vec2<i32>(1,0),seed^0x1234567u),t);
    let c=mix(hash(cell+vec2<i32>(0,1),seed),hash(cell+vec2<i32>(0,1),seed^0x1234567u),t);
    let d=mix(hash(cell+vec2<i32>(1,1),seed),hash(cell+vec2<i32>(1,1),seed^0x1234567u),t);
    return mix(mix(a,b,f.x),mix(c,d,f.x),f.y);
}
fn cover(distance:f32,feather:f32)->f32 {
    if feather<=0.0 {return select(0.0,1.0,distance>=0.0);}
    return smoothstep(-feather*.5,feather*.5,distance);
}
'''

id = 'invert'
add(id,'Invert','反相','调色',[
    param(id,'channel','0001',kind='enum',bounds=(1,5),options=['RGB','红','绿','蓝','Alpha']),
    param(id,'blend','0002')],r'''
let c=sample_input(p);let channel=$channel$.x;var rgb=c.rgb;var alpha=c.a;
if channel==1.0 {rgb=1.0-rgb;} else if channel==2.0 {rgb.r=1.0-rgb.r;}
else if channel==3.0 {rgb.g=1.0-rgb.g;} else if channel==4.0 {rgb.b=1.0-rgb.b;}
else {alpha=1.0-alpha;}
return mix(vec4(rgb,alpha),c,$blend$.x/100.0);
''',differences=['仅提供 RGB/R/G/B/Alpha；宿主菜单 5 对应 AE 菜单 16（Alpha），不支持 HLS/YIQ。'])

id='black_white'
ps=[param(id,name,f'{i+1:04}') for i,name in enumerate(['red','yellow','green','cyan','blue','magenta'])]
ps += [param(id,'tint','0007',kind='bool'),param(id,'tint_color','0008',kind='color')]
add(id,'Black & White','黑白','调色',ps,r'''
let c=sample_input(p);let hi=max(c.r,max(c.g,c.b));let lo=min(c.r,min(c.g,c.b));let delta=hi-lo;
var hue=0.0;
if delta>0.000001 {
    if hi==c.r {hue=(c.g-c.b)/delta;} else if hi==c.g {hue=2.0+(c.b-c.r)/delta;} else {hue=4.0+(c.r-c.g)/delta;}
    hue=fract(hue/6.0)*6.0;
}
var weight=0.0;
if hue<1.0 {weight=mix($red$.x,$yellow$.x,hue);}
else if hue<2.0 {weight=mix($yellow$.x,$green$.x,hue-1.0);}
else if hue<3.0 {weight=mix($green$.x,$cyan$.x,hue-2.0);}
else if hue<4.0 {weight=mix($cyan$.x,$blue$.x,hue-3.0);}
else if hue<5.0 {weight=mix($blue$.x,$magenta$.x,hue-4.0);}
else {weight=mix($magenta$.x,$red$.x,hue-5.0);}
weight/=100.0;
let grey=clamp(lo+delta*weight,0.0,1.0);var rgb=vec3(grey);
if $tint$.x>0.5 {rgb*= $tint_color$.rgb/max(luma($tint_color$.rgb),.000001);}
return finish(c,rgb);
''',differences=['六色按色相线性混合灰度权重，着色以亮度归一化；与 AE 色彩选择性响应不同。'])

id='channel_mixer'
names=['red_red','red_green','red_blue','red_constant','green_red','green_green','green_blue','green_constant',
       'blue_red','blue_green','blue_blue','blue_constant']
ps=[param(id,name,f'{i+1:04}') for i,name in enumerate(names)]+[param(id,'monochrome','0013',kind='bool')]
add(id,'Channel Mixer','通道混合器','调色',ps,r'''
let c=sample_input(p);
var rgb=vec3(dot(c.rgb,vec3($red_red$.x,$red_green$.x,$red_blue$.x))+$red_constant$.x,
             dot(c.rgb,vec3($green_red$.x,$green_green$.x,$green_blue$.x))+$green_constant$.x,
             dot(c.rgb,vec3($blue_red$.x,$blue_green$.x,$blue_blue$.x))+$blue_constant$.x)/100.0;
if $monochrome$.x>0.5 {rgb=vec3(rgb.r);}
return finish(c,rgb);
''')

id='posterize'
add(id,'Posterize','色调分离','调色',[param(id,'levels','0001')],r'''
let c=sample_input(p);let levels=max(2.0,round($levels$.x));
return finish(c,round(c.rgb*(levels-1.0))/(levels-1.0));
''',differences=['级别采样时四舍五入为整数；量化采用最近色阶。'])

id='threshold'
add(id,'Threshold','阈值','调色',[param(id,'level','0001')],r'''
let c=sample_input(p);return finish(c,vec3(select(0.0,1.0,luma(c.rgb)>=$level$.x)));
''',differences=['身份固定为 ADBE Threshold2；使用 BT.601 亮度，不混用旧版 Threshold。'])

id='find_edges'
add(id,'Find Edges','查找边缘','风格化',[param(id,'invert','0001',kind='bool'),param(id,'blend','0002')],r'''
let c=sample_input(p);
let dx=sample_input(p+vec2(1.0,0.0)).rgb-sample_input(p-vec2(1.0,0.0)).rgb;
let dy=sample_input(p+vec2(0.0,1.0)).rgb-sample_input(p-vec2(0.0,1.0)).rgb;
let edge=clamp(sqrt(dx*dx+dy*dy),vec3(0.0),vec3(1.0));
return finish(c,mix(select(1.0-edge,edge,$invert$.x>0.5),c.rgb,$blend$.x));
''',differences=['中心差分逐通道边缘，与 AE 内核不同；此控件原始比例为 0～1。'])

for id in ['emboss','color_emboss']:
    ps=[param(id,'direction','0001',units='deg'),param(id,'relief','0002',units='px'),
        param(id,'contrast','0003',units='%'),param(id,'blend','0004',units='%')]
    base='vec3(.5)' if id=='emboss' else 'c.rgb'
    add(id,'Emboss' if id=='emboss' else 'Color Emboss','浮雕' if id=='emboss' else '彩色浮雕','风格化',ps,r'''
let c=sample_input(p);let angle=radians($direction$.x);let offset=vec2(cos(angle),sin(angle))*$relief$.x;
let delta=luma(sample_input(p+offset).rgb)-luma(sample_input(p-offset).rgb);
return finish(c,mix(BASE+delta*$contrast$.x/100.0,c.rgb,$blend$.x/100.0));
'''.replace('BASE',base),differences=['沿方向两点亮度差；边缘钳制，未复现 AE 精确浮雕内核。'])

id='mosaic'
add(id,'Mosaic','马赛克','风格化',[param(id,'horizontal','0001'),param(id,'vertical','0002'),param(id,'sharp','0003',kind='bool')],r'''
let c=sample_input(p);let blocks=max(vec2(1.0),round(vec2($horizontal$.x,$vertical$.x)));
let size=fx.size.xy/blocks;let origin=floor(p/size)*size;var rgb=vec3(0.0);
if $sharp$.x>0.5 {rgb=sample_input(origin+size*.5).rgb;}
else {
    for(var y:i32=0;y<4;y=y+1) {for(var x:i32=0;x<4;x=x+1) {
        rgb+=sample_input(origin+size*(vec2(f32(x),f32(y))+.5)/4.0).rgb;
    }}
    rgb/=16.0;
}
return finish(c,rgb);
''',differences=['普通模式每块固定 4×4 采样近似均值；锐化颜色取块中心；块数取整。'])

id='turbulent_displace'
ps=[param(id,'amount','0002',units='px'),param(id,'size','0003',units='px'),
    param(id,'offset','0004',kind='vec2',relative=[.5,.5]),param(id,'complexity','0005'),
    param(id,'evolution','0006',units='deg'),param(id,'seed','0010')]
add(id,'Turbulent Displace','湍流置换','扭曲',ps,r'''
let q=(p-$offset$.xy)/$size$.x;let seed=u32(round($seed$.x));let phase=radians($evolution$.x);
var pos=q;var amplitude=1.0;var total=0.0;var v=vec2(0.0);
for(var i:i32=0;i<10;i=i+1) {
    let weight=clamp($complexity$.x-f32(i),0.0,1.0)*amplitude;
    v+=vec2(noise(pos,phase,seed+u32(i)*29u,3.0),noise(pos,phase,seed+u32(i)*29u+101u,3.0))*weight;
    total+=weight;pos=fract(pos/4096.0)*8192.0;amplitude*=.5;
}
let shift=(v/max(total,.000001)*2.0-1.0)*$amount$.x;
return sample_input(p+shift);
''',differences=['仅湍流模式，1～10 层插值值噪声；边缘钳制、固定输出，不提供固定边缘和扩展图层。',
                '演化为周期性的双噪声插值，未复现 AE 非循环演化；随机植入取整。'])

id='gradient_ramp'
ps=[param(id,'start','0001',kind='vec2',relative=[.5,0]),param(id,'start_color','0002',kind='color'),
    param(id,'end','0003',kind='vec2',relative=[.5,1]),param(id,'end_color','0004',kind='color'),
    param(id,'shape','0005',kind='enum',options=['线性','径向']),param(id,'scatter','0006'),param(id,'blend','0007')]
add(id,'Gradient Ramp','渐变','生成',ps,r'''
let c=sample_input(p);let axis=$end$.xy-$start$.xy;
var t=dot(p-$start$.xy,axis)/max(dot(axis,axis),.000001);
if $shape$.x==2.0 {t=length(p-$start$.xy)/max(length(axis),.000001);}
t=clamp(t+(hash(vec2<i32>(floor(p)),bitcast<u32>(fx.clock.w))-.5)*$scatter$.x/255.0,0.0,1.0);
let rgb=mix($start_color$.rgb,$end_color$.rgb,t);
return finish(c,mix(rgb,c.rgb,$blend$.x));
''',differences=['保留源 Alpha，不在空像素生成；散射为固定像素哈希抖动。'])

id='fill'
add(id,'Fill','填充','生成',[param(id,'color','0002',kind='color'),param(id,'opacity','0005')],r'''
let c=sample_input(p);return finish(c,mix(c.rgb,$color$.rgb,$opacity$.x));
''',differences=['仅无蒙版填充颜色与不透明度；不提供填充蒙版、反转或蒙版羽化。'])

id='linear_wipe'
add(id,'Linear Wipe','线性擦除','过渡',[param(id,'completion','0001',units='%'),
    param(id,'angle','0002',units='deg'),param(id,'feather','0003',units='px')],r'''
let c=sample_input(p);let amount=$completion$.x/100.0;
if amount<=0.0 {return c;} if amount>=1.0 {return vec4(0.0);}
let a=radians($angle$.x);let direction=vec2(sin(a),-cos(a));
let extent=dot(abs(direction),fx.size.xy)*.5;
let projection=dot(p-fx.size.xy*.5,direction);
let coverage=cover(projection+extent-amount*extent*2.0,$feather$.x);
return vec4(c.rgb,c.a*coverage);
''',differences=['平面半空间擦除，羽化为 smoothstep；端点明确完全保留/完全透明。'])

id='radial_wipe'
add(id,'Radial Wipe','径向擦除','过渡',[param(id,'completion','0001',units='%'),param(id,'angle','0002',units='deg'),
    param(id,'center','0003',kind='vec2',relative=[.5,.5]),param(id,'direction','0004',kind='enum',options=['顺时针','逆时针','双向']),
    param(id,'feather','0005',units='px')],r'''
let c=sample_input(p);let amount=$completion$.x/100.0;
if amount<=0.0 {return c;} if amount>=1.0 {return vec4(0.0);}
let q=p-$center$.xy;var angle=fract((atan2(q.x,-q.y)-radians($angle$.x))/6.28318530718);
if $direction$.x==2.0 {angle=fract(-angle);} else if $direction$.x==3.0 {angle=min(angle,1.0-angle)*2.0;}
let distance=(angle-amount)*6.28318530718*max(length(q),.5);
let coverage=cover(distance,$feather$.x);
return vec4(c.rgb,c.a*coverage);
''',differences=['角向覆盖率与按半径换算的像素羽化；起始射线的接缝与 AE 不完全相同。'])

id='venetian_blinds'
add(id,'Venetian Blinds','百叶窗','过渡',[param(id,'completion','0001',units='%'),param(id,'direction','0002',units='deg'),
    param(id,'width','0003',units='px'),param(id,'feather','0004',units='px')],r'''
let c=sample_input(p);let amount=$completion$.x/100.0;
if amount<=0.0 {return c;} if amount>=1.0 {return vec4(0.0);}
let a=radians($direction$.x);let width=$width$.x;let phase=fract(dot(p,vec2(cos(a),sin(a)))/width);
let distance=(min(phase,1.0-phase)-amount*.5)*width;
return vec4(c.rgb,c.a*cover(distance,$feather$.x));
''',differences=['条带以图层原点为相位，双边 smoothstep 羽化；实际宽度按图层像素解释。'])

id='fractal_noise'
ps=[param(id,'noise_type','0002',kind='enum',options=['块','线性','柔和线性','样条']),param(id,'invert','0003',kind='bool'),
    param(id,'contrast','0004'),param(id,'brightness','0005'),param(id,'rotation','0008',units='deg'),
    param(id,'scale','0010',units='%'),param(id,'offset','0013',kind='vec2',relative=[.5,.5]),
    param(id,'complexity','0015'),param(id,'evolution','0023',units='deg'),param(id,'seed','0027'),param(id,'opacity','0029',units='%')]
add(id,'Fractal Noise','分形杂色','生成',ps,r'''
var q=rotate(p-$offset$.xy,radians($rotation$.x))/$scale$.x;
let phase=radians($evolution$.x);let seed=u32(round($seed$.x));
var amplitude=1.0;var total=0.0;var v=0.0;
for(var i:i32=0;i<20;i=i+1) {
    let weight=clamp($complexity$.x-f32(i),0.0,1.0)*amplitude;
    v+=noise(q,phase,seed+u32(i)*29u,$noise_type$.x)*weight;
    total+=weight;q=fract(q/4096.0)*4096.0/.56;amplitude*=.7;
}
var grey=clamp((v/max(total,.000001)-.5)*$contrast$.x/100.0+.5+$brightness$.x/100.0,0.0,1.0);
if $invert$.x>0.5 {grey=1.0-grey;}
let c=sample_input(p);return finish(c,mix(c.rgb,vec3(grey),$opacity$.x/100.0));
''',differences=['仅基本分形，固定子影响 70%、子缩放 56%；普通覆盖混合，保留源 Alpha。',
                '独立值噪声、最大20层，样条模式目前等同柔和线性；演化周期性且不自动依赖时间。'])


def main():
    with zipfile.ZipFile(LIB/'legacy/core-effects-1.1.0.msfx') as archive:
        manifest=json.loads(archive.read('manifest.json'))
    assert len(manifest['effects'])==36
    manifest['version']='1.2.0'
    for e in manifest['effects']:
        for p in e['params']:
            if p['kind']=='color':
                p['min'],p['max']=0,1
    for definition, shader in EFFECTS:
        manifest['effects'].append(definition)
        (LIB/definition['passes'][0]['shader']).write_text(shader.lstrip(),encoding='utf-8',newline='\n')
    (LIB/'manifest.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n',encoding='utf-8',newline='\n')
    audit=dict(ae_version=CAPTURE['version'],profile=CAPTURE['profile'],parameters=AUDIT)
    (ROOT/'docs/effects/common-range-audit.json').write_text(json.dumps(audit,ensure_ascii=False,indent=2)+'\n',encoding='utf-8',newline='\n')
    print(f'Core {manifest["version"]}: {len(manifest["effects"])} effects; {len(AUDIT)} captured controls')


if __name__=='__main__':
    main()
