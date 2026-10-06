# motion-studio Effect SDK 3（保留 SDK 1/2）

SDK 1图像效果契约保持支持。专用插件UI、实例化粒子和镜头生成器使用 [SDK 2扩展](../scene-effects/sdk.md)；当前JNI渲染计划为 [协议2](render-plan.md)，两者的版本号分别管理。

`.msfx` 是 ZIP，包含根目录 manifest.json、manifest 引用的 UTF-8 WGSL 和可选 PNG。一个包可声明多个效果。模板见 `sdk/effect-template`；描述类型的唯一实现来源是 `crates/aem-effects/src/schema.rs`。

```powershell
cargo run -p aem-effects --bin effect_tool -- pack sdk/effect-template artifacts/gain.msfx
cargo run -p aem-effects --bin effect_tool -- check artifacts/gain.msfx
cargo run -p aem-effects --bin effect_tool -- glsl artifacts/gain.msfx artifacts/gain-glsl
cargo run -p aem-effects --bin effect_tool -- builtin artifacts/core-effects.msfx
```

打包按路径排序并固定 ZIP 时间戳，包 SHA-256 包括原始完整 ZIP 字节。插件 ID/效果 ID/参数 ID 只允许 ASCII 字母、数字、点、下划线和连字符；版本使用 semver。更新算法、参数或资源必须增加插件版本。SDK 1 只支持静态着色器，不支持 JavaScript、原生代码或网络调用。

内置库直接嵌入版本控制中的library/core-effects.msfx固定字节，不在不同目标系统上重新压缩，保证桌面和Android解析同一SHA-256。修改内置manifest/源码后，先增加版本，再执行 `effect_tool pack crates/aem-effects/library crates/aem-effects/library/core-effects.msfx`，然后重新构建并运行对照。宿主检查嵌入包与manifest一致，陈旧的包会明确报错。

当前核心版本 1.3.0 包含 59 项；1.0.0、1.1.0、1.2.0 原包保留并一同预装。生成器不读取本地参考图，最新版本重建方式：

```powershell
py -3.14 tools/generate_tiling_library.py
cargo run -p aem-effects --bin effect_tool -- pack crates/aem-effects/library crates/aem-effects/library/core-effects.msfx
```

最新新增效果、参数范围与兼容限制见 [动态拼贴与图像工具](tiling-library.md)。打包只收入 manifest 引用的文件，legacy 目录不会嵌入新包。历史生成器保持各自版本。

## SDK 3 输出矩形

新增 `output_bounds={x,y,width,height}`，每个字段是一棵 BoundsExpr。可用 `input_size` 和 `input_origin`，component 只能为 0（x/width）或 1（y/height）；读取当前效果输入矩形，单位为未变换图层像素。原有 constant/parameter/add/multiply/max/abs/ceil 继续支持，深度上限 16，有限数值绝对值上限 32768。工作参数本身仍遵循各自 min/max。

`output_bounds` 要求 SDK 3、图像效果，和非零 padding 互斥；结果尺寸必须至少 1 px，可扩展也可裁切，区域位置允许负值。声明 `rect_bounds` 能力。包校验采用单位输入检查结构及默认参数，运行时在实际输入矩形上检查尺寸/预算。合法参数超出设备资源能力会明确报错。

```json
{"output_bounds": {
  "x": {"op":"input_origin","component":0},
  "y": {"op":"input_origin","component":1},
  "width": {"op":"multiply","a":{"op":"input_size","component":0},"b":{"op":"constant","value":2}},
  "height": {"op":"input_size","component":1}
}}
```

矩形效果的 main_fx 仍返回声明的工作色彩/Alpha；SDK 包装器在最后一个 pass 负责转换到宿主合成空间，可直接写合成槽位 0，减少外扩副本。中间 pass 保持工作空间。不得依赖输出纹理格式或自行重复转换。SDK 1/2 包和没有 output_bounds 的效果保持原执行路径，JNI 计划版本仍为 2、参数布局仍为 624 字节。

## Manifest

必需字段：format_version=1、sdk_version=1/2/3、id、version、name、author、license、effects。每个效果必需 id/name/english_name/category/params/passes；其它字段有默认值。一个效果最多 32 个参数、8 个 pass、4 张 PNG、1 个 Curve 参数；一个图层最多 16 个效果。

参数字段：id/name/kind/default（固定四数）/min/max/step/units/animatable/implemented/options；kind 为 float、vec2、vec3、color、bool、enum、curve。可选 center_default 对 vec2 使用图层中心，relative_default=[x,y] 使用未变换尺寸的比例；relative_default 优先。宿主 UI 要使用 id，不能把中文名称当键。SDK 中 params 的槽位是 manifest.params 数组顺序，工程 JSON 对象的键排序与槽位无关。

passes 是 `{shader:"shaders/main.wgsl",entry:"main_fx"}` 数组。resources 是最多四个 PNG 路径，依次映射 resource0…resource3。effect 可声明 working_space=linear/srgb、alpha_mode=premultiplied/straight、edge_mode=transparent/clamp/repeat/mirror、edge_param（布尔控制 transparent/clamp）、padding 表达式和 required_capabilities。

AE 效果另声明 reference_match_name/reference_version/compatibility_profile/compatibility/known_differences。当前基准 profile 为 AE 2021 18.0.1、8 bpc、sRGB IEC61966-2.1、非线性工作空间、方形像素、关闭运动模糊；不能用同名旧版 matchName 替代。

padding 表达式按当前采样参数计算四边对称外扩，不改变图层锚点或变换。支持 constant(value)、parameter(id,component)、add(a,b)、multiply(a,b)、max(a,b)、abs(value)、ceil(value)，嵌套最多16。最终结果必须非负且不超过32768。表达式所需临时纹理超过 64 MiB 或设备尺寸时错误，宿主不会缩小合法参数或裁切效果。

## WGSL 函数契约

```wgsl
fn main_fx(p: vec2<f32>) -> vec4<f32> {
    let c = sample_input(p);
    return vec4(c.rgb, c.a);
}
```

宿主注入全屏顶点/片元入口、uniform、纹理和工具函数。不要自行声明入口或绑定。p 是未变换图层的像素坐标，Y 向下；半径和位置保持逻辑像素单位，预览缩小由采样函数按 fx.output_mode.w 自动映射。所有效果在图层空间执行，然后才应用图层透明度、空间变换和摄影机。外扩像素允许负坐标。

`sample_input(p)` 读取上一 pass，首 pass 是效果原始输入；`sample_source(p)` 始终读取当前效果链位置的原始输入；边缘模式作用于逻辑区域，外侧透明模式返回零。`curve_lookup(vec4)` 读取宿主派生 LUT。PNG 可用 `textureSampleLevel(resource0,resource_sampler0,uv,0.0)`，资源字节以 RGBA8 读取；需要色彩解码时由算法明确调用 srgb_decode。

`fx` 是 624 字节、16 字节对齐的固定参数块，七个 vec4 后为 params[32]：

| 字段 | 含义 |
|---|---|
| size | 原图层逻辑宽高、当前 pass 实际像素宽高 |
| region | 输出区域 left/top/逻辑宽高 |
| input_region / source_region | 上一 pass / 当前效果输入区域 |
| clock | 图层局部秒、局部帧（可为负）、从0开始的 pass 索引、稳定种子 |
| mode | 边缘0/1/2/3、输入色彩0线性/1sRGB、输入Alpha0预乘/1直通、宿主原始资源标记 |
| output_mode | 输出色彩、输出Alpha、效果混合量、预览采样比例 |
| params | 按 manifest 顺序的四分量参数 |

SDK 1的clock各分量均为f32；seed由工程u32转换而来，超过2^24时相邻整数可能映射到同一值。随机效果使用clock.w的浮点位模式散列，保证同一工程、帧和包版本随机寻帧稳定；不要承诺所有u32种子都产生不同图像。需要完整32位种子时须升级参数块协议。

宿主会在效果前后转换 working_space/alpha_mode，合成统一使用线性预乘 Alpha。8 bpc 临时目标会量化并截断到0～1；扩展到HDR/浮点是后续宿主能力。内置 effect_opacity 由最终 pass 与 sample_source 混合，插件无需重复实现这个额外宿主参数。

循环只能使用字面整数上下界和每次+1，单循环最多1024次、保守组合最多65536次，不能在循环体修改/借用计数器。不支持 while、loop 或自定义绑定。WGSL 由 Naga 校验并生成 GLSL ES300，GLSL 的 uniform block 显式 std140。两端使用同一算法和参数布局；驱动编译仍在 GPU 首次准备时进行，插件作者应在目标真机运行回归。

## 安装与资源

压缩包与解压后文件合计分别最多16 MiB，manifest/WGSL单文件最多256 KiB；PNG维度及解码累计受128 MiB限制。拒绝绝对路径、父级路径、反斜杠、大小写重复路径、链接、无引用文件、未知字段和未知能力。只在验证完整成功后写入 `<SHA256>.msfx` 并原子安装。App 不执行 ZIP 内的其他文件。

预览缓存按精确 hash/效果/pass 编译，图片资源不逐帧上传，Curve LUT 仅在字节变化时更新。原图资源与插件资源有内存预算；临时纹理有单独64 MiB预算。无效果图层保持原有直接绘制路径。导出持有 Arc 包引用和原始字节，卸载不影响已开始的任务。

RenderStats.parameter_resource_upload_bytes单独统计Curve LUT上传；首个256项LUT为1024字节，未改变的后续帧为0，未计入素材上传。GLES资源按包hash与路径复用，素材与插件PNG合计最多128 MiB。

目前支持 single_frame、multipass、param_lut、dynamic_bounds、color_profile。暂不支持图层引用、蒙版、跨时间取帧、音频、生成图层或计算 shader；manifest 请求这些能力会拒绝安装。新增能力必须增加 SDK/协议支持与测试，不能仅增加一份效果名称。
