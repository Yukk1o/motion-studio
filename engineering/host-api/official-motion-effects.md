# 官方创作效果、置换来源与共享字体 API

效果 SDK **6**，GPU 帧计划 **7**。宿主支持工程格式 **1–9**；本功能在现有工程增加可选的 `fonts` 和效果实例 `image_input`，无需因添加这些字段单独升级工程版本。格式 9 用于空间路径，缺失字段保持旧工程语义。原核心包 1.4.1 与所有历史包字节不改写，统一新版为 2.0.0。

## 官方效果目录

`NativeBridge.plugin(session, {"op":"catalogue"})` 自动返回统一内置包 `com.motionstudio.effects.ae2021` / `2.0.0`（沿用原插件 ID，显示名为 Motion Studio 内置效果）。`packages` 中取得准确版本/hash，再沿用已有 `add`、数值、动画与曲线 API。

内置包共 95 项：原 59 项保持参数契约，新增 36 项如下。源码按组维护，分发与效果目录使用一个包：

| 分类 | 效果 ID |
|---|---|
| 调色 | solarize, vignette, vibrance, gradient_map |
| 风格与过渡 | halftone, dither, contour_lines, crt_screen, vhs, ascii, drop_shadow, compression_artifacts, noise_dissolve |
| 变形 | flip, bend, stretch, corner_pin, fluted_glass, displacement_map |
| 模糊 | channel_blur, region_blur, bokeh_blur |
| 噪声、背景与渐变 | noise_generator, curl_noise, swirl_pattern, aurora, gradient, multi_point_gradient, flow_gradient, grid_gradient |
| 材质 | marble, paper, wool, weave, brushed_metal, carbon_fiber |

参数类型、范围、默认值、动画支持与差异以 manifest 为准。主要枚举从 **0** 开始：

- `gradient.shape`：Linear / Radial / Conic / Diamond / 圆形 HSV / 方向彩虹。
- `noise_generator.basis`：Perlin / Simplex / Voronoi / Gabor / Block / Blue。`octaves` 用于 Perlin；Blue 为静态 64×64 像素平铺，频谱检查记录随源文件保存。
- `region_blur.mode`：Progressive / TiltShift，共用可变半径模糊；最终 pass 保留清晰带原像素。
- `bokeh_blur.quality`：16 / 32 / 64 个确定性光圈采样。

目录还返回 `aliases`（名字、插件 ID、效果 ID、参数预设）：Duotone→tint、RectangularCoordinates→polar_coordinates、ChromaticAberration→warp_chroma、AngularBlur→radial_blur、SwirlDistortion→twirl、BlockNoise→fractal_noise；渐变、噪声、区域模糊的细分名称对应模式。别名不能作为保存工程中的新效果 ID。上游 Swirl 背景独立为 `swirl_pattern`，MeshGradient 流动背景对应 `flow_gradient`，规则双线性网格为 `grid_gradient`。

颜色以声明的 working space 计算；默认 sRGB straight，宿主转换到合成目标。参数位置使用图层逻辑像素；长宽比、预览缩放和小数帧沿用现有宿主契约。各效果保留 `effect_opacity=0` 的精确旁路。模糊为有界采样近似；材质为二维表面；CompressionArtifacts 为同帧四 pass 的 8×8 DCT/量化模拟，RGBA8 系数精度，不宣称完整 JPEG 编码或其他软件像素兼容。具体限制写入每项 `known_differences`。

## 图层与素材作为置换来源

`displacement_map` 默认使用包内 PNG；以下请求改为同一合成内另一图层的像素：

```json
{"op":"image_input","object":10,"instance":1,"input":{"kind":"layer","layer":20,"stage":"effects"}}
```

调用 `NativeBridge.plugin`。源图层可以是图片、文字栅格、矢量、视频、子合成，以及带噪声/渐变等效果的普通图层；空对象、音频和调整图层没有独立像素，不能作为来源。

- `effects`（省略 stage 时默认）：同帧源图层的蒙版和效果输出；不叠加图层合成变换与不透明度。场景生成器仍按其自身相机/世界空间契约生成像素。
- `source`：原始内容和固有颜色，不包含蒙版、效果、合成变换、不透明度。
- 源可以隐藏，仍按自己的 clip/source time 求值；超出片段时间则为中性置换。源图像整个输出矩形拉伸映射到目标输入矩形。
- effects 阶段建立依赖 DAG，拒绝循环。多个消费者共享一次 GPU 求值和快照；层叠顺序不会改变来源。没有上一帧反馈和应用层 GPU 帧回读。
- `amount` 为 XY 逻辑像素，`channels` 为 RG / 亮度，`midpoint` 默认 0.5；透明来源按 Alpha 衰减位移。图层来源按显示 RGB 数值解释。

另外两种覆盖来源：

```json
{"op":"image_input","object":10,"instance":1,"input":{"kind":"asset","asset":30}}
{"op":"image_input","object":10,"instance":1,"input":{"kind":"empty"}}
```

传 `input:null` 恢复包内资源；Empty 是明确的空来源。删除源图层自动置 Empty，撤销可以恢复。绑定是结构编辑，支持历史、复制、保存和重开；图层引用不得跨合成拆分边界。PNG 素材采用现有素材加载、打包与冻结导出通路。

多选复制时，选区内来源绑定到对应的副本，选区外来源仍引用原图层。来源图层或绑定素材已移除时拒绝粘贴；原始像素阶段允许互相引用，粘贴会在一次原子批次内先创建全部副本再绑定来源。字体生成资源沿用普通素材编辑的注册、预览刷新及撤销路径，可直接用于已打开的预览。

SDK 6 的 `image_input` capability 要求 image renderer、恰好一个包内资源、最多 30 个参数；宿主保留 slots 30/31。GPU 计划 7 复用 image pass 的 word 8：0=包内资源，负数=素材 slot，正数=源 draw index+1；sprite pass 保持原 start/count。wgpu 和 GLES 都支持这条协议。快照消耗计入源纹理预算，超预算报告错误，不缩改用户参数。

## 系统字体与用户导入

系统目录由 `NativeBridge.systemFonts()` 返回 JSON：`protocol/fonts/errors/variableAxes`。读取 Android 系统/OEM 字体配置及字体目录，条目包括 `family/name/weight/style/path/face_index/axes/bytes`。选择后调用同一原生导入：

```json
{"op":"font_import","path":"/system/fonts/Roboto-Regular.ttf","face_index":0,"license":"字体原许可说明"}
```

用户选择文档 URI 后，在 session 工作线程调用 `NativeBridge.importFont(session, context, uri, faceIndex, license)`；辅助函数通过 SAF 读取并清理私有暂存文件，不负责文件选择 UI。

TTF / OTF / TTC 共享 Rust 字形模块。单文件上限 32 MiB，字号 4–256 px，最多保留 2 个解析 face；覆盖栅格缓存最多 16 MiB / 2048 entries。face 缓存数量与源字节限制不等于字体解析对象的精确 RAM 上限。字形位图不因每帧改变而重建。

字体按 SHA-256 与 collection face index 标识；所用源文件复制进 `assets/fonts/`，名称、face index 和许可记入工程 `fonts`。工程打包带上源字体和生成栅格，换设备后不依赖原机系统字体。内置 IBM Plex Mono 使用 SIL OFL 1.1，源字体与完整许可保留；不捆绑第三方系统字体。

```json
{"op":"font_catalogue"}
{"op":"font_atlas","font":"返回的字体 ID","size":32,"characters":" .:-=+*#%@","object":10,"instance":1}
{"op":"font_text","font":"返回的字体 ID","size":48,"text":"Motion Studio\nHello","width":1024}
```

`font_atlas` 支持 1–256 个不同可打印字符，按实际覆盖率排序，返回素材、排序字符、cell 尺寸和覆盖率。有 object/instance 时，必须指向官方 ASCII；素材绑定、`glyph_count` 和 `glyph_aspect` 一次性提交并可撤销。默认 IBM Plex Mono；没有自定义图集时 ASCII 用内置原创位图。

`font_text` 返回带 Alpha 的可复用 PNG 素材，可通过现有 Content API 用于文字图层。当前为基本从左到右排版、字距和换行；复杂文字塑形、双向排版、字体 fallback、彩色 Emoji 和可变字体 axes 尚未提供。缺失字形明确报错。将来的塑形层可继续复用字体身份、资源与栅格缓存。

## 验证与成本

测试覆盖 36 项真实 GPU 输出、旁路、随机小数帧、通道独立半径、焦点带、渐变模式、透明色、隐藏来源、层叠顺序与字体图集；图层依赖循环与撤销在核心测试中验证。字体缓存与工程包也有独立测试。

`MOTION_EFFECT_REPORT` 在原 `effect-costs.json` / `.md` 路径产出统一内置包全部 95 项的 4K 计划成本，scope 为 `current_builtin_manifest`；CI 汇总并上传这份报告。这个默认单层报告不包含多图层来源快照，不冒充设备性能测量。

构建内置包：先运行 `generate_motion.py`，再运行 `generate_builtin.py`，最后 `effect_tool pack crates/aem-effects/builtin-library crates/aem-effects/builtin-library/builtin-effects.msfx`。SDK 6 单包上限 128 项、manifest 上限 512 KiB；旧 SDK 单包上限仍为 64 项，layer 参数/pass/循环/资源限制不变。
