# 运动粒子与原生编辑器验收记录

日期：2026-10-07。基线 `2644f9cf5e210b4911493e3cd573ae656045a676`，分支 `codex/particle-emitter-trails-20261007`。实现范围和调用契约见 [粒子交接](../host-api/particle-emitter.md)、[原生 UI 插槽](../host-api/native-plugin-ui.md)。

## 构建与回归

| 验证 | 结果 |
|---|---|
| Rust workspace，离线 locked、单线程执行 | 184 成功、0 失败；原有大文件导入用例 1 项默认忽略 |
| Android arm64-v8a / x86_64 debug native + APK / test APK | 构建成功 |
| Android JVM 单元测试 | 10 成功 |
| Android lintDebug | 成功 |
| SDK 包重建与 effect_tool check | 重建 SHA-256 一致；WGSL、GLES 和 SDK 5 契约校验成功 |
| Android 设备测试 | 6 成功，26.609 s |

设备测试环境为 Android 35 x86_64 软件图形模拟器，独立 application ID `com.motionstudio.editor.effectsacceptance`，显示覆盖为 1080×1920 / 420 dpi。未使用用户手机，因此不把这些结果作为 Vivo Y300 Pro 的播放性能结论。原生 Surface 预览和竖屏时间轴已实测；低高度横屏滚动分支仍需真机补验。

设备用例包含：点击效果名称直接打开原生页面；共享 GPU 预览、播放与暂停；关键帧添加与定位；0 / 30 帧键值编辑，退出后 15 帧插值为 60；选中参数与外部 `effect:1:position` 同步；整页提交后一次撤销/重做；取消恢复进入前参数；旧 WebView 编辑器；原有粒子冻结 MP4、超限拒绝及残留输出清理；效果目录和既有参数面板回归。

出生算法回归覆盖源图层 / Null / 父级 / 片段时间、出生外观快照、速度继承、重力与阻力解析解、重复帧缓存复用、正序 / 倒序 / 随机寻帧、摄影机剔除后重新出现、保存恢复、种子、撤销 / 重做、缺失源与不支持的历史表达式、PNG 透明轮廓 / 宽高比 / 多实例共享纹理。

## 双端画面

未编码图像对照为 128×128、8 位通道单位（0–255），RGB 与 Alpha MAE 均要求 ≤3。运动粒子用例包含路径动画、自定义 16×8 PNG 和随机寻帧后计划字节一致性。

| 生成器 | RGB MAE | Alpha MAE | 可见实例 |
|---|---:|---:|---:|
| lens_flare | 0.293681 | 0.030273 | 7 |
| starfield | 0.203831 | 0.004639 | 400 |
| sparks | 0.442403 | 0.001160 | 63 |
| dust | 0.485774 | 0.007263 | 335 |
| snow | 0.119333 | 0.001221 | 77 |
| energy | 0.959413 | 0.011658 | 91 |
| particle_emitter + PNG | 0.367402 | 0.725769 | 84 |

旧 sparks 冻结 MP4 回归：全画面 RGB MAE 0.957113、前景 MAE 5.134612，满足 <6 / <8；导出过程中修改实时工程不影响冻结任务。新运动粒子的 PNG 已验收未编码双端输出，尚未单独记录其 Android MP4 编码误差。

## 桌面示例与测量边界

`particle_probe --sprite` 生成 6 秒、30 fps、960×540 的运动发射器和五角星 PNG 示例，共 180 帧。示例星形素材由工具生成；无第三方纹理、预设或插件二进制。

以下使用 NVIDIA GeForce RTX 4070 Laptop GPU / Vulkan、debug 构建，计时包含场景采样、wgpu 渲染和同步 RGBA 读回，排除 PNG / MP4 编码；数字是离线捕获吞吐，不是编码速度或手机预览 FPS。

| 时间段（s） | 渲染读回 FPS | 平均 CPU 准备（µs） |
|---|---:|---:|
| 0–1 | 59.25 | 676.17 |
| 1–2 | 79.70 | 832.80 |
| 2–3 | 77.24 | 882.37 |
| 3–4 | 79.88 | 843.77 |
| 4–5 | 75.68 | 915.30 |
| 5–6 | 79.23 | 859.47 |

## 资源与已知范围

新包：`com.motionstudio.effects.particles` / `1.0.0`，SHA-256 `37047f16cb9306b7e15d73c343f4f76fbf26c4ddfee4e1c2d3fe973939760e8b`。原场景包版本和已发布字节保留，不自动迁移旧粒子效果。JNI 计划版本仍为 4，工程无需额外格式升级。

支持路径发射、独立粒子轨迹、出生参数动画、单张工程 PNG，以及宿主原生槽。速率 / 寿命 / 力场首期固定，历史表达式、碰撞、连续 ribbon、Aux 和精灵图集暂未实现；不声明 Trapcode Particular 文件或算法兼容。原生时间轴首期绑定效果参数；图层变换槽修改已有图层属性，图层变换动画的首键和缓动仍从主编辑器操作。

本地交付保存在仓库根外层的 `artifacts/particle-emitter-trails/`：APK、原生页面截图、示例视频、180 帧、原始 JSON 和测试日志均被 git 忽略。debug APK 的 SHA-256 为 `f21603c21450efb683130604bd9d5c0a2e454d4f1fab09ea2428bd5e2d380ed5`。仓库只提交源码、可重建 `.msfx` 包与 SDK 文档。

复现设备用例：

```powershell
python tools/build_android.py --abis arm64-v8a,x86_64 --effects-acceptance --task assembleDebug assembleDebugAndroidTest testDebugUnitTest lintDebug
adb -s <serial> install -r android/app/build/outputs/apk/debug/app-debug.apk
adb -s <serial> install -r android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk
adb -s <serial> shell am instrument -w -r -e class com.motionstudio.editor.NativeParticleEditorTest,com.motionstudio.editor.SceneEffectsTest,com.motionstudio.editor.IntegratedFrontendTest#catalogueChainParametersAndUndoWorkFromThePanel com.motionstudio.editor.effectsacceptance.test/androidx.test.runner.AndroidJUnitRunner
```

## 同步子合成与预览优化主分支后的补验

已同步主分支 `0ca9665`（PR #28 子合成、PR #31 预览性能）。保留按活动合成路由的
`CompositionBridge.plugin` 与原生页提交/取消事务；子场景携带 PNG 元数据和出生历史。
新增 GPU 用例验证在子合成内创建运动粒子、引用到父合成后的 PNG、出生状态、
正序/倒序/随机寻帧画面、引用撤销重做及保存恢复，帧间对照 MAE ≤3。
场景生成器的现有图层预合成限制继续生效；该用例使用已有合成引用 API。

补验结果：Rust workspace `motion-android/diagnostics` 206 成功、0 失败、1 个原有用例忽略；
arm64-v8a / x86_64、APK / test APK、10 个 JVM 用例、lint 构建成功。
11 个设备用例执行 46.792 s，其中原生粒子/场景/目录 6 项与合成后端 3 项成功；
2 项原有子合成未编码对照未达阈值，保留为环境兼容性待查项：

| 用例 | RGB MAE | Alpha MAE | 前景 RGB MAE |
| --- | ---: | ---: | ---: |
| 混合帧率、重复 3D 引用与空间效果 | 3.389263 | 6.591417 | 8.829046 |
| 嵌套动画矢量与调整图层 | 1.821311 | 0.519803 | 3.041541 |

在同一软件图形模拟器上，重新安装 PR #28/#31 集成构建 `714062b` 并单独执行这两项，
得到相同误差；其 Git tree 与 `0ca9665` 相同，排除了新增运动粒子作为该差异来源。
此前主分支 GitHub Actions 模拟器验收通过，不能将本次本地模拟器差异表述成所有设备失败，
也不能将本次 11 项表述为全部通过。原始日志分别为 `integrated-android-tests.log`、
`baseline-composition-parity.log`，保存在本地 artifacts。

更新后的本地 APK 为 109,517,131 B，SHA-256
`63010e03bb012877e4bfe8edf79fc8ad5a4b27b000cde9cfc0d2bf857427706b`。
上文首次交付 APK 的哈希保留为历史记录。CI 已加入原生粒子页及 SceneEffects 用例。
