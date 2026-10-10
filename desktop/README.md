# Motion Studio Desktop

原生 Rust 桌面编辑器，与 Android 共用工程、动画、效果规划和渲染核心。

## 运行

```powershell
py tools/build_desktop.py --task build
./target/release/motion-studio.exe --project E:/MotionProjects/FirstProject
```

Windows 需要 MSVC 工具链。默认构建支持工程编辑和 GPU 预览；视频解码需要可选的 FFmpeg 开发库及 `--ffmpeg` 构建选项。

## 编辑工作区

- **项目**在左，**合成**在中，**效果控件**在右，**时间轴**在下。
- 打开工程时选择目录中的 `project.json`，或选择 `.aem` 工程包。也可将工程目录或文件拖入主窗口。
- 新建纯色图层；点击属性数值，输入后按 Enter 提交，Esc 取消。秒表按钮开启或关闭该属性动画。
- 点击时间尺跳帧；拖动图层条移动片段。一次拖动对应一次撤销。
- Ctrl+S 保存，Ctrl+Z 撤销，Ctrl+Shift+Z / Ctrl+Y 重做，Space 播放，左右方向键逐帧。
- Ctrl+滚轮缩放时间轴，Shift+滚轮横向滚动，滚轮纵向滚动图层。
- 拖动分隔线调整面板大小；拖动标签到其他面板的边缘重新停靠，放在中央合并标签。
- 标签右侧的箭头将面板拆成原生浮窗；浮窗的“停靠”按钮或关闭按钮将面板放回主窗口。合成面板也能浮动。
- “默认”按钮恢复工作区。布局与语言保存在应用数据目录中的 `desktop-layout.json`，独立于工程文件。

## 中英文

工具栏可切换中文 / English，窗口菜单也提供语言选项。首启参考系统语言，之后使用已保存的偏好。

```powershell
./target/release/motion-studio.exe --locale zh
./target/release/motion-studio.exe --locale en
```

中文使用系统 CJK 字体，字体集合保留正确的 face index。Linux 可安装 Noto Sans CJK。工程名称、用户输入的图层名称和核心 API 字段保持原始数据。

## MCP 与内置 agent 接口

[MCP 接入说明](MCP.md)包含可直接配置的 stdio 启动方式、工具参数和共享界面模式。

内置 agent 的扩展点是 `mcp::ToolRouter` 和同一份 `Engine` 命令队列。编辑支持版本检查、原子批次和撤销；目前尚未接入模型供应商或聊天面板。

[节点创作方案](NODE-AUTHORING.md)定义桌面节点图生成 WGSL、导出手机 `.msfx` 的下一阶段；节点编辑器尚未实现。

## 验证

```powershell
cargo test --locked -p aem-desktop -p aem-ui -p aem-host -p aem-desktop-media
./target/debug/motion-studio.exe --smoke artifacts/desktop/smoke-en.json --locale en
./target/debug/motion-studio.exe --smoke artifacts/desktop/smoke-zh.json --locale zh
```

隐藏窗口验收创建动画图层、保存并重开工程，实际呈现 GPU 预览，拆出属性及合成窗口、调整窗口大小，再重新停靠，输出报告和真实界面截图。隐藏验收不会改写用户的布局偏好。预览使用共享 GPU 设备，不读回画面；PNG 验收截图单独执行读回。

目前界面完成的是基础编辑闭环。完整曲线编辑器、素材导入界面、文字排版界面和节点编辑器仍待实现；MP4 编码器也仍未完成；当前播放是画面预览，桌面音频输出尚未接入。共享后端已有的能力不等于所有桌面面板都已提供。
