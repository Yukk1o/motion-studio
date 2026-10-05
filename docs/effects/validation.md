# 回归与AE参考采集

本机采集的AE应用版本为18.0.1x1，首批20项的原生效果版本均为18.0.1；matchName、参数名、原生单位、范围、默认值、可动画性保存在 `crates/aem-effects/reference/ae2021-parameters.json`。宿主归一化描述在library/manifest.json，逐项兼容记录在reference/records。AE内部曲线数据不是普通可序列化参数，自定义Curves参考仍需独立采集。

参考设置：8 bpc，sRGB IEC61966-2.1，关闭工作空间线性化，方形像素，关闭合成/图层运动模糊。基准工具生成256×256渐变、棋盘格和透明边缘；每项默认/代表参数，共117个静态用例（Curves只含3个默认用例）。这些用例不包含全部边界值和动画。

```powershell
# 在独立AE实例中运行；不要向用户正在编辑的AE实例发送脚本。
AfterFX.exe -m -r <仓库绝对路径>/tools/ae_collect.jsx
py -X utf8 tools/ae_reference_inputs.py
AfterFX.exe -m -r <仓库绝对路径>/tools/ae_render_references.jsx
# queue.json与render-references.aep生成后：
py -X utf8 tools/ae_render_cli.py <aerender.exe绝对路径> artifacts/ae-reference/18.0.1 --jobs 2 --skip-existing
cargo run --offline -p aem-render --bin effect_probe -- artifacts/ae-reference/18.0.1
py -X utf8 tools/compare_ae_effects.py artifacts/ae-reference/18.0.1 --publish
```

AE命令行脚本与aerender用法见Adobe的[脚本文档](https://helpx.adobe.com/ca/after-effects/desktop/automate-in-after-effects/automate-animation/scripts.html)和[自动渲染文档](https://helpx.adobe.com/my_en/after-effects/desktop/render-and-export/automate-rendering/automated-rendering-network-rendering.html)。需允许脚本写文件；安装输出模板必须提供TIFF+Alpha。模板名称、CLI返回码和UI启动行为可能随本机安装变化，工具以实际帧可解码为成功条件，保留原始TIFF和日志。不要仅凭进度文件或进程退出码认定采集完成。

序列文件名必须有 `[#####]`。本机AE曾在无帧标记时生成0字节占位文件；这些文件被拒绝。此安装的TIFF写入RGB+Alpha预乘输出，却把第4样本的TIFF标签写成unspecified，Pillow默认会丢Alpha。ae_reference_io.py仅在内存中修复该标签为associated Alpha再解码，保留原文件；此处理对应已确认的本机输出模块契约。更换输出模板为straight Alpha时必须同步修改解码契约。

RGB和Alpha误差以0～255单位报告，阈值均为3。edge区域为图像16像素边框或16像素邻域内存在Alpha变化的区域，interior为其余区域；RGB忽略两张图均完全透明的像素，Alpha统计整个ROI。whole/interior/edge分别报告。没有参考、无效参考或任何ROI超阈值均保留差异；脚本不会自动把效果标为verified。完整验收还应加入色块/脉冲、半透明边缘、边界值、参数动画、实例复制/顺序、图层变换和摄影机、正倒随机寻帧以及保存/撤销回归。

后端检查：

```powershell
cargo test --workspace --offline -j 1
py tools/build_android.py --abis arm64-v8a,x86_64 --effects-acceptance --task assembleDebug assembleDebugAndroidTest
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
adb install -r android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk
adb shell am instrument -w -e class com.motionstudio.editor.EffectsRuntimeTest com.motionstudio.editor.effectsacceptance.test/androidx.test.runner.AndroidJUnitRunner
adb shell am instrument -w -e class com.motionstudio.editor.ExportParityTest com.motionstudio.editor.effectsacceptance.test/androidx.test.runner.AndroidJUnitRunner
```

EffectsRuntimeTest检查缺失依赖阻止PNG/MP4、禁用Curves保留数据且不生成LUT、四效果链与编码MP4的全画面<6/前景<8，以及20项未编码GLES/wgpu的RGB/Alpha≤3。测试会输出App私有目录effects-test下的JSON报告。宿主既有GPU/媒体错误、取消与释放测试继续保留。请记录设备GL_RENDERER、API、ABI、耗时与峰值内存；模拟器不能替代物理设备性能验收。当前运行证据见reference/reports/validation.json。既有ExportParityTest先记录编码画面、时间戳和内存，再检查1080p吞吐量，阈值仍为30 fps；吞吐量未达标时测试仍失败。

本次 `.msfx` 构建：Rust工作区52项通过；arm64-v8a/x86_64构建成功；MuMu独立应用7项通过。20项未编码GLES/wgpu的最大RGB MAE为0.235/255、Alpha为0；四效果链MP4全画面1.175/255、前景1.281/255。既有1080p测试达到57.1 fps，8个索引帧的最大全画面误差2.135/255、前景4.639/255、时间戳误差不超过1微秒。117组AE静态对照75组达标、42组有差异，20项仍为近似实现、已验收0项。报告保留测试前后APK SHA-256和包hash；性能数字仅适用于这次模拟器运行。

effects-acceptance只为debug增加applicationIdSuffix，安装为com.motionstudio.editor.effectsacceptance，与前端开发的普通App并存；不改变发布版本应用ID。测试前后核对安装APK的SHA-256，避免共享设备上其它构建替换App导致报告混用。

物理设备交付前还要验证：高参数导致预算超限、不同预览分辨率、实际GPU驱动失败后的输入旁路、8pass/16实例上限、PNG资源、重复导入/冲突/版本卸载、导出取消和冻结资源、长时间预览后的资源释放。前端界面清单独立列在frontend-integration.md，当前分支不包含其实现。
