<h1 align="center">Motion Studio</h1>

<p align="center">Android 动效编辑器 · 图层动画 · 3D 摄影机</p>

<p align="center"><strong>AI 自主驱动的实验项目</strong></p>

<p align="center"><a href="#从图层到镜头">功能</a> · <a href="#技术结构">技术结构</a> · <a href="#构建与运行">快速开始</a> · <a href="LICENSE">MIT 许可证</a></p>

---

## 关于项目

Motion Studio 是一个由 AI 自主驱动开发的 Android 动效编辑器实验项目，目标是在手机上实现流畅的图层动画、3D 运镜与曲线编辑。

目前已实现关键帧、摄影机与父子级、自定义曲线、工程保存及视频导出，正在继续完善交互、稳定性与设备适配。

编辑界面支持视频与音频导入、视频原声、真实波形、音量和静音；播放使用混音采样时钟，MP4 导出同时编码画面和声音。效果属性使用内嵌面板，支持效果链、参数动画、五通道颜色曲线和效果包管理。接口与资源范围见 [音频 API](crates/aem-media/README.md) 和 [视频 API](crates/aem-media/VIDEO.md)。

核心效果包现有 59 项，包括动态拼贴、光学补偿、球面化与 Alpha 工具；场景效果包提供 6 项粒子和镜头效果。参数范围和未支持选项按实例绑定的精确包显示；全部效果仍为近似实现，不能将渲染一致性检查视为视觉兼容性已完整验收。

第三方效果开发见 [插件 SDK](sdk/README.md)，提供可安装的 `motion-studio-plugin` skill、WGSL/原生 UI 契约与独立 `.msfx` 打包器。普通效果可直接打包、导入测试，无需重新构建 App。

属性与连续效果参数支持 JavaScript 数值表达式。属性菜单可打开表达式工作区，支持启停、随机种子、整体或 XYZ 分量、保存草稿、移除和撤销；编译失败保留原值和代码。当前提供数值和动画函数子集，不支持跨图层属性引用。粒子与镜头的专用编辑器也在效果工作区打开，支持参数、元件、图层引用与拖动事务；编辑时保留预览和时间轴。

合成外侧使用独立灰色背景和细边线。主页设置集中管理效果包和布局；“调整布局”允许改变预览、面板和时间轴比例，并保存横竖屏各自的布局，抓手只在调整模式显示。工程主页支持搜索、继续编辑和自定义尺寸、帧率及时长；图层可多选并批量编辑，数值滑轮和时间轴松手后逐渐减速，再次触摸即停止。

图层底栏提供变换、效果及视频原声分类，超出可用宽度时左右滑动。透明度与位置、旋转、缩放一起归入变换。播放控制栏可在当前播放头切割选中图层；更多菜单支持同一工程内复制和粘贴单层或多层，保留动画、效果、表达式及选中图层之间的父子关系，一次撤销整个粘贴。立即生成副本的操作命名为“创建副本”。

## 桌面编辑器

`feat/desktop` 正在加入原生 Rust 桌面端：可调整的面板与浮窗、基础编辑与保存、中英文界面，以及与窗口共享会话的 MCP 工具。运行方式与当前界面范围见 [桌面端说明](desktop/README.md)，自动化接入见 [MCP](desktop/MCP.md)。[节点创作与手机效果包导出](desktop/NODE-AUTHORING.md)目前是下一阶段的设计方案。

## 从图层到镜头

| 图层与动画 | 摄影机与空间 |
| --- | --- |
| 图片、形状与缓存文字 | 用户主动创建摄影机 |
| 位置、旋转、缩放、透明度与锚点 | 空对象与通用多级父子关系 |
| 属性关键帧、时间轴、撤销与重做 | 推进、平移与环绕 |
| 显式 XYZ 分离、每轴关键帧与曲线 | 2D/3D 图层切换、交叉平面点选 |
| 长按移动片段与层级、边缘裁剪、分割 | 交叉平面和透明纹理的几何导出 |
| 二次/三次贝塞尔、弹性曲线 | 独立观察、顶视与侧视 |
| 进度/速度图、参数编辑、曲线复制 | 父级循环检查与姿态保持 |

工程可在本地自动保存，支持工程库、完整工程包导入/导出、指定帧 PNG 和 H.264 MP4。

媒体后端支持 MP4 / MOV / MKV / WebM 的 H.264、H.265、VP8、VP9 导入，以及 FLAC、ALAC、Vorbis、Opus、AAC、MP3 和多种 WAV 位深。音频源采样率为 8–192 kHz；视频源支持 4K、最高 240 fps、8 位 SDR，不限预设画幅比例，仍受像素与边长预算约束，具体组合以设备探测结果为准。工程帧率可设为 1–240 的整数，源帧率与工程设置独立。设备解码器查询与格式范围见 [媒体格式 API](crates/aem-media/FORMATS.md)。

属性面板的图层操作菜单提供“分离 XYZ”；分离后，轴选择器控制当前关键帧和曲线。新图层默认 2D，点击面板上的 2D/3D 可切换模式并保留各轴动画。

分离后的拖动区只编辑选中的轴：位置和目标点的 X 轴左右拖动，Y/Z 轴上下拖动。缩放分离后默认解除 XY 比例联动，手动开启时显示联动提示。分离操作可撤销；编辑独立轨道后，目前尚不支持直接合并回整体轨道。

在时间轴长按片段后横向拖动可移动开始时间，纵向拖动可调整图层层级；选中片段的两端可直接拖动裁剪。长按菜单提供精确移动、精确裁剪和在当前帧分割。移动与裁剪保留完整动画轨道，拖动支持单次撤销和取消恢复。

关键帧需要先长按再拖动，进入移动时提供触觉反馈；普通滑动用于浏览时间轴，点按定位与长按操作菜单保留。关键帧移动支持一次撤销，取消拖动保留原位置。

曲线、层级、求值与渲染采用独立实现。

## 技术结构

| 层 | 技术与职责 |
| --- | --- |
| 界面 | Kotlin、Jetpack Compose、SurfaceView |
| 动画核心 | Rust：工程模型、关键帧、曲线与摄影机 |
| GPU 预览 | wgpu：合成、呈现与 PNG |
| 平台边界 | JNI |
| 媒体导入 | Rust 工程事务与磁盘缓存，Android MediaExtractor/MediaCodec 视频解码，确定性 PCM 混音 |
| 视频输出 | 冻结工程逐帧求值，EGL/GLES 写入 MediaCodec 输入 Surface |

播放采样复用内存，素材纹理在上传后复用。预览提供清晰、流畅、省电与自动档位。静态图层的原有 GLES 导出路径无需应用层整帧读回；导入的视频使用有界 PTS 预取缓存，默认 SDR YUV420 由 GPU 转色，输入纹理复用。接入约定与测量范围见 [视频预览后端](engineering/host-api/host-video-preview.md)。系统与驱动内部拷贝尚未完整测量，性能结论保留具体运行条件。

## 构建与运行

当前 Android 工具链脚本面向 **Windows / PowerShell**。需要 Git、Python 3.12+、Rust/rustup；Windows 原生核心检查需要可用的 MSVC C++ 构建工具。Android 应用最低 API 29，默认构建 `arm64-v8a` 与 `x86_64`。

GitHub Actions 使用 Ubuntu 24.04、Rust 1.97.1、JDK 21、Gradle 8.11.1、Android 35 与 NDK 27.0.12077973。构建脚本也接受 Linux 环境中的 `JAVA_HOME`、`ANDROID_HOME`、`ANDROID_NDK_HOME` 和 PATH 中的 Gradle。

**1. 准备独立工具链**

```powershell
py tools/bootstrap_android.py
```

脚本将 JDK、Gradle、Android SDK/NDK 放在 `.tools` 中，不修改系统 PATH。工具链及生成产物不提交到 Git。

**2. 构建普通开发 APK**

```powershell
py tools/build_android.py --task assembleDebug
```

产物：`android/app/build/outputs/apk/debug/app-debug.apk`。仅构建模拟器版本可加 `--abis x86_64`。

本机原生链接内存不足时可加 `--codegen-units 8`，仅覆盖项目 crate 的本次 release 构建参数，保持优化级别和仓库发布配置。

## 让结果能够复现

<details>
<summary><strong>核心与 GPU 检查</strong></summary>

```powershell
cargo test --workspace --locked
cargo run -p aem-render --bin render_probe -- artifacts/render-probe
```

GPU 检查不会在缺少 GPU 时静默跳过。

</details>

<details>
<summary><strong>Android 功能验证</strong></summary>

```powershell
py tools/build_android.py --task assembleDebug assembleDebugAndroidTest
py tools/validate_android.py --serial <adb-device-serial>
```

验证包启用测试所需的 GPU 故障注入，普通开发 APK 不启用。测试使用独立工程，保存日志、PNG、MP4 与 JSON 记录；请使用专门的验收设备或虚拟机。

</details>

<details>
<summary><strong>非 Debuggable 性能测量</strong></summary>

```powershell
py tools/build_android.py --task assembleBenchmark assembleBenchmarkAndroidTest
py tools/validate_performance.py --serial <adb-device-serial> --seconds 600 --output <report-directory>
```

该命令生成指定环境的测量结果。桌面 GPU、模拟器与手机性能分别记录，不互相替代。

</details>

## 已有验证与待完成事项

当前整合版本已通过 151 项 Rust/GPU 检查，并在 Android 模拟器验证表达式工作区、真实插件页面操作、媒体格式导入、视频特效、几何透明纹理导出、取消后再次导出及六秒 Surface 播放。新增七项图像效果已逐项比较编码帧与 PNG，动态拼贴包含动画输出边界检查；媒体格式检查还包含 19 个素材的画面和音频对照。运行日志与生成产物保存在本地 `artifacts/`，不纳入版本管理。另有一项超大工程的 release 专项测试未运行。

无效预览时间导致的状态污染已修复，播放时钟也处理了早于播放开始的帧回调；编码输出由独立线程读取，避免输入 Surface 在输出队列积压时阻塞整个导出。短时资源释放及已保存工程的进程终止恢复检查通过。更长时间的资源趋势、输入到显示延迟、完整呈现归因，以及正式手机的性能/热状态仍需补齐。

布局检查可分别运行效果面板和表达式/专用编辑器两组，覆盖窄屏、横屏与大字体，完成后恢复设备显示设置：

```powershell
py tools/validate_layout_profiles.py --serial <adb-device-serial> --suite integrated --output artifacts/layout-integrated
py tools/validate_layout_profiles.py --serial <adb-device-serial> --suite workspaces --output artifacts/layout-workspaces
```

供安装测试的预览包使用本地开发证书签名，关闭调试与故障注入：

```powershell
py tools/build_android.py --task assemblePreview
```

产物：`android/app/build/outputs/apk/preview/app-preview.apk`。

### 持续集成与自动发布

Pull Request 和 main 更新会运行 Rust/GPU 测试、工具与 SVG 检查、Android 双架构构建、JVM 测试、Lint 和 Android 35 模拟器交互检查。运行日志与测试报告作为 Actions artifacts 保存。模拟器套件明确选择稳定的编辑交互用例；完整视频导入、导出与设备性能验收仍使用上面的专用命令。

main 的检查全部通过后自动更新 [预览版](https://github.com/Yukk1o/motion-studio/releases/tag/preview)。推送 `vMAJOR.MINOR.PATCH` 标签后自动发布正式版，带 `-rc.1` 等后缀的标签发布预发行版。标签必须指向需要发布的代码；发布脚本核对构建提交和校验和，已发布的正式版本不能被替换。Android versionCode 使用同一工作流的递增运行编号。

下载包同时包含 arm64-v8a 与 x86_64，发布附件提供 APK、`SHA256SUMS` 和 `build-info.json`。发布前检查签名、包名、版本和非调试状态；预览/正式包不启用 GPU 故障注入。构建或交互检查失败时不会发布。手动运行工作流仅执行 CI。

签名材料从仓库 Actions secrets 读取：预览使用 `PREVIEW_KEYSTORE_BASE64`、`PREVIEW_KEYSTORE_PASSWORD`、`PREVIEW_KEY_ALIAS`、`PREVIEW_KEY_PASSWORD`；正式版使用对应的 `RELEASE_` 前缀。密钥不会进入 Git、日志或构建附件，临时 keystore 在构建后删除。预览沿用开发证书以保持现有预览包的覆盖安装，正式版使用独立发布证书；两种证书的安装包不能直接互相覆盖。维护者必须在仓库以外备份正式签名文件和密码，后续版本保持同一证书。

测试代码位于 `crates/*/tests/` 和 `android/app/src/androidTest/`。需要了解具体断言、测试负载与边界时，可直接检查这些实现。

## 一起改进这个实验

欢迎通过 Issue 提供可复现问题，通过独立分支和 Pull Request 提交改动。请附上触发步骤、环境和相关验证结果；说明使用了哪些 AI 工具，以及哪些结论经过实际检查。

项目采用 [MIT License](LICENSE)，与现有 Cargo 项目声明一致。第三方依赖保留各自的许可证。本项目提供实验代码与证据，不为尚未测量的设备或场景作质量保证。
