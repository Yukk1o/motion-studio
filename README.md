# Motion Studio

**面向 Android 与桌面的开源动效编辑器。**

用图层组织画面，用关键帧和曲线控制运动，用 3D 摄影机连接空间。Motion Studio 以 Rust 和 GPU 合成为基础，提供 Android 编辑器、原生桌面工作区，以及共享的工程与编辑后端。

这是一个由 **AI 自主驱动开发的实验项目**，正在持续完善创作体验与平台支持。

[动画与曲线](#动画与曲线) · [3D 摄影机](#3d-摄影机) · [效果与插件](#效果与插件) · [桌面与自动化](#桌面与自动化) · [开始使用](#开始使用) · [当前进展](#当前进展) · [参与开发](#参与开发)

## 为运动画面而做

Motion Studio 用于图层动画、动态标题、镜头运动和画面合成。你可以把图片、形状、文字、视频和其他合成放进工程，给属性添加关键帧，组合效果，再预览和导出作品。

Android 端以触屏操作为主，属性编辑保留预览与时间轴。桌面端提供项目、合成、效果控件和时间轴工作区，面板可以调整、停靠或拆成浮窗。两个平台共享工程数据与编辑后端，界面能力按各自的开发进度开放。

## 动画与曲线

- **图层动画**：位置、旋转、缩放、透明度、锚点，以及效果参数的关键帧。
- **独立维度**：由用户主动分离 XYZ，每轴拥有自己的关键帧时间和区间曲线。
- **曲线编辑**：二次与三次贝塞尔、弹性曲线，进度与速度视图，曲线复制和粘贴。
- **时间轴**：片段移动、非破坏裁剪、分割、层级调整与图层复制。
- **编辑历史**：撤销、重做与连续手势事务；批量修改可以作为一次操作提交。
- **矢量描线**：修剪路径支持开始、结束、偏移和多路径模式；虚线描边支持三组线段与间隔，参数可添加关键帧和缓动曲线。Android 沿用矢量属性面板，详见[路径操作 API](engineering/host-api/vector-path-operations.md)。
- **表达式**：属性和连续效果参数支持 JavaScript 数值表达式，包含常用动画函数、随机种子与分量表达式。

表达式目前提供数值与动画函数子集，跨图层属性引用仍待实现。XYZ 分离后的轨道可以独立编辑，合并回整体轨道尚未开放。

图层支持 15 种混合模式、线性与 sRGB 混合空间，以及 Alpha、亮度和对应反转的轨道遮罩。遮罩读取来源图层的效果、蒙版、不透明度与摄影机投影；Android 提供混合面板，桌面可通过共享编辑接口使用这些能力。接口与约束见[图层混合与轨道遮罩](engineering/host-api/layer-compositing.md)。

## 3D 摄影机

摄影机由用户主动创建。可以直接调整位置与目标点，也可以通过空对象和父子级搭建运镜控制结构。

- 图层默认使用 2D，由用户选择开启 3D。
- 摄影机支持位置与环绕模式，以及推进、平移和环绕操作。
- 摄影机与普通图层都能绑定父级，支持多级关系与循环检查。
- 提供独立观察、顶视与侧视，便于安排空间中的图层。
- 交叉平面与透明纹理使用几何合成，支持对应的预览和导出路径。

## 效果与插件

统一内置效果包提供 **95 项效果**，覆盖调色、模糊、扭曲、渐变、噪声、字符画、投影和材质纹理；场景效果包另提供 **6 项粒子与镜头效果**。

效果可以堆叠、排序、启停和设置参数动画。置换贴图支持引用其他图层的原始像素或效果后像素，颜色曲线支持五通道编辑。字体后端支持系统字体与用户导入的 TTF、OTF、TTC，文字和 ASCII 效果共用栅格缓存。

第三方效果以 `.msfx` 打包，导入后即可测试，无需重新构建应用。[插件 SDK](sdk/README.md) 提供 WGSL 契约、参数与原生编辑器协议、打包工具，以及可安装的 `motion-studio-plugin` skill。

效果采用独立实现，部分行为与参数仍有近似或未支持项；具体范围见[效果与字体 API](engineering/host-api/official-motion-effects.md)。

## 素材、工程与输出

媒体后端支持 MP4、MOV、MKV、WebM 中的 H.264、H.265、VP8、VP9，以及 AAC、MP3、FLAC、ALAC、Vorbis、Opus、AIFF 和多种 WAV 位深。

视频源支持最高 4K、240 fps、8 位 SDR 和任意画幅比例，仍受边长、像素和设备解码能力约束。音频源支持 8–192 kHz；工程帧率与素材帧率独立，工程可设置为 1–240 的整数帧率。

Android 端提供视频原声、波形、音量、静音和带音频的 H.264 MP4 导出。工程支持本地保存、工程库、指定帧 PNG，以及包含素材的 `.msproj` 工程包；旧 `.aem` 工程包保持读取兼容。

[媒体格式](crates/motion-media/FORMATS.md) · [音频 API](crates/motion-media/README.md) · [视频 API](crates/motion-media/VIDEO.md)

## 桌面与自动化

桌面工作区采用原生 Rust 窗口，支持中文与英文、面板缩放与停靠、标签组合、浮窗和布局保存。当前界面已提供工程打开与保存、GPU 预览、时间轴定位、片段移动和基础属性编辑。

桌面程序也提供 MCP：

- `--mcp`：无窗口工程会话，供脚本或 agent 操作。
- `--mcp-ui`：打开编辑器，用户与 MCP 客户端共享工程、播放头与撤销历史。
- `--mcp-read-only`：只读取状态。

编辑接口支持 revision 检查、原子命令批次、撤销和保存。内置 agent 的接口已预留，模型供应商与聊天界面尚未接入。

[桌面使用说明](desktop/README.md) · [MCP 配置与工具](desktop/MCP.md)

## 开始使用

### Android

从[预览版](https://github.com/Yukk1o/motion-studio/releases/tag/preview)获取 APK，或在[发布页](https://github.com/Yukk1o/motion-studio/releases)选择版本。预览包面向体验与问题反馈，正式版本以对应发布说明为准。

下载附件中的 `SHA256SUMS` 和 `build-info.json` 可用于核对文件与构建来源。

### 从源码运行桌面端

准备 Rust 1.97.1 和 Python；Windows 还需要 MSVC 工具链。

```powershell
git clone https://github.com/Yukk1o/motion-studio.git
cd motion-studio
py tools/build_desktop.py --task build
./target/release/motion-studio.exe --locale zh
```

默认构建提供编辑与预览。桌面视频解码需要 FFmpeg 开发库，并在构建时添加 `--ffmpeg`；安装方式和运行参数见[桌面说明](desktop/README.md)。

### 从源码构建 Android

```powershell
py tools/bootstrap_android.py
py tools/build_android.py --task assembleDebug
```

工具链放在项目的 `.tools` 目录；APK 输出到 `android/app/build/outputs/apk/debug/app-debug.apk`。只构建模拟器版本时可加 `--abis x86_64`。

## 当前进展

| 部分 | 当前范围 |
| --- | --- |
| Android 编辑器 | 图层动画、曲线、摄影机、效果、音视频导入与导出；持续改进交互和设备适配 |
| 桌面编辑器 | 基础编辑流程、自由布局、浮窗、中英文和 MCP；完整创作面板仍在完善 |
| 桌面音频与视频输出 | 视频解码后端已提供；桌面音频播放与 MP4 编码仍在开发 |
| 节点创作 | 规划桌面节点图生成 WGSL 并导出 `.msfx`，手机执行效果包；主分支尚未提供节点编辑器 |
| 兼容性与性能 | 持续进行 GPU、模拟器与真机验证；设备支持范围以实际记录为准 |

本项目已有可运行的编辑与渲染流程，也有尚未完成的能力。共享后端支持某项功能，并不表示两个平台的所有面板都已接入。节点路线见[节点创作方案](desktop/NODE-AUTHORING.md)。

## 技术结构

| Crate / 界面 | 职责 |
| --- | --- |
| `motion-model` | 稳定工程数据、序列化、校验、曲线与父子级矩阵求值 |
| `motion-core` | 命令执行、撤销、编辑事务和 JavaScript 表达式引擎 |
| `motion-effects` | 效果包、WGSL 与参数协议 |
| `motion-render` | 场景采样、GPU 合成、预览与 PNG 输出 |
| `motion-media` | 素材导入、缓存、音频混音与媒体任务 |
| `motion-host` | 聚合编辑、渲染、效果与媒体，供平台共享会话 |
| Android | Kotlin、Jetpack Compose、SurfaceView、JNI、MediaCodec |
| Desktop | Rust、winit、wgpu、原生面板与窗口 |

render 和 media 的生产依赖通过 model 与编辑内核解耦。宿主显式提供 `FrameEvaluator`、`EditSink` 和冻结打包接口；JS 引擎与文件发布实现留在 core。model 保留现有效果 schema 与 ZIP 错误类型依赖。

Rust 调用方从 `motion-render` 导入 `Scene`，采样时传入求值器；直接调用工程表达式方法时需导入 `motion_core::ProjectExpressions`。工程格式、命令 JSON 与平台协议保持原定义。

## 参与开发

Motion Studio 由 AI 编程代理推进实现、测试与迭代，人类提供产品方向、反馈和代码审阅。欢迎通过 [Issue](https://github.com/Yukk1o/motion-studio/issues) 报告问题，或通过独立分支与 Pull Request 提交改进。

反馈时请附上复现步骤、平台、应用版本和相关素材范围。提交改动时说明解决的问题、验证方式，以及 AI 工具的参与情况。

基础检查：

```powershell
cargo test --workspace --locked -- --test-threads=1
cargo run -p motion-render --example render_probe -- artifacts/render-probe
```

CI 覆盖 Rust/GPU、桌面平台构建、libav、Android 构建与模拟器交互。测试与报告保存在 Actions artifacts；真机持续性能、热状态和设备兼容性仍单独验收。

Android 设备验证：

```powershell
py tools/build_android.py --task assembleDebug assembleDebugAndroidTest
py tools/validate_android.py --serial <adb-device-serial>
```

测试代码位于 `crates/*/tests/` 和 `android/app/src/androidTest/`。更具体的源码接入文档见 [host API](engineering/host-api/)，效果开发见 [SDK](sdk/README.md)。

## 许可证

Motion Studio 采用 [MIT License](LICENSE)。第三方依赖、字体与其他素材保留各自的许可证。
