# AEM

仅面向 Android 的图层动效与 3D 摄影机编辑器，目标技术栈为 Kotlin/Jetpack Compose、Rust、wgpu 和 MediaCodec。

当前已实现 Rust 动画/摄影机核心和 wgpu 平面合成，并通过 15 项核心测试及实际 GPU 像素测试。Android 应用与 JNI 尚未接入，当前不能安装运行。完整交付目标仍是 A1–A13 验收。


本地工具链配置位于 `.tools/environment.json`。`tools/bootstrap_android.py` 负责项目内的独立工具链准备，不修改系统 PATH。

开发检查：

    cargo test --workspace --locked
    cargo run -p aem-render --bin render_probe -- artifacts/render-probe

GPU 检查不会在缺少 GPU 时静默跳过；桌面 GPU 结果不替代 Android 性能验收。
