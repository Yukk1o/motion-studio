# 插件专用编辑器1.1.0

本次交付粒子与镜头光效的插件页面、现有协议的JS传输层、当前帧基础变换采样及版本保留。App的打开按钮、WebView窗口、来源隔离和工作线程转发由App前端接入。页面自身可用，不需要App前端再实现粒子参数表或镜头元件编辑器。

场景包1.1.0 SHA-256：`c8c312ed6239fe65760e7c41811a9c0a1db2a548b8abe142230377bb57830bc1`。保留的1.0.0为 `ffd6d7ddbff861072da76dd7f84f072a7b01b187f8488e59e88e4bc468c20724`；旧实例仍加载旧页面。六个生成器的WGSL、参数、场景默认配置和混合模式保持一致，版本变化用于新增UI文件。核心图像包1.2.0及既有SDK 1版本未改动。

## 编辑行为

- 粒子：发射/运动/外观/变换分组、种子、RGBA拾色及通道输入、可动画参数开关、粒子容量反馈、三种密度/尺寸起点。图层变换按当前帧采样，使用既有transform消息，仅修改所属图层。
- 镜头：手动位置或图层/Null绑定、缺失引用修复、Alpha遮挡、总强度与尺寸；稳定ID元件列表，增删/复制/启停/上下移动，形状、尺寸、强度、偏移、星芒、色差与RGBA颜色。
- 数值遵守精确包硬范围；常用滑块范围和硬范围分开。非法值不发送，后端错误不覆盖工程。元件尺寸支持小于1px的合法正值，不取整；菜单按min+序号提交。
- 修改按前一条返回的revision串行提交，向量和SceneSettings从最新状态合并。拖动begin/set/commit，一次撤销；Esc/pointercancel回滚。预设、恢复默认及拾色为一个事务。动画恢复默认只修改当前帧，不删除其他键。
- 锁定、断开及陈旧revision有明确反馈；错误编辑不自动重放。预览单请求在途、150ms合并、保留比例、丢弃旧revision/frame及关闭窗口后的结果。页面未增加播放/寻帧/导出等未授权消息。

## 可复现测试

Rust工作区108项通过，新增的两项检查分别验证原始场景包保留及当前合成帧到图层局部变换键的采样。Clippy通过，保留已有警告。生成器重复执行得到相同包hash。历史1.0.0的Android/视频验收仍在 [validation.md](validation.md)，不能把旧报告当作1.1.0页面验收。

最终ARM64/x86_64原生库及debug/AndroidTest APK构建成功。默认release构建的两任务及单任务尝试均因本机LLVM内存分配失败中断，保留日志；最终验证仅在本地构建命令中将四个项目crate的release codegen-units分为8，使用单任务，仍保持优化级别3及既有链接设置。没有修改仓库发布配置或系统内存配置。最终构建日志为 `artifacts/plugin-editor-android-low-memory-build.log`，默认构建中断记录为 `plugin-editor-android-build.log`、`plugin-editor-android-final-build.log`；不能将最终结果表述为默认配置已在该受限环境通过。

该最终APK在MuMu QA实例（Android 15、x86_64、独立验收App ID）运行 `SceneEffectsTest#editorAssetsScopedEditsCancelAndGpuPreview` 与 `#sixGeneratorsMatchGlesWithoutVideoEncoding`，2项通过，耗时1.874秒。六种生成器最大RGB MAE为0.190917/255，Alpha为0；测试报告的包hash与上述1.1.0一致。安装后的App/Test APK hash与本地构建一致，来源为代码提交 `98d127cc413418f2299c6d0adb3160e5c508a6a0`，后续提交只更新交付记录：App为 `ba260bbcb26a935812a90e1c12a6813ac14fd1f41b6f24b5a07c7ac343ca14de`，Test为 `05d14fbff259058715fc20a36c00a9b9d3e97c64bd8f4210a72a5736c5c6d5d1`。原始日志在 `artifacts/plugin-editor-android-tests.log`，设备JSON与来源在 `artifacts/plugin-editor-browser/android-gles-report.json`、`provenance.json`。此次没有重跑MP4、整组Android回归或物理设备性能测试；这两项也不包含Android WebView窗口挂载。

浏览器测试用打包页面连接真实Rust PluginEditorSession和wgpu渲染器；预览是实际GPU输出，修改与撤销由真实Engine执行。11组检查通过，覆盖：

1. 镜头元件、粒子发射与粒子外观在320、375、414、768、1024、1440px下无页面横向溢出，控件触控高度至少48px。
2. 元件稳定ID、复制、0.5px尺寸、排序、删除。
3. 连续参数提交、非法范围、动画开关、拖动单事务与真实撤销。
4. Esc取消拖动并恢复原值。
5. 外部修改产生陈旧revision后刷新状态，由用户明确重试。
6. 慢宿主响应下合并中间拖动值，保留最后一次输入，不积压逐事件修改。
7. 最终滑块修改失败后回滚整个事务，避免悬空事务或提交部分值。
8. RGBA连续修改保留其他通道；预设一次撤销；超20,000粒子容量反馈。
9. 当前帧变换编辑、锁定、断开/重连及宿主寻帧通知。

上述布局分为3组，行为分为8组，共11组；测试没有浏览器JS错误或CSP违规。原始结果、消息轨迹、GPU预览截图及日志保存在忽略的 `artifacts/plugin-editor-browser/`，不上传reference/refer或截图。测试脚本与原生驱动位于 `tools/plugin-editor-tests/` 和 `crates/aem-render/src/bin/plugin_editor_probe.rs`；原生驱动仅用于桌面测试，不暴露给插件或Android接口。

```powershell
$env:CARGO_TARGET_DIR='<测试构建目录>'
cargo build --offline -p aem-render --bin plugin_editor_probe
$env:EDITOR_PROBE_BIN='<测试构建目录>/debug/plugin_editor_probe.exe'
# 可选：使用已安装的Chrome，否则执行npx playwright install chromium。
$env:PW_BROWSER_PATH='C:/Program Files/Google/Chrome/Application/chrome.exe'
cd tools/plugin-editor-tests
npm ci
npm test
```

Node至少20，浏览器测试依赖锁定在package-lock.json。测试用回环HTTP资源白名单与注入的宿主通道；这是桌面Chromium验证，尚未挂载Android WebView容器。Android主框架/来源约束、窗口生命周期、线程转发、手机软键盘和触控体验须在App前端接入后验证。页面不提供节点模拟器、粒子碰撞、模型粒子等尚未交付的宿主能力。
