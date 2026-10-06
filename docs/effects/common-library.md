# 核心效果 1.2.0 与性能修正

核心包 `core-effects.msfx` 现有 52 项效果；同时预装 1.0.0、1.1.0 原始包，既有工程仍按精确版本和 SHA-256 加载。新实例从 catalogue 选择最新兼容版本，升级旧实例需要显式操作。文件名和产品名称不使用 AE 版本名，内部插件 ID 为兼容旧工程保持稳定。

本批增加 16 项独立 WGSL 实现，全部为 `approximate`。本机 AE 18.0.1x1 已采集参数身份、默认值、API 范围、单位和可动画性；这不等于完成 AE 画面对照。首期没有新增 App 面板，前端依据 manifest 渲染控件。

| 一级分类 | ID / 中文 / English | 支持与差异 |
|---|---|---|
| 调色 | invert / 反相 / Invert | RGB、红、绿、蓝、Alpha；不含 HLS/YIQ；宿主菜单 5 对应 AE 的 16 |
| 调色 | black_white / 黑白 / Black & White | 六色色相权重、着色；独立灰度响应 |
| 调色 | channel_mixer / 通道混合器 / Channel Mixer | 十二项系数与恒量、单色；保留 Alpha |
| 调色 | posterize / 色调分离 / Posterize | 2～255 级，采样时取整，最近色阶量化 |
| 调色 | threshold / 阈值 / Threshold | 固定 `ADBE Threshold2`；按 BT.601 亮度二值化 |
| 风格化 | find_edges / 查找边缘 / Find Edges | 逐通道中心差分、反转、与原图混合 |
| 风格化 | emboss / 浮雕 / Emboss | 方向、起伏、对比度、原图混合；两点亮度差 |
| 风格化 | color_emboss / 彩色浮雕 / Color Emboss | 浮雕响应叠加原色 |
| 风格化 | mosaic / 马赛克 / Mosaic | 水平/垂直块数、锐化颜色；普通模式每块固定 16 点近似均值 |
| 扭曲 | turbulent_displace / 湍流置换 / Turbulent Displace | 数量、大小、偏移、复杂度、演化、种子；仅基本湍流，Alpha 随位移搬移 |
| 生成 | gradient_ramp / 渐变 / Gradient Ramp | 线性/径向、起终点颜色、散射、原图混合；保留 Alpha |
| 生成 | fill / 填充 / Fill | 颜色、不透明度；不含蒙版/反转/羽化 |
| 过渡 | linear_wipe / 线性擦除 / Linear Wipe | 完成度、角度、羽化 |
| 过渡 | radial_wipe / 径向擦除 / Radial Wipe | 完成度、角度、中心、方向、羽化 |
| 过渡 | venetian_blinds / 百叶窗 / Venetian Blinds | 完成度、方向、条带宽度、羽化 |
| 生成 | fractal_noise / 分形杂色 / Fractal Noise | 基本值噪声、多层细节、演化、缩放、种子等；样条目前等同柔和线性 |

全部新增效果在 sRGB 直通 Alpha 空间执行，最终由宿主转换为合成空间。RGB 输出为 8 bpc 有界值；除反相的 Alpha 模式、扭曲及擦除外保留源 Alpha。生成效果是在当前图层覆盖范围内改色，不向完全透明区域扩展内容。湍流和分形的演化使用周期性的双噪声插值，未复现 AE 非循环演化；不自动增加时间，用户可对 evolution 写关键帧或表达式。

基于 [Adobe 调色说明](https://helpx.adobe.com/after-effects/desktop/adjust-colors/work-with-color-correction-effects/color-correction-effects.html)、[风格化说明](https://helpx.adobe.com/after-effects/desktop/apply-effects-and-animation-presets/list-of-effects/stylize-effects.html)、[生成说明](https://helpx.adobe.com/after-effects/desktop/apply-effects-and-animation-presets/list-of-effects/generate-effects.html) 与 [过渡说明](https://helpx.adobe.com/after-effects/desktop/apply-effects-and-animation-presets/list-of-effects/transition-effects.html) 理解效果行为；算法未使用 Adobe 或第三方插件代码。参数数值以本机版本采集为准。

## 有效范围

可复查的参数快照在 `tools/effect_metadata/ae18-ranges.json`；76 个新增控件的原始 AE 值、宿主值及范围依据在 [common-range-audit.json](common-range-audit.json)。快照包含原有 20 项的数值范围复核；除颜色及宿主曲线外，提供的有限范围与实采结果一致。SDK 自定义/创意参数范围由宿主契约定义，不能标成 AE/Sapphire 原版硬范围。

| 控件 | 有效范围 | 说明 |
|---|---|---|
| 色调分离级别 | 2～255 | 浮点轨道，执行时取整 |
| 黑白六色权重 | −200～300 | AE 实采，百分比意义 |
| 通道混合系数/恒量 | −200～200 | AE 实采，算法除以 100 |
| 阈值（新效果） | −30000～30000，默认 0.5 | 原始归一化值；8 bpc 常用 0～1，合法硬范围不能缩成滑块范围 |
| 马赛克块数 | 1～4000 | 执行时取整 |
| 湍流数量/大小/复杂度 | −10000～10000 / 2～1000 / 1～10 | 合法参数不代表设备预算一定充足 |
| 擦除完成度/羽化 | 0～100% / 0～32000 px | 0% 完全保留，100% 完全透明，即使羽化很大 |
| 百叶窗宽度 | 1～32000 px | AE API unitsText 为百分比；本算法按像素解释，差异明确保留 |
| bool / enum | bool 为 0 或 1；enum 为描述文件的整数值 | 阶跃关键帧，不允许小数菜单值 |
| 颜色 | 每个 RGBA 分量 0～1 | 宿主 RGBA8 契约；不沿用 AE API 暴露的 HDR 超大范围 |
| 无 AE 上下界的角度/坐标 | ±1,000,000 | 宿主有限数值限制，不宣称是 AE 硬范围 |

新核心包将 Tint/Tritone 的颜色范围收紧为 0～1；AE API 自身返回约 ±3,921,568，并非采集脚本换算错误。旧包和工程依赖不变。包导入校验 bool 范围、enum 选项数量和整数默认值；项目轨道、保存的关键帧及渲染采样都拒绝非法值。命令失败不修改工程或撤销历史。

前端必须读取该实例对应精确包的 min/max，而非为效果名硬编码范围。数值输入按硬范围检查，滑块可提供常用交互范围，但不得覆盖硬范围或静默修改数值。step 是交互增量，不代表 Float 参数只能取 step 的倍数。不能对 enum 直接使用从 0 开始的选项下标：序号为 `min + index`。不支持的 AE 控件列在 known_differences，不能展示成已实现的选项。

```json
{"op":"add","object":2,"plugin":"com.motionstudio.effects.ae2021","version":"1.2.0","hash":"catalogue 返回的精确 SHA-256","effect":"linear_wipe"}
{"op":"effect","object":2,"action":{"kind":"set","effect":3,"param":"completion","frame":0,"value":[40,0,0,0]}}
```

effect=3 仅为示例，必须从返回 state 读取真实实例 ID。动画、手势撤销、曲线及表达式继续使用既有接口。

## 临时纹理与性能

此前所有工作槽位按整条链的最大宽高分配并向上凑到 128 像素；单 pass 还预留第三张工作纹理。色彩空间混用或扩大半径后可能产生不必要的 64 MiB 错误。

现在每个槽位按实际写入 pass 的宽高高水位分配；单 pass 只占原输入和输出两张工作纹理。wgpu/GLES 都保留原色彩格式，SDK 和帧计划仍兼容原协议；两端适配器须一起更新以获得相同内存收益。没有降低参数、预览分辨率或裁切输出。效果不透明度为 0 时省掉该效果所有 pass、转换和外扩，同时保留启用依赖与参数预检。

1080×1920 的“极坐标 + 边缘发光，半径 48px”原方案需要 70 MiB，新方案需要 **50.8623 MiB**，减少 **27.34%**，在 64 MiB 内完整渲染。真正超过预算仍显式报错，包含图层、实例、所需 MiB 或具体纹理尺寸与设备上限；预览继续使用该效果输入，正式导出停止。

该收益是纹理内存的测量/计划核算，不应宣称 GPU 帧耗时也提高 27.34%。半径改变引起容量变化时仍需重建纹理；分块执行、容量增长策略、降低固定采样成本及多档预览尚未实现。不能自动裁到摄影机可见区域：扭曲和模糊可能需要区域外的原像素。物理设备的持续帧率和内存压力仍需后续验收。

## 重建

```powershell
python tools/generate_common_library.py
cargo run -p aem-effects --bin effect_tool -- pack crates/aem-effects/library crates/aem-effects/library/core-effects.msfx
cargo test --workspace -- --test-threads=1
```

新生成器始终从不可变的 1.1.0 包和参数快照构建，重复执行不会删除新效果或依赖本地参考目录。`generate_creative_library.py` 仍是旧 1.1.0 的生成工具，不能用于构建最新包。`tools/ae_collect_common.jsx` 供复核实机参数，必须在独立、工具持有的 AE 实例执行；生成的项目、日志、图像均放入忽略的 artifacts。所有 reference/refer 资料继续不进 Git。
