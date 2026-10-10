# 颜色面板与曲线验证记录

基于主分支 `0ca9665285197c0d35c22ac3b5d4d1b57ef118e4`，独立工作树／分支 `codex/color-editor-contract-20261008`。用户参考图用于色板、色环及面板内编辑布局；保留 Motion Studio 现有主题。

## 已完成检查

| 检查 | 结果与覆盖 |
|---|---|
| Rust 工作区 | 198 通过，0 失败，1 忽略；含自然三次样条的已知解、旧线性曲线、五通道独立性、冻结 LUT 无损编辑、合法性与撤销／重做 |
| Android JVM | 14 通过，0 失败；含 HEX/RGBA、透明度精度及 HSV 往返 |
| Android 构建 | arm64-v8a、x86_64 原生库与 APK 均编译成功；unit 和 lint 通过 |
| 原生接口／用户色板 | 4 项通过；五通道图形接口、非法点拒绝、真实 wgpu PNG Alpha／RGB 输出、GLES Alpha 与 LUT、一致的收藏顺序／名称／Alpha、删除和空常用列表 |
| 最新界面交互 | 6 项通过；颜色预览／确认／取消／单次撤销，RGB 总览与 Alpha 拖动，收藏和常用色管理与重开恢复，吸管保留 Alpha、横屏色环，播放／第二次手势的独立历史，背景颜色关闭状态恢复 |
| 工具检查 | 34 个图标校验通过；4 项 CI 工具测试通过；Git diff 无空白错误 |
| 只读复核 | 使用一位子 agent，修复其发现的颜色事务与时间轴／播放冲突及背景面板恢复问题 |

界面检查使用 Android API 35 独立模拟器，普通手机屏幕 `1080×2400 @420 dpi`，工程 `1080×1920 / 30 fps`，覆盖横竖屏。这里的结果证明功能行为，不作为 VIVO 真机帧率或功耗测量。

双端输出用例：纯色 `(0.25,0.5,0.75,1)` 配合 Alpha 映射 `1→0.5`。wgpu PNG 中心像素 Alpha 约 128，直通 RGB 约 64/128/191（容差 3）；GLES 未编码缓冲和冻结 LUT 的 Alpha 约 128（容差 1）。该用例未扩大为全部效果／颜色空间的逐像素一致声明。

颜色面板进入普通属性时保留原属性边界、预览及时间轴几何；进入效果／矢量时替换其专用面板。吸管以显示画面的像素为基准，编辑 Alpha 不重新量化 RGB。

## 可复现入口

- Rust：`cargo test --workspace --locked --features motion-android/diagnostics -- --test-threads=1`。
- Android 构建：`python tools/build_android.py --abis arm64-v8a,x86_64 --effects-acceptance --task assembleDebug assembleDebugAndroidTest testDebugUnitTest lintDebug`。
- 交互：`ColorEditingTest`，共 6 项；底层契约与持久化：`ColorCurvesApiTest`、`ColorBookmarksTest`，共 4 项。三组已接入 `tools/ci_android_ui.py`。
- 契约与迁移：[host-color-curves.md](../host-api/host-color-curves.md)。

本机原始日志、截图和 APK 位于忽略的 `artifacts/`。最新界面结果为 `color-editor-validated-normal-ui-tests.log`，构建为 `color-editor-palette-release-build.log`，Rust 为 `color-editor-workspace-tests.log`。参考图片、用户工程／素材和 APK 不提交到 Git。

自然三次样条尚未采集 AE 对照帧，不能计入逐像素还原的已验收效果数量。新工程格式为 8；缺少插值字段的旧对象保持线性，新对象显式记录各通道插值模式。
