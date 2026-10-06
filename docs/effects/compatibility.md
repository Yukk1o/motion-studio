# AE 2021 效果兼容矩阵

基准来自本机 AE 18.0.1x1。已验收数量为 **0**；阈值通过仅表示当前静态样本通过，不能代替完整验收。参数原始采集与逐项报告在 `crates/aem-effects/reference`。

| 效果 | matchName | 状态 | 已对照 / 用例 | 超阈值用例 |
|---|---|---|---|---|
| 亮度和对比度 / Brightness & Contrast | `ADBE Brightness & Contrast 2` | 近似实现 | 6/6 | 3 |
| 曝光度 / Exposure | `ADBE Exposure2` | 近似实现 | 6/6 | 0 |
| 色相/饱和度 / Hue/Saturation | `ADBE HUE SATURATION` | 近似实现 | 6/6 | 3 |
| 色调 / Tint | `ADBE Tint` | 近似实现 | 6/6 | 0 |
| 三色调 / Tritone | `ADBE Tritone` | 近似实现 | 6/6 | 0 |
| 颜色平衡 / Color Balance | `ADBE Color Balance 2` | 近似实现 | 6/6 | 3 |
| 色阶 / Levels | `ADBE Easy Levels2` | 近似实现 | 6/6 | 0 |
| 曲线 / Curves | `ADBE CurvesCustom` | 近似实现 | 3/3 | 0 |
| 高斯模糊 / Gaussian Blur | `ADBE Gaussian Blur 2` | 近似实现 | 6/6 | 2 |
| 快速方框模糊 / Fast Box Blur | `ADBE Box Blur2` | 近似实现 | 6/6 | 3 |
| 定向模糊 / Directional Blur | `ADBE Motion Blur` | 近似实现 | 6/6 | 3 |
| 径向模糊 / Radial Blur | `ADBE Radial Blur` | 近似实现 | 6/6 | 6 |
| 锐化 / Sharpen | `ADBE Sharpen` | 近似实现 | 6/6 | 1 |
| 钝化蒙版 / Unsharp Mask | `ADBE Unsharp Mask2` | 近似实现 | 6/6 | 2 |
| 镜像 / Mirror | `ADBE Mirror` | 近似实现 | 6/6 | 1 |
| 偏移 / Offset | `ADBE Offset` | 近似实现 | 6/6 | 1 |
| 凸出 / Bulge | `ADBE Bulge` | 近似实现 | 6/6 | 2 |
| 旋转扭曲 / Twirl | `ADBE Twirl` | 近似实现 | 6/6 | 3 |
| 波形变形 / Wave Warp | `ADBE Wave Warp` | 近似实现 | 6/6 | 6 |
| 极坐标 / Polar Coordinates | `ADBE Polar Coordinates` | 近似实现 | 6/6 | 3 |

参考采集、截图和原始报告保存在本地 reference/，不纳入 Git；干净检出不依赖这些资料。

## 核心1.2.0新增的16项

参数身份、默认值及硬范围已由本机18.0.1x1采集；新增16项全部为近似实现，AE图像对照尚未完成，不计入已验收数量。列表、支持的参数子集和差异见 [common-library.md](common-library.md)，可复查参数记录见 [common-range-audit.json](common-range-audit.json)。
