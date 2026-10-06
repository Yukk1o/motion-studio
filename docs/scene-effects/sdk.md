# `.msfx` SDK 2

ZIP 的 format_version 仍为 1；sdk_version 可为 1 或 2。SDK 1 图像效果继续采用 `main_fx(pixel_position)`，已发布包无需重打包。SDK 2 新增 renderer、blend、scene 和 editor 描述。

## 编辑器声明

```json
{
  "editor": {
    "id": "example.lens-editor",
    "protocol": 1,
    "title": "镜头编辑器",
    "entry": "ui/editor.html",
    "files": ["ui/editor.html", "ui/editor.js", "ui/editor.css"]
  }
}
```

支持 ui/ 内的 HTML、JS、CSS、JSON 和 PNG。文件最多 32 个，每文件最多 256 KiB；文本必须是无 NUL 的 UTF-8；PNG 在解码前检查大小，并计入包资源预算。沿用 ZIP 路径、重复名称、软链接、包总大小及未引用文件校验。资源不能通过包清单声明网络权限。

模板见 `crates/aem-effects/scene-library/ui/`。图像效果也可以提供专用编辑器，不必采用场景生成器。

## 场景生成器

```json
{
  "renderer": "particles",
  "blend": "additive",
  "working_space": "linear",
  "alpha_mode": "premultiplied",
  "scene": {"occlusion": false, "elements": []},
  "required_capabilities": ["scene_projection", "sprite_instances", "plugin_editor"],
  "passes": [{"shader": "shaders/sprite.wgsl", "entry": "main_sprite"}]
}
```

renderer 取 image（默认）、particles 或 lens_flare；blend 取 alpha（默认）或 additive。场景生成器使用一个精灵外观 pass，必须采用线性、预乘输出；宿主管理投影、实例、排序和 RGBA16F 累积目标。插件不能声明额外 GPU 绑定或自行运行 compute。

```wgsl
fn main_sprite(uv: vec2<f32>, color: vec4<f32>, style: vec4<f32>) -> vec4<f32> {
    let p = (uv - 0.5) * 2.0;
    let a = exp(-dot(p, p) * 6.0) * (1.0 - smoothstep(0.8, 1.0, length(p)));
    return vec4(color.rgb * a, color.a * a);
}
```

uv 是每个精灵的 0..1 局部坐标；color 是宿主提供的线性预乘颜色。style.x 为镜头形状编码：0 glow、1 halo、2 ghost、3 streak、4 star；style.y 为星芒数，style.z 为色差，镜头 style.w 为 1；粒子 style.w 为寿命进度 0..1，采用形状 0，可以结合 curve_lookup 实现自定义生命周期外观。保持 premultiplied RGB 的约定，可以使用 source / resource PNG、普通 fx 参数与时钟；WGSL 通过同一 Naga 编译生成 GLES 300 源码。

标准宿主参数的类型、范围和可动画性由包校验确认。可增加外观参数，但总数仍不超过 32。标准参数名称及默认值以 scene-library/manifest.json 为准。

| 粒子参数 | 单位 / 语义 |
|---|---|
| rate、lifetime | 粒子/秒、秒；rate×lifetime ≤ 20,000 |
| speed、spread | 像素/秒；speed 为局部向上速度，spread 为各向同性随机速度 |
| gravity | 局部 XYZ 加速度，像素/秒²；Y 正方向向下 |
| extent、shape | 像素；形状 0 点、1 盒、2 球；球形是可按三轴伸缩的体积发射 |
| size、end_size | 出生/结束时的完整精灵直径，像素 |
| color、end_color | sRGB RGBA 0..1；转换到线性后渲染 |
| fade | 淡入和淡出分别占寿命的比例 0..0.5 |
| prewarm | 从时间 0 即展示已出生的完整寿命窗口 |

出生速率、寿命、速度、随机速度、重力、发射范围、形状和预热首版不支持动画。尺寸、颜色及淡入淡出总控可动画，当前帧总控重新计算所有粒子的寿命外观；发射器图层变换可动画。Seed 为效果实例级 u32。

镜头总控 position 为合成坐标 XYZ 像素，intensity 为倍数，scale 为百分比。attenuation 开关距离衰减，reference_distance 为参考距离，occlusion_radius 为合成像素的九点源遮挡采样半径。scene.source_layer 为可选世界枢轴引用；未设置时使用 position。

镜头元素有稳定 id、shape、enabled、offset、size、color、intensity、rays、chromatic。offset=0 位于光源、1 位于画面中心、2 位于光源关于中心的对侧。一个效果最多 64 个元素；元素配置的编辑是一次原子操作。

## 打包与版本

```powershell
python tools/generate_scene_library.py
cargo run -p aem-effects --bin effect_tool -- check crates/aem-effects/scene-library/scene-effects.msfx
```

自定义包沿用 effect_tool pack。修改算法、UI 或其他包资源后都需要增加版本；同版本不同哈希继续拒绝安装。安装、版本并存、撤销、复制、冻结输出及卸载沿用现有插件体系。新的预装场景包不能被卸载；可以停用。

插件执行不加载 Adobe、Video Copilot 或其他厂商的 SDK、脚本、资源或私有格式。
