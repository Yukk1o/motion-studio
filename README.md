# Motion Studio

仅面向 Android 的图层动效与 3D 摄影机编辑器，目标技术栈为 Kotlin/Jetpack Compose、Rust、wgpu 和 MediaCodec。

Android 应用已接入 Rust/JNI 和真实 GPU 预览，可编辑图层与摄影机关键帧、导入图片和文字、保存工程、输出 PNG 与 MP4。属性编辑叠加在原界面上，预览和时间轴保持尺寸与位置。Motion Studio 已在 MuMu 上运行；24 项 Rust/GPU 检查和 10 项 Android 集成检查通过。完整 A1–A13 验收尚未完成，虚拟机结果不替代双真机持续性能验证。


本地工具链配置位于 `.tools/environment.json`。`tools/bootstrap_android.py` 负责项目内的独立工具链准备，不修改系统 PATH。

开发检查：

    cargo test --workspace --locked
    cargo run -p aem-render --bin render_probe -- artifacts/render-probe

Android 构建与设备检查（PowerShell，主工作树和独立工作树均可）：

    py tools/build_android.py --task assembleDebug assembleDebugAndroidTest
    py tools/validate_android.py --serial emulator-5554

默认构建 arm64-v8a 与 x86_64；仅测试 MuMu 时可加 `--abis x86_64`。设备检查使用独立测试工程，保存测试日志、视频、PNG、编码记录，并重新打开编辑器。APK 位于 `android/app/build/outputs/apk/debug/app-debug.apk`。工具链与构建产物不提交到 Git。

GPU 检查不会在缺少 GPU 时静默跳过；桌面 GPU 结果不替代 Android 性能验收。
