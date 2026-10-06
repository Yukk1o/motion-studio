# 矢量与调整图层验收记录

## 2026-10-07：同步主分支后的补充验证

基线为 `main@55830ed`，包含空间效果边界（#29）、视频输入和导入顺序修复。
完整 Rust 工作区回归 **172 项通过，0 项失败，1 项既有大文件测试忽略**。
命令增加 `--features aem-android/diagnostics`，Windows GPU 使用 DX12。
日志在本地 `artifacts/pr24-host-validation.log`，不提交 Git。

补充 GPU 回归覆盖：两个调整层的顺序、旋转且带父级的作用区域、相交 3D 图层、
半透明 Alpha 保留、矢量几何及填充/描边动画随机寻帧、重复帧资源复用和删除后的释放。
发现并修复了矢量外扩源矩形叠加效果后重复应用原点的问题：调整后的效果偏移以
已外扩源矩形为基准，图层原锚点不变。回归用例在修复前 Alpha MAE 为 8.9675，
修复后满足不超过 0.1 的断言。

补充 Android 验收覆盖 6 个专项：既有矢量/调整层 2 项、空间效果 2 项、未编码对照
1 项、取消并重新导出 1 项。未编码对照在同一个 GLES 导出器内绘制 0、7、23、7 帧，
并切换纯矢量、矢量 Tint、单调整层、模糊调整层和完整叠加场景，共 9 组输出。
圆环孔径动画改变画面，返回同一帧的原生 PNG 一致。最大全图 RGB MAE **1.9615**、
前景 RGB MAE **2.3911**、Alpha MAE **0**（0–255 单位，三项阈值均为 3）。

专项发现 GLES 复用 MSAA 源时，半透明 Alpha 从 128 增长到 176、200，孔径变化后
留下旧图形；修复前第 23 帧 Alpha MAE 为 24.2364。新增首次矢量使用时的清除能力
探测：填满 4×4 MSAA 目标，再清空并读取一个测试像素。异常设备使用缓存的实顶点
透明三角形覆盖全部采样点，正常设备保留快速清空；解析后丢弃不再需要的 MSAA 内容。
首期探测只读取 4 字节，纯非矢量导出不探测，动画帧不增加应用读回。
导出报告单列 `graphicsCapabilityReadbackBytes`，不混入 `applicationFrameReadbacks`。
这与 SwiftShader 曾记录的[多采样清除问题](https://swiftshader.googlesource.com/SwiftShader.git/+/53e83aa8ecebea4913051dcdfd6923a3dd6fcccb%5E%21/)症状一致；
本次启用回退以运行时探测为准，不依据厂商名称推断。

ARM64/x86_64 原生构建、Android APK/test APK、10 项单元测试和 `lintDebug` 通过（0 错误、5 项告警）；软件 Vulkan
宿主回归经 GitHub CI 验证。新增 Android 专项进入 CI 选择器。Android CI 固定禁用
模拟器 Vulkan 和硬解仿真，与本地软件 GLES 验收条件一致；不代表 Vulkan 或 ARM 真机验收。
本地证据另存于 `artifacts/pr24-android-final-build.log`、`pr24-android-final-tests.log`、
`pr24-unencoded-report.json`，采集画面及日志不提交 Git。

以下 2026-10-06 记录保留当时的结果。

## 2026-10-06 初始验证

本分支不自动合并。前端界面由前端开发接入，接口见 `host-vector-adjustment.md`。

## 已完成检查

| 检查 | 结果及边界 |
| --- | --- |
| Rust 工作区运行回归 | `cargo test --workspace --offline --locked`，D3D12、单线程：159 项通过，1 项按既有配置跳过的大文件验收；包含 4 个新增核心用例和 4 个新增 GPU 用例 |
| Android 原生 | ARM64 / x86_64 release 编译通过；JNI 新能力、矢量采样和帧计划字段经原生构建检查 |
| Android Kotlin / 安装包 | `assembleDebug` / `assembleDebugAndroidTest` 通过；现有弃用和 SDK XML 告警保留 |
| GLES 全屏回归 | `fullscreenTriangleArithmeticCoversPbuffer` 通过；软件 GLES 3.0 上常量数组动态索引产生空白且 GL 错误码为 0，算术生成同坐标能输出红色；测试保存两种结果并要求算术路径通过 |
| 预览 / MP4 | `shapesAdjustmentAndEncodedOutputMatchPreview` 通过：全图 RGB MAE **1.1773**，前景 **1.2927**，前景通道样本 **9309**；阈值分别为 < 6 / < 8，另要求形状实际可见 |
| 既有效果 / 粒子导出 | Tint / Gaussian Blur / Curves / Wave Warp 冻结导出对照通过（全图 MAE 0.7869，前景 0.6100）；火花粒子冻结导出及容量失败清理通过（全图 0.9480，前景 5.1200，前景像素 2119）；共计 4 项 Android 用例通过 |
| 新增 Rust 文件格式 / Git diff | 新增 Rust 文件 `rustfmt --check` 通过，Git diff 检查通过；工作区全量格式检查存在既有格式差异，不在此分支批量重排 |
| 文件范围 | 独立分支 / 工作树，未包含研究资料、reference/refer、安装包、构建输出、模拟器镜像或日志 |

核心用例覆盖：25 项目录与合法默认值、参数越界拒绝、一次手势一次撤销、失败原子回滚、形状转换、局部时间关键帧、保存恢复、调整层创建、3D 禁用、旧工程升级和非法闭合路径。

GPU 用例覆盖：圆环镂空、开放路径描边、全部形状产生像素、重复渲染复用、调整层处理下方合成、上方及背景保留、透明度混合、跨图层模糊、矩形范围、v4 二进制资源表。既有工作区回归还覆盖效果、粒子、光效、YUV、素材时间实例和交叉平面。

Android 对照覆盖圆环、爱心、半透明填充与 Tint 调整层，采集首个编码帧；保存工程、采样状态、二进制帧计划、PNG、解码 PNG 和误差报告便于复核。该用例不能代表所有效果组合、未编码 Alpha 一致性或真实手机性能。

## 测试设备与修复

新建独立官方 AVD `motion-studio-api35-software`，Android 15 / API 35 x86_64，720×1280、2048 MiB RAM、4 vCPU，ADB `emulator-5554`；使用 WHPX 加速、ANGLE / SwiftShader 软件 GLES，并关闭模拟器 Vulkan。所有镜像与 AVD 数据保存在项目 E 盘工具目录。

默认 Windows GPU 探测曾在 `nvoglv64.dll` 崩溃，桌面验证改用 D3D12。MuMu 在此前测试时退出，未计为通过。新的模拟器 Vulkan / Lavapipe 试验返回 device lost，未计为通过；最终通过的 Android 对照使用 GLES 软件后端。软件编码器为 `c2.android.avc.encoder`，这些耗时不用于推断 ARM 手机性能。

修正了 GLES 3.0 无计算着色器时的设备能力申请，并将宿主全屏三角形、呈现三角形及精灵角点改成算术 / 条件表达式，避免常量数组动态索引的驱动兼容问题。坐标与渲染契约保持一致，效果包 ID、版本和参数不变。

缺失的 Windows C++ 链接工具和 SDK 库已恢复为 E 盘隔离工具，用于上述构建；无需改变项目工程格式以适配工具环境。

## 后续专项验收

真实 ARM 手机仍需记录预览、导出耗时、发热与内存。上述新增宿主用例已覆盖多调整层、
旋转 / 父级作用区、相交 3D 平面、矢量动画随机寻帧及资源释放；全部设备的资源预算与性能
不能由软件模拟器结果推断。

```powershell
$env:CARGO_TARGET_DIR='E:/Dev/aem/target/video-host'
$env:WGPU_BACKEND='dx12'
cargo test --workspace --offline --locked --config profile.test.debug=0 --config profile.test.incremental=false -j1 -- --test-threads=1

py -3.14 -X utf8 tools/build_android.py --abis arm64-v8a,x86_64 --rust-only --codegen-units 8
adb -s emulator-5554 shell am instrument -w -e class com.motionstudio.editor.VectorAdjustmentTest com.motionstudio.editor.test/androidx.test.runner.AndroidJUnitRunner
```

前端可根据接口文档开始接入；真机专项结果须继续补入本记录。
