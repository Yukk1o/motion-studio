# Motion Studio 核心效果库 1.1.0

`crates/aem-effects/library/core-effects.msfx` 包含36项效果，新增16项直接放入核心库。已发布1.0.0的20项描述、WGSL及完整包字节保持不变并继续预装，既有工程不会自动升级。包显示名为“Motion Studio 核心效果”；内部插件ID `com.motionstudio.effects.ae2021` 保留用于解析工程身份，界面使用manifest.name。

## 一级分类

必需字符串字段 `EffectDefinition.category` 随目录接口返回。核心库共六类：

| category | 数量 | 新增效果ID |
|---|---:|---|
| 调色 | 8 | 无 |
| 模糊与锐化 | 6 | 无 |
| 扭曲 | 9 | warp_chroma、kaleido、kaleido_polar |
| 光效 | 7 | glow、glow_edges、rays、edge_rays、streaks、glint、light_leak |
| 运动 | 2 | shake、transform_blur |
| 风格化 | 4 | grain、scan_lines、film_damage、digital_damage |

前端按category分组，按name/english_name显示双语名称。自定义包可以使用其它分类；工程实例仍按插件ID、版本、hash和效果ID联合解析。

## 新增效果与兼容状态

Sapphire名称用于指出视觉参考，依据[Boris FX官方效果目录](https://borisfx.com/documentation/sapphire/ae/summary-index/)。实现使用独立WGSL算法与参数子集，尚未采集Sapphire原版渲染、内部matchName或精确参数快照。16项均为 `approximate`，profile为 `motion-creative-sapphire-inspired-v1`，reference_match_name为空；不能计入已还原数量。

| ID | 中文 / English | 视觉参考 | 实现与主要差异 |
|---|---|---|---|
| glow | 柔光发光 / Glow | S_Glow | 高光阈值后横纵各65点高斯采样并叠加；默认半径16px、亮度1、阈值0.5。大半径仍用固定点数。 |
| glow_edges | 边缘发光 / Glow Edges | S_GlowEdges | 中心差分提取亮度边缘，随后两次高斯采样，共3pass。 |
| rays | 放射光束 / Rays | S_Rays | 向中心64点径向积分，默认长度0.8、衰减2；固定输出边界。 |
| edge_rays | 边缘光束 / Edge Rays | S_EdgeRays | 先提取边缘再作径向积分，共2pass；固定输出边界。 |
| streaks | 高光光条 / Streaks | S_Streaks | 按角度65点方向采样，默认半径16px，指数衰减核。 |
| glint | 高光星芒 / Glint | S_Glint | 四条相隔45°的轴、每轴49点；方向与半径可调，没有光学衍射模型。 |
| light_leak | 镜头漏光 / Light Leak | S_LightLeak | 有种子的移动椭圆光斑与屏幕混合；默认暖色、半径120px、速度0.25Hz、强度0.5，保留原Alpha。 |
| warp_chroma | 色差分离 / Chromatic Warp | S_WarpChroma | 红蓝沿中心径向反向偏移，绿色保持原位置，默认8px；没有全部原版变形模式。 |
| kaleido | 镜面万花筒 / Kaleidoscope | S_Kaleido | 折叠角度扇区，默认6瓣、镜像边缘；segments采样后取整。 |
| kaleido_polar | 极坐标万花筒 / Polar Kaleidoscope | S_KaleidoPolar | 角度折叠加径向重复，默认环宽64px。 |
| shake | 镜头抖动 / Camera Shake | S_Shake | 平滑哈希噪声驱动平移、旋转和缩放，默认12px、1°、0.02、6Hz；固定图层尺寸。 |
| transform_blur | 变换运动模糊 / Transform Motion Blur | S_BlurMoCurves | 32点显式曝光路径积分，默认曝光平移16px；不自动计算动画曲线速度，不取相邻素材帧。 |
| grain | 胶片颗粒 / Film Grain | S_Grain | 分块均匀整数哈希噪声，默认强度0.08、尺寸1px、单色、24Hz；无胶片库存响应或相关颗粒模型。 |
| scan_lines | 扫描线 / Scan Lines | S_ScanLines | 水平/垂直余弦条纹，默认强度0.35、线距4px；无电视色彩/交错场处理。 |
| film_damage | 旧胶片损伤 / Film Damage | S_FilmDamage | 颗粒、点状灰尘、竖向划痕、闪烁及跳片，默认12Hz；无毛发、污渍或自动失焦。 |
| digital_damage | 数字故障 / Digital Damage | S_DigitalDamage | 条带错行、通道偏移及亮度闪断；默认概率0.35、位移32px、条带8px、色差4px、12Hz；无跨帧数据损坏。 |

Sapphire的[BlurMoCurves说明](https://borisfx.com/documentation/sapphire/ae/blurmocurves/)使用动画变换曲线产生模糊，变换常量时没有运动模糊。本库transform_blur使用用户指定的曝光位移/旋转/缩放，静态参数也可产生模糊。这项差异应在帮助说明中展示。

完整参数ID、类型、范围、默认值、单位、可动画性与已知差异以 `library/manifest.json` 及目录接口为准。新增参数ID为稳定英文名；原20项仍使用p0001等ID。新增参数均支持动画，bool/enum使用阶跃关键帧。最后的effect_opacity为0～100%宿主混合量，默认100%。

## 渲染规则与限制

光效（漏光除外）、色差、万花筒、抖动及变换模糊在**线性预乘Alpha**空间运算。高光阈值从还原直通RGB的亮度计算；affect_alpha控制光效外扩Alpha，RGB保持有效预乘范围。色差取各通道采样Alpha最大值，完全透明处RGBA归零。万花筒、抖动和变换模糊采样并搬移Alpha。

漏光、颗粒、扫描线、胶片损伤及数字故障在**sRGB直通Alpha**空间运算。前三项保留原Alpha；胶片损伤随跳片采样移动Alpha；数字故障跟随绿色采样Alpha，完全透明输出RGB归零。漏光不在原透明区域生成内容。宿主在链位置前后转换，最后统一线性预乘合成。

glow、glow_edges、streaks、glint按 `abs(radius) × expand` 外扩，warp_chroma按 `abs(amount) × expand` 外扩。宿主向上取整像素边界，不移动锚点。其它新增效果保持当前输入边界；expand=false显式选择固定边界。资源不足时报错，不自动减小合法参数。

带edge参数的效果提供透明/钳制/重复/镜像，值0/1/2/3。光效默认透明，万花筒与运动/损伤默认镜像。所有临时输出8 bpc、截断至0～1；没有HDR增亮、外部背景/遮罩、Mocha跟踪或跨时间取帧。

边缘发光3pass，发光及边缘光束2pass，其余新增项1pass。固定点数限制每帧工作量，大半径画质与复杂链性能需要目标设备验收；参数范围不保证任意尺寸都满足64MiB纹理预算。

时间使用图层局部秒。颗粒、胶片损伤及数字故障按 `floor(local_seconds × rate)` 更新，rate=0固定随机状态；镜头抖动与漏光随时间平滑变化；扫描线由speed驱动，默认静止。相同包、种子、输入和局部时间产生相同结果，不依赖前一帧。SDK 1种子f32精度限制见 [sdk.md](sdk.md)。

关闭强度/混合量可保留输入；transform_blur将曝光平移、旋转和缩放归零可保留静态变换后的结果。位置/半径使用未变换图层像素，默认中心由宿主初始化；预览缩小统一由采样比例换算。

## 前端请求与版本

先调用catalogue，选择启用的核心1.1.0并读取hash。不要用目录数组首项作为推荐版本；旧工程按精确版本与hash加载，新实例使用最新兼容semver版本。

```json
{"op":"add","object":2,"plugin":"com.motionstudio.effects.ae2021","version":"1.1.0","hash":"目录中该版本的SHA-256","effect":"glow"}
```

从返回state读取实例ID；例如ID为3，通过command编辑：

```json
{"op":"effect","object":2,"action":{"kind":"set","effect":3,"param":"radius","frame":0,"value":[24,0,0,0]}}
{"op":"effect","object":2,"action":{"kind":"animate","effect":3,"param":"strength","frame":0,"enabled":true}}
{"op":"effect","object":2,"action":{"kind":"set","effect":3,"param":"strength","frame":30,"value":[2,0,0,0]}}
```

所有请求固定四分量。新增color为RGBA，目前光色算法使用RGB并保留第四分量。选择1.0.0时仅显示该版本20项；切换版本使用显式upgrade接口，会重置参数、关键帧和种子，可撤销。面板、分类折叠、控件与动画入口由前端开发实现，见 [frontend-integration.md](frontend-integration.md)。

## 构建与回归

```powershell
py -X utf8 tools/generate_creative_library.py
cargo run --offline -p aem-effects --bin effect_tool -- pack crates/aem-effects/library crates/aem-effects/library/core-effects.msfx
cargo test --workspace --offline -j 1
$env:MOTION_EFFECT_GALLERY='<本地输出目录>'
cargo test --offline -p aem-render --test creative_effects
```

新增GPU测试以程序生成的透明边缘、渐变、色块和高光脉冲检查16项默认输出、关闭效果、外扩Alpha/锚点、随机寻帧、局部时间与种子变化。包测试检查1.0.0原字节/描述/WGSL保持不变，两个版本安装、重载和独立禁用。

Android EffectsRuntimeTest逐项编译36项GLSL并对照未编码wgpu输出，阈值RGB及Alpha MAE≤3/255；原有链编码、依赖检查和冻结资源测试保留。执行结果见 [validation.md](validation.md)。参考图片、截图和原始报告仅存本地，reference/及refer/均被忽略，构建测试不依赖它们。
