# 核心效果 1.3.0：动态拼贴与图像工具

`core-effects.msfx` 共 59 项，新增下列 7 项；一级分类共 10 类。原核心 1.0.0、1.1.0、1.2.0 精确包继续预装，场景效果 1.0.0/1.1.0 保持不变。工程不自动升级。新增包使用 SDK 3；旧包仍按原 SDK 解析。

| 一级分类 | ID | 中文 / English | 原生范围与已知限制 |
|---|---|---|---|
| 风格化 | motion_tile | 动态拼贴 / Motion Tile | 拼贴宽/高 0–100%；输出宽/高 0–30000%；位置 px，相位 deg。镜像、相位方向及动画均支持。输出分别扩展/裁切两轴，不修改锚点与变换。 |
| 扭曲 | optics_compensation | 光学补偿 / Optics Compensation | FOV 0–179.999984741211°，方向枚举 1–3；反转镜头扭曲。最佳像素及自动调整大小仅允许默认值。 |
| 扭曲 | spherize | 球面化 / Spherize | 半径 0–2500 px；中心 px；双线性球面映射。 |
| 扭曲 | cc_lens | CC 镜头 / CC Lens | Size 0–500%，Convergence −200–100%；独立镜头模型、圆外透明。 |
| 模糊与锐化 | cc_radial_fast_blur | CC 径向快速模糊 / CC Radial Fast Blur | Amount 0–100%；标准/最亮/最暗。64 个有限径向样本，内部核与 Cycore 不同。 |
| 遮罩 | simple_choker | 简单阻塞工具 / Simple Choker | −100–100 px；最终输出/遮罩视图；两 pass 方形形态核，分数半径插值。AE 边缘行为存在差异。 |
| 通道 | solid_composite | 固态层合成 / Solid Composite | 源与背景不透明度 0–100%；归一化 RGBA。正常混合支持，其余原生模式禁用。 |

这些是独立算法，全部保持 `approximate`。本机 AE 18.0.1x1 已采集实际 matchName、效果版本、参数默认值、硬范围、单位及动画属性；CC 效果额外记录 Cycore 版本。资料来源：[Adobe 风格化效果](https://helpx.adobe.com/my_en/after-effects/desktop/apply-effects-and-animation-presets/list-of-effects/stylize-effects.html)、[Adobe 扭曲效果](https://helpx.adobe.com/lu_en/after-effects/desktop/apply-effects-and-animation-presets/list-of-effects/distort-effects.html)、[Cycore 官方手册](https://www.cycorefx.com/downloads/cfx_hd_std/CycoreFX%20HD%201.8.9%20Manual.pdf)。

数值事实在 `tools/effect_metadata/ae18-tiling-ranges.json`，宿主映射见 [范围审计](tiling-range-audit.json)。AE 没有 min/max 的位置与角度使用宿主 ±1000000 有限数值边界；这是宿主限制。颜色归一化范围 0–1 与 AE 原始 HDR API 范围分开记录。离散枚举保留原生从 1 开始的索引。

## 动态拼贴与内存

中心是主拼贴中心；拼贴缩放、输出范围、镜像和相位互相独立。每隔一个列/行执行相位偏移，360° 对应一个完整拼贴周期；负坐标的奇偶判断也一致。拼贴宽/高为 0 时，根据 AE 参考折叠为平均色列/行，使用最多 64 样本近似；不直接当透明。输出宽/高为 0 时内部保留最小 1 像素透明矩形。

新增声明式 `output_bounds` 使输出宽高独立。宿主保留输入矩形容量以支持裁切，避免源图层先物化时写出纹理边界。效果输入为前序效果的完整输出矩形；在前序外扩之后的行为尚未通过 AE 完整对照。

SDK 3 矩形效果最后一个 pass 直接转换到合成纹理，省去一张扩展输出副本和一个转换 pass。1080×1920、输出 200%×200% 的动态拼贴计划临时纹理为 **41472000 字节（39.55 MiB）**，相同矩形的三纹理路径为 74649600 字节（71.19 MiB）。这个数字仅是计划临时纹理容量，不包含素材/最终输出/驱动分配，也不是手机帧率。SDK 1/2 旧包继续使用原执行路径。

64 MiB 预算与设备最大纹理尺寸仍有效，超过限制明确报告图层及效果实例，不静默改变参数或截断画面；大输出仍可能超限。稳定尺寸沿用现有跨图层、跨帧纹理池。

## 前端接入

App 效果面板无需解析边界表达式，读取 catalogue 的 category、params、min/max/default、units、animatable、implemented 和 options 即可。不要把软滑块范围当成硬范围：例如输出宽/高建议滑块常用段 100–400%，精确输入仍允许 0–30000%。宿主资源限制可能比原生参数范围先触发。

`implemented=false` 的控件显示原生默认值及原因，并禁用编辑；后端对修改后的工程/输出同样拒绝，不只是界面约束。枚举值用 `min + optionsIndex`，布尔只允许 0/1。参数和实例使用稳定 ID，拖动遵循现有 begin/set/commit 一次撤销契约。新实例选择 1.3.0，原工程精确版本及 hash 保持原样。

## 重建与 AE 参考

```powershell
py -3.14 tools/generate_tiling_library.py
cargo run -p aem-effects --bin effect_tool -- pack crates/aem-effects/library crates/aem-effects/library/core-effects.msfx
py -3.14 tools/ae_tiling_inputs.py
```

前两步只依赖 Git 中的原始 1.2.0 包和数值快照，最后一步需要 Pillow，生成物写入忽略的 artifacts/。历史生成器只重建自己的版本，不要用 1.1/1.2 生成器覆盖最新包。

在工具专有 AE 实例执行 `tools/ae_collect_tiling.jsx` 采集参数，执行 `tools/ae_render_tiling.jsx` 建立 24 个参考队列；不要连接用户已经打开的 AE。参考工程 8 bpc、sRGB、关闭工作空间线性化、方形像素、关闭运动模糊。随后关闭专有实例，再运行：

```powershell
py -3.14 tools/ae_render_cli.py "AE Support Files/aerender.exe" artifacts/ae-tiling/18.0.1 --reuse
cargo run -p aem-render --bin effect_probe -- artifacts/ae-tiling/18.0.1
py -3.14 tools/compare_tiling_effects.py artifacts/ae-tiling/18.0.1 --publish-report
```

对照报告只跟踪数值、参数快照和 SHA-256；AE 工程、参考/输出图和 gallery 都保留在 Git 忽略的 artifacts/。静态用例的阈值通过不代表完成效果验收；更广泛的动画/参数/变换/设备验收未完成时不计入已还原数量。
