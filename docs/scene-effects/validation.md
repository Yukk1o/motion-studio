# 场景效果验收

验收日期：2026-10-06（Asia/Tokyo）。代码提交 `d550363dd51ff61484bd0c46519b57c17d48786f`；本次后续提交只更新文档。完整数字及构建哈希见 [validation.json](validation.json)。测试对象为 Motion Studio 独立效果，没有进行厂商插件画面对照。

## 结果与范围

- Rust workspace：98 项通过，0 失败；原基线 85 项，新增核心编辑器 3 项、渲染 7 项、包校验 3 项。
- 修改过的 Rust 文件格式检查通过；workspace Clippy 通过，保留已有告警。
- 场景包 effect_tool check、插件 JavaScript 语法检查和重复生成哈希一致性通过；既有 SDK 1 核心包哈希保持。
- Android ARM64 / x86_64 原生库及两个测试 APK 构建通过。设备执行为 MuMu QA 实例 1、Android 15、x86_64，独立 App ID `com.motionstudio.editor.effectsacceptance`。
- Android 完整回归：9 项通过（场景 3、原效果运行时 4、表达式 2），耗时 12.19 秒。没有在 ARM64 真机运行。

覆盖：编辑器版本固定、作用域限制、非法元件、锁定、陈旧 revision、撤销/取消、参数轨道、所属图层变换、保存恢复和实例删除；粒子随机寻帧、全发射器裁剪、边缘精灵保留、摄影机投影、预算失败和帧计划边界；光源 Null 跟随、前后深度、Solid / 图像 Alpha 及半透明遮挡；六个生成器实际 GPU 绘制及后接图像效果；UI 资产访问限制、GPU PNG 预览、冻结导出和失败输出清理。多个效果共享同一编辑器 PNG 时只计一次解码预算。

## wgpu / GLES 未编码对照

透明 128×128 合成，帧 7；粒子 extent 设为 [80,80,40]。RGB、Alpha 分别统计，单位为 8 位通道值；通过阈值均为 MAE ≤ 3（即 3/255）。GPU 线性预乘结果按原有 PNG 捕获规则还原 straight alpha；PNG 对照解码设置 `inPremultiplied=false`、`inScaled=false`。

| 效果 | 可见实例 | RGB MAE | Alpha MAE | 结果 |
|---|---:|---:|---:|---|
| lens_flare | 7 | 0.190917 | 0.000000 | 通过 |
| starfield | 400 | 0.064472 | 0.000000 | 通过 |
| sparks | 63 | 0.031006 | 0.000000 | 通过 |
| dust | 335 | 0.166550 | 0.000000 | 通过 |
| snow | 77 | 0.025210 | 0.000000 | 通过 |
| energy | 91 | 0.152031 | 0.000000 | 通过 |

调试期间，默认 Android PNG 解码的预乘量化使 Dust RGB MAE 显示为 4.539867（RGBA8）及 4.683491（RGBA16F）。这些数据包含对照解码误差，不能用来证明 RGBA16F 单独改善了画面。改用原始像素解码后得到上表结果，测试阈值未改变。Android 的默认预乘及原始像素用途见 [BitmapFactory.Options 文档](https://developer.android.com/reference/android/graphics/BitmapFactory.Options#inPremultiplied)。线性 RGBA16F 累积仍作为精度方案保留，最终输出继续使用既有 8 位链。

## MP4 对照

Sparks、128×128、12 帧、30 fps、黑色背景；比较帧 7。导出回调中把编辑会话的出生速率改为超预算值，已经冻结的输出仍成功，后续新任务明确失败并删除失败输出。

- 全画面 RGB MAE：1.022278 / 255，阈值 < 6/255。
- 前景 RGB MAE：5.326493 / 255，阈值 < 8/255；前景 2144 像素。

## 构建身份与复现

场景包 SHA-256：`ffd6d7ddbff861072da76dd7f84f072a7b01b187f8488e59e88e4bc468c20724`。已安装的 App / test APK 分别为 `a07e49be22a49b6e761843ac516eb2a8252cc3140615a3300a75cf333c188fd3` / `72004f03dcbc1ceedcdf052e4ebd9edb2ee8ffb3b948d78577f9373f62755786`；通过设备 `sha256sum` 与本地 APK 比对，二者均一致。两份 ABI 的库哈希记录在 JSON 中；APK 与原始图像保存在本地，不加入 Git。

```powershell
cargo test --workspace -j 2
cargo clippy --workspace --all-targets -j 2
python tools/generate_scene_library.py
node --check crates/aem-effects/scene-library/ui/editor.js
cargo run -p aem-effects --bin effect_tool -- check crates/aem-effects/scene-library/scene-effects.msfx
$env:CARGO_BUILD_JOBS='2'
python tools/build_android.py --abis arm64-v8a,x86_64 --effects-acceptance --task assembleDebug assembleDebugAndroidTest
```

设备安装与测试使用独立 ADB server 5038、QA serial `127.0.0.1:16416`；仅操作 QA 实例，不操作前端实例。安装 debug App 和 androidTest APK 后：

```powershell
adb -P 5038 -s 127.0.0.1:16416 shell am instrument -w -r -e class com.motionstudio.editor.SceneEffectsTest,com.motionstudio.editor.EffectsRuntimeTest,com.motionstudio.editor.PropertyExpressionsTest com.motionstudio.editor.effectsacceptance.test/androidx.test.runner.AndroidJUnitRunner
```

断言 `OK (9 tests)`；am instrument 的 shell 退出码不能单独证明通过。SceneEffectsTest 将数值报告写到 App 私有 `files/scene-effects-test/<uuid>/project/`，可以通过 run-as 提取。

## 未执行与能力边界

App WebView 容器/窗口视觉验收由前端接入后完成；ARM64 真机执行、手机耗时/内存/温升、长时间粒子压力测试及厂商插件逐像素对照尚未执行。首版支持局部空间精灵粒子和源 Alpha 平面遮挡；没有模型粒子、碰撞、拖尾或粒子与其他图层逐粒子深度交错。完整限制见 [runtime.md](runtime.md)。通过当前测试不表示这些能力已经交付。
