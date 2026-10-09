# Android runtime 模块拆分验证

基于主分支 `0ca9665285197c0d35c22ac3b5d4d1b57ef118e4`，分支 `codex/android-runtime-modules-20261008`。本次只调整代码边界，不引入颜色或蒙版工作树的协议／功能变化。

`runtime.rs` 从 2178 行降为 546 行，保留 Session 生命周期、线程归属检查、GPU/Surface 资源及 render/sample 核心。JNI 入口、参数转换、响应封装、命令／插件分发和 snapshot 移入 [runtime 模块说明](../../crates/aem-android/src/runtime/README.md) 中列出的模块。

已验证：

- 静态比较 51 个 `Java_com_motionstudio_editor_*` 入口及 `JNI_OnLoad`，名称／签名一致、无重复。7 个核心会话方法、状态 JSON、原有 envelope、失败 sentinel 与 diagnostics 条件逐项核对。
- Android ARM64 和 x86_64 的 diagnostics 原生编译通过；实际 `.so` 导出表各保留 52 个目标符号，两端清单一致。
- ARM64 不启用 diagnostics 的普通构建通过。
- APK、测试 APK、10 项 JVM 测试及 Android lint 通过。
- 普通 `1080×2400 @420dpi` 屏幕上，属性面板与合成 API 共 6 项 Android 检查通过，覆盖历史、保存／打包、嵌套渲染、视频／音频及冻结输出等实际 JNI 调用。

本机原始证据位于忽略的 `artifacts/`：`runtime-modules-build.log`、`runtime-modules-standard-build.log`、`runtime-modules-exported-symbols.json` 和 `runtime-modules-jni-tests.log`。子 agent 的静态比较报告位于该工作树忽略的 `target/runtime-modules-audit/report.json`。没有提交 APK、原始日志、用户工程或参考素材。
