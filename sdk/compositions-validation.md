# 子合成与预合成验收记录

日期：2026-10-07。分支：`codex/nested-compositions`，最新基线：`main@55830ed`。
接口和首期限制见 [前端接入契约](host-compositions.md)。

## 同步主分支后的补充验证

完整 Rust 工作区（含 `aem-android/diagnostics`）**177 项通过，0 项失败，1 项既有大文件测试忽略**。
日志在本地 `artifacts/pr28-host-validation.log`。主分支带入空间效果边界、视频输入能力和导入顺序修复。

修复合成设置仍限制 30/60 fps 的问题，与主分支统一为整数 **1–240 fps**。
修复递归音频时间使用 `48000 / fps` 整数单位导致的截断：时间沿引用链按有理数累计，
只在最终绝对采样边界向下取整。24、25、29、59、90、144、239 fps 的一秒时长均为
48,000 个采样点；不同片段起点和两级预合成保持音频声部时序。59/144 fps 的真实
44.1/48 kHz 混合素材在两级预合成前后、连续与随机采样时 PCM 逐样本一致。

补充 Android 验收：API 35 x86_64 / GLES 软件模拟器，**6 个专项用例通过**。
其中既有合成接口 3 项、主分支空间效果 2 项；新增未编码对照 1 项修正测试自身的
上下文调用顺序后单独重跑通过。两级合成使用 59/144 fps，重复引用含独立源偏移和
3D 变换，子层带 Shake；同一个 GLES 导出器按 0、17、58、115、17 帧绘制，
节点数量从 5 降至 3 再恢复为 5。重复帧原生 PNG 相同、动画确实改变画面。
5 帧中最大全图 RGB MAE **0.0793**、前景 RGB MAE **0.3562**、Alpha MAE **0**
（0–255 单位，验收阈值均为 3），未编码结果符合当前参考契约。

本地 x86_64 release、10 项单元测试、APK/test APK 及 `lintDebug` 通过（0 错误、5 项告警）；GitHub CI 对
同一代码完成 ARM64/x86_64 原生构建和 Android 构建检查。专项已加入 CI 测试选择器，
各分支存在相应用例时自动执行。源码不包含采集媒体、日志或安装包。
本地新增记录：`artifacts/pr28-android-build.log`、`pr28-android-tests.log`、
`pr28-android-parity.log`、`pr28-unencoded-report.json`。

以下保留原 `main@5dd9387` 的验收记录。

## 宿主回归

Windows x86_64、MSVC、wgpu DX12。执行：

```powershell
$env:WGPU_BACKEND='dx12'
cargo test --workspace --offline --locked --config profile.test.debug=0 --config profile.test.incremental=false -j1 -- --test-threads=1
```

结果：164 项通过，0 项失败。已有大文件用例
`imports_128_mib_sources_and_roundtrips_a_project_over_256_mib` 按原配置忽略，
此次未单独执行。

新增专项覆盖：

- 核心 9 项：旧工程迁移；预合成原子性、图层顺序和历史；拒绝循环、锁定、非连续
  选区与跨选区父级；同号图层的合成作用域；30/60 fps 和重复引用独立取帧；设置
  影响、调速与裁剪；从子合成保存和工程包往返；嵌套表达式时间与尺寸。
- 渲染 3 项：两级预合成前后在关键帧、正序、倒序和随机时间的 RGBA 平均误差
  不超过 3/255、最大误差不超过 4/255；引用层 3D 变换；输出纹理复用与释放；
  同号对象在视频/合成纹理之间切换；Tint 与后序 GPU 执行包。
- 音频：两级预合成、不同片段起点、44.1/48 kHz 和随机定位的 PCM 与原工程逐样本
  相同；子节点切换 30/60 fps、保留秒数策略不改变混音。

本地完整日志：仓库工作区的 `artifacts/nested-compositions-host-tests.log`。
日志及生成媒体作为本地证据保留，不提交 Git。

## Android 验收

API 35、x86_64 软件图形模拟器（swangle / GLES 3），禁用模拟器 Vulkan 和硬件视频
解码仿真。构建原生 release 库及 debug/test APK，启用 diagnostics：

```powershell
py -3.14 -X utf8 tools/build_android.py --abis x86_64 --target-dir E:/Dev/aem/target/nested-compositions --codegen-units 8 --task assembleDebug assembleDebugAndroidTest
adb -s emulator-5554 shell am instrument -w -r -e class com.motionstudio.editor.CompositionBackendApiTest com.motionstudio.editor.test/androidx.test.runner.AndroidJUnitRunner
```

`CompositionBackendApiTest` 包含三个端到端用例：

结果：3 项全部通过，0 项失败；JUnit 总耗时 5.351 秒。该耗时是测试执行时间，
不作为预览帧率或手机吞吐指标。

1. 列表、打开和面包屑；选择/时间轴恢复；循环和引用删除错误；合成设置预览、
   revision 防陈旧确认；从子合成保存、打包和重新打开。
2. Tint 和两级嵌套 PNG 的预合成前后 RGB 平均误差不超过 3/255；24 帧 MP4 解码后
   与原生 PNG 的 RGB 平均误差不超过 6/255；执行包扩容、3D 开关、撤销与重做。
3. 子合成内视频和音频导入；切换节点后拒绝错误上下文提交；30/60 fps 两级引用；
   WGPU Surface 连续与随机寻帧、PNG；冻结 PCM 非静音；带音轨的 24 帧 MP4。

该用例也检查 Vulkan 不可用时仅保留一个后端的窗口连接，再回退到 GLES。
完整构建与测试日志分别为本地 `artifacts/nested-compositions-android-build.log`、
`artifacts/nested-compositions-android-tests.log`。

## 交付边界

本次交付原生/Kotlin 接口、嵌套预览与导出，编辑器已接入合成管理、
面包屑、多选预合成、引用片段与设置确认页面。`CompositionFrontendTest` 检查
切换节点的选区、时间和缩放恢复，编辑与撤销的作用域，矢量编辑、剪贴板隔离和高帧率设置。
`CompositionParityTest` 增加两级嵌套矢量动画与调整图层的未经编码 GLES 对比。

首期自动预合成只接受连续堆叠的二维图层，使用完整源合成时长、移动所有属性；
特殊关联直接报错，不静默更改画面。引用层可以显式启用 3D；折叠变换、保留属性
模式和预渲染尚未实现。

ARM64 真机、Vulkan 真机、持续播放的帧率/发热/峰值内存尚未验证。
模拟器测试证明接口与输出行为，不代表手机性能。
