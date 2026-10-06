# 核心 1.3.0 验证记录

代码来源为 `5d5516ae61866865da37224c38f451c50ee8dbf4`；后续交付提交只更新文档。核心包 SHA-256 为 `a2655482198ebc3a1e75b569a8d5e14d8232c9ad2445c24fc9badfb6274aa454`。重新生成、打包后得到相同 hash；核心 1.2.0 原始 hash `14ef270c1199166fdc12e0d0ad43f7789256086fd24aaa76721e7bd31565adf6` 继续保留。场景效果包未改变。

## 已完成

- `cargo test --workspace --offline -- --test-threads=1`：115 项通过。新增覆盖 SDK 版本/矩形校验、原生数值范围、裁切源纹理容量、独立扩边/预览比例、纹理预算、镜像与相位像素、未实现控件拒绝、保存恢复与随机寻帧。
- Clippy 检查通过；现有 large_enum_variant、derivable_impls、encode_pass 参数数量、manual_clamp 与旧测试 manual_range_contains 警告保留。
- 59 项 WGSL/Naga/GLES 300 编译通过；24 个实际桌面 GPU 对照渲染均成功，设备为 NVIDIA GeForce RTX 4070 Laptop GPU。
- 本机 AE 18.0.1x1 实际采集 28 个参数，完成 24 个 8 bpc/sRGB/非线性/方形像素/关闭运动模糊静态参考输出；数值见 [tiling-validation.json](tiling-validation.json)。RGB 在可见像素上统计，Alpha 在区域全部像素上统计，内部与边缘分别计算。
- 24 个用例中 14 个通过现有全图/内部/边缘联合阈值。动态拼贴 11 个用例的非空区域 RGB MAE 最大 0.646/255、Alpha 为 0；半透明密集拼贴的内部区域 mask 为空，严格联合判定不将其计为通过。空 RGB 内部区域不代表已证明一致。
- 动态拼贴 1080×1920、输出 200%×200% 计划容量 41472000 B（39.55 MiB），无需降分辨率或裁切；此为纹理容量测试，不是手机耗时/驱动实际内存测试。

## Android 最终构建与执行

ARM64、x86_64 原生库以及 debug/test APK 构建成功。受本机内存限制，使用单构建任务，并对 aem-effects/aem-core/aem-render/aem-android 设置本地 release codegen-units=8；opt-level=3、thin LTO 保持原配置，仓库 release profile 未修改。本次没有复验默认 codegen-units=1 的构建。

在 MuMu 独立验收实例（Android 15、x86_64，独立 effectsacceptance App ID）运行 `EffectsRuntimeTest#allFiftyNineEffectsCompileOnDeviceAndUnencodedGlesMatchesWgpu`：1 项仪器测试通过，耗时 18.068 秒，逐项比较 59 项未编码输出，最大 RGB MAE **0.143376/255**、Alpha **0**。动态拼贴使用宽度 50%、输出宽 200%、镜像和相位 90°；光学补偿、球面化、阻塞工具也使用非默认参数。这是 GLES/wgpu 一致性，不是 AE 还原误差。

设备报告中 59 项包 hash 均与最终包一致。安装的 App/Test APK hash 与本地最终构建一致：

| 产物 | SHA-256 |
|---|---|
| App debug APK | `67335845fe3419ff0618f4dd01b13379d9075eb413ce26f476c7c057d69f65c2` |
| AndroidTest APK | `48b1b0219bb96cc6e564726225bd48aa46b587fcfefd05b5eee0a6a23ac973d3` |

原始日志及输出在 Git 忽略的 artifacts/：rust-tests.log、clippy.log、android-build.log、android-raw-tests.log、android-gles-report.json、provenance.json，以及 ae-tiling/18.0.1 下的 AE 工程、原始 TIFF、PNG、参数快照和 gallery。Git 只收入源码、数值快照、范围与误差报告；不收入 refer/reference 或渲染素材。

## 验收边界

球面化、镜头、模糊、阻塞工具的图像算法仍有差异。简单阻塞工具目前为方形形态核，在半透明平台区与 AE 的收缩/扩张处理差异明显；不应据此承诺 Alpha 清边等价。光学补偿的最佳像素/自动扩边及固态层合成的非默认混合模式拒绝启用。

新增效果全部保持 approximate，已验收数量没有增加。尚未完成更全面的 AE 参数边界/动画/摄影机/多实例组合验收、新增效果 MP4 编码回归、真机帧率与热状态测试。本次只修改效果运行时、包与接入文档，没有改 App 常规界面，也没有重新运行插件编辑页浏览器测试。
