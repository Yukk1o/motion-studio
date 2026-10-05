<h1 align="center">Motion Studio</h1>

<p align="center">Android 动效编辑器 · 图层动画 · 3D 摄影机</p>

<p align="center"><strong>AI 自主驱动的实验项目</strong></p>

<p align="center"><a href="#从图层到镜头">功能</a> · <a href="#技术结构">技术结构</a> · <a href="#构建与运行">快速开始</a> · <a href="LICENSE">MIT 许可证</a></p>

---

## 关于项目

Motion Studio 是一个由 AI 自主驱动开发的 Android 动效编辑器实验项目，目标是在手机上实现流畅的图层动画、3D 运镜与曲线编辑。

目前已实现关键帧、摄影机与父子级、自定义曲线、工程保存及视频导出，正在继续完善交互、稳定性与设备适配。

后端已提供视频与音频导入 API，包括动态视频帧、原声、波形和冻结读取；播放界面与带声音 MP4 导出仍需前端接入。接口与资源范围见 [音频 API](crates/aem-media/README.md) 和 [视频 API](crates/aem-media/VIDEO.md)。

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

属性面板的图层操作菜单提供“分离 XYZ”；分离后，轴选择器控制当前关键帧和曲线。新图层默认 2D，点击面板上的 2D/3D 可切换模式并保留各轴动画。

分离后的拖动区只编辑选中的轴：位置和目标点的 X 轴左右拖动，Y/Z 轴上下拖动。缩放分离后默认解除 XY 比例联动，手动开启时显示联动提示。分离操作可撤销；编辑独立轨道后，目前尚不支持直接合并回整体轨道，接口需求见 [XYZ 取消分离需求](docs/dimension-merge-requirements.md)。

在时间轴长按片段后横向拖动可移动开始时间，纵向拖动可调整图层层级；选中片段的两端可直接拖动裁剪。长按菜单提供精确移动、精确裁剪和在当前帧分割。移动与裁剪保留完整动画轨道，拖动支持单次撤销和取消恢复。

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

播放采样复用内存，素材纹理在上传后复用。预览提供清晰、流畅、省电与自动档位。静态图层的原有 GLES 导出路径无需应用层整帧读回；导入的视频按需解码并上传动态帧。系统与驱动内部拷贝尚未完整测量，性能结论保留具体运行条件。

## 构建与运行

当前 Android 工具链脚本面向 **Windows / PowerShell**。需要 Git、Python 3.12+、Rust/rustup；Windows 原生核心检查需要可用的 MSVC C++ 构建工具。Android 应用最低 API 29，默认构建 `arm64-v8a` 与 `x86_64`。

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

当前基线已通过 38 项 Rust/GPU 检查，并在 Android 模拟器 中验证编辑操作、工程保存、180 帧视频编码、八个准确索引解码帧/PNG 比对、取消后再次导出及六秒 Surface 播放。运行日志与生成产物保存在本地 `artifacts/`，不纳入版本管理。

无效预览时间导致的状态污染已修复，播放时钟也处理了早于播放开始的帧回调；短时资源释放及已保存工程的进程终止恢复检查通过。更长时间的资源趋势、输入到显示延迟、完整呈现归因，以及正式手机的性能/热状态仍需补齐。效果扩展在基础验收后推进。

供安装测试的预览包使用本地开发证书签名，关闭调试与故障注入：

```powershell
py tools/build_android.py --task assemblePreview
```

产物：`android/app/build/outputs/apk/preview/app-preview.apk`。

测试代码位于 `crates/*/tests/` 和 `android/app/src/androidTest/`。需要了解具体断言、测试负载与边界时，可直接检查这些实现。

## 一起改进这个实验

欢迎通过 Issue 提供可复现问题，通过独立分支和 Pull Request 提交改动。请附上触发步骤、环境和相关验证结果；说明使用了哪些 AI 工具，以及哪些结论经过实际检查。

项目采用 [MIT License](LICENSE)，与现有 Cargo 项目声明一致。第三方依赖保留各自的许可证。本项目提供实验代码与证据，不为尚未测量的设备或场景作质量保证。
