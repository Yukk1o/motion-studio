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

EffectsRuntimeTest检查缺失依赖阻止PNG/MP4、禁用Curves保留数据且不生成LUT、四效果链与编码MP4的全画面<6/前景<8，以及核心1.1.0的36项未编码GLES/wgpu的RGB/Alpha≤3。测试会输出App私有目录effects-test下的JSON报告。宿主既有GPU/媒体错误、取消与释放测试继续保留。请记录设备GL_RENDERER、API、ABI、耗时与峰值内存；模拟器不能替代物理设备性能验收。早期20项运行证据见本地reference/reports/validation.json。既有ExportParityTest先记录编码画面、时间戳和内存，再检查1080p吞吐量，阈值仍为30 fps；吞吐量未达标时测试仍失败。

本次 `.msfx` 构建：Rust工作区52项通过；arm64-v8a/x86_64构建成功；MuMu独立应用7项通过。20项未编码GLES/wgpu的最大RGB MAE为0.235/255、Alpha为0；四效果链MP4全画面1.175/255、前景1.281/255。既有1080p测试达到57.1 fps，8个索引帧的最大全画面误差2.135/255、前景4.639/255、时间戳误差不超过1微秒。117组AE静态对照75组达标、42组有差异，20项仍为近似实现、已验收0项。报告保留测试前后APK SHA-256和包hash；性能数字仅适用于这次模拟器运行。

effects-acceptance只为debug增加applicationIdSuffix，安装为com.motionstudio.editor.effectsacceptance，与前端开发的普通App并存；不改变发布版本应用ID。测试前后核对安装APK的SHA-256，避免共享设备上其它构建替换App导致报告混用。

物理设备交付前还要验证：高参数导致预算超限、不同预览分辨率、实际GPU驱动失败后的输入旁路、8pass/16实例上限、PNG资源、重复导入/冲突/版本卸载、导出取消和冻结资源、长时间预览后的资源释放。前端界面清单独立列在frontend-integration.md，当前分支不包含其实现。


对齐 main (`87eea34`) 后新增片段/效果参数、负局部 Curves 键及共享执行计划时钟检查，Rust 工作区71项通过（core 57、effects 6、render 8）。原有 validation.json 对应集成前运行，不能视为本次集成后 Android 验证；本次结果单独保存在 reference/reports/main-integration.json。

集成后的 ARM64/x86_64 原生构建和两份 APK 构建通过。MuMu 独立应用完整13项仪器复核：12项通过，1项1080p吞吐检查失败（29.82 fps，阈值仍为30 fps）；该用例的180帧、8帧画面对照与时间戳均通过。同一 APK 的一次单独性能复核通过，吞吐30.59 fps。初次集合还出现编码器 Binder 通信停滞，停止测试应用后相关单测5.173秒通过；保留中断、完整集合失败及单独复核的全部原始日志。不能将单次复核通过表述为整组13项全通过，性能稳定性仍需物理设备及持续测试。

测试前后 APK SHA-256 一致；原始 JSON、日志及来源提交位于 reference/reports/main-integration-android 的 interrupted/full-suite/performance-recheck 三个目录，各有 run.json 索引。汇总见 main-integration.json。

参考采集、截图和原始报告保存在本地 reference/，不纳入 Git；干净检出不依赖这些资料。

## 核心1.1.0扩充验证（2026-10-06）

核心库共36项、六个一级分类，新增加的16项为独立近似算法。1.0.0继续预装，原20项描述及WGSL与1.0.0完全一致；1.1.0包SHA-256为 `df9da73a4c18ee5c8d1c652d60bb5c45b01677371e461f65cafc3272746bbbf2`。源码与包更改后必须重新生成对应报告，不能沿用本记录。

- `cargo test --workspace --offline -j 1`：75项通过（core 57、effects 7、render 11）。新增包版本保留检查及3项真实GPU测试，覆盖16项可见默认输出/关闭效果、发光外扩Alpha、锚点不变、空像素、种子变化、随机寻帧与局部时间。
- arm64-v8a/x86_64原生库及debug/AndroidTest APK构建通过。初次APK打包因E盘空间不足失败，释放空间后同一实现重建通过，没有更改算法或降低验收阈值。
- 独立MuMu效果验收应用（API 35、x86_64、GL_RENDERER报告Adreno (TM) 640）：EffectsRuntimeTest四项全部通过，用时9.103秒；APK测试前后SHA-256一致。
- 36项GLES/wgpu未编码对照全部通过，最大RGB MAE为0.264391/255（glint），Alpha MAE为0。Wave Warp及新增的漏光、抖动、颗粒、扫描线、胶片损伤、数字故障在第7帧测试，其它项在第0帧；Curves使用非恒等曲线。
- 原有Tint/Blur/Curves/Wave Warp四效果链12帧MP4及冻结插件引用检查通过，全画面RGB MAE为1.175415/255，前景为1.281022/255。此编码用例不等于新增16项组合的全面编码验收。

本次原始JSON、仪器日志及APK来源位于本地、Git忽略的 `artifacts/core-effects-device-final`，程序生成图集位于 `artifacts/gallery`。没有提交reference/refer资料。新增16项没有Sapphire原版参考输出，仍为approximate；没有物理设备性能报告。上述结果是指定输入和模拟器的双端执行验证，不能代替AE/Sapphire还原验收，也没有重跑或改写前文13项完整集合的性能结论。

四项最终设备测试通过后，共享ADB服务在回收APK来源时中断；通过独立5038端口恢复报告和测试后hash检查，未重新安装或重跑测试。回收记录包含在core-effects-device-final/run.json，原始仪器日志保留。
