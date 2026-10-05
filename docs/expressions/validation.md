# 表达式验证

自动回归入口：

```powershell
cargo test --workspace --locked -j 1
py tools/build_android.py --effects-acceptance --task assembleDebug assembleDebugAndroidTest
adb -s <QA设备> shell am instrument -w -e class com.motionstudio.editor.PropertyExpressionsTest com.motionstudio.editor.effectsacceptance.test/androidx.test.runner.AndroidJUnitRunner
```

Rust 核心覆盖多行 JavaScript、函数/if 完成值、向量运算及成员赋值副作用、合成秒数/负偏移/关键帧时间、关键帧采样/近似速度、loopIn/Out 类型与区间、百分比透明度、单轴累积、摄影机、随机寻帧/独立 context、语法/类型/非有限数、死循环/内存申请/取消、保存恢复、撤销重做、复制/拆分/删除、禁用草稿。原有无表达式 20 图层 60 Hz 零分配测试继续运行。

Rust GPU 测试覆盖表达式移动几何、驱动 Tint 参数、PNG 输出及共享 GLES 执行计划，效果复制/删除后的表达式目标，以及冻结工程保留源码。

Android `PropertyExpressionsTest` 覆盖实际 JNI/QuickJS ABI、能力发现、原轨道/当前采样区分、历史、恢复、超时原子性、第零帧失败工程保留、错误阻止 PNG/帧计划，以及冻结表达式的第 7 帧 MP4 对照 PNG。MP4 全画面 RGB MAE <6、前景 <8（8 bit 数值）。原始图像、APK、视频和设备报告仅写忽略的 artifacts 或 App 私有目录，不提交 reference/refer。

尚未采集本机 AE 18.0.1 的表达式数值报告，wiggle/random/差分速度/ease 算法已明确标记差异。模拟器结果不能代替真机实时性能与内存测量。

## 2026-10-06 实测

- Rust 全工作区 85 项通过（核心 66、效果包 7、渲染 12），包含 9 项表达式核心检查和 1 项表达式 GPU 检查。
- Android ARM64 和 x86_64 release 原生库、隔离 debug APK 与测试 APK 构建通过。MuMu x86_64 QA 实例执行 `PropertyExpressionsTest`，2 项通过，0.815 秒。
- 128×128、30 fps、12 帧冻结表达式 MP4 输出完整，时间戳严格递增。第 7 帧相对 PNG 的全画面 RGB MAE 为 **0.654785/255**，前景为 **5.354167/255**，满足既有 MP4 阈值。编码器 `OMX.google.h264.encoder`，模拟器报告机型 V2362A；这是模拟器功能对照，不是真机性能报告。
- 后续帧表达式失败的 MP4 中止、删除不完整文件，资源清理报告没有错误；导出中修改实时会话的表达式没有改变冻结结果。
- 已确认 APK 包含两个 ABI 和引擎/依赖许可资产。测试对应源提交 `46307ed77616d61560e1150cd0cc241214af5fe7`；App APK SHA-256 `12804bd0ba0b514aa88cfd64442cdb60f16394a907c94785d8800c913db5fbf8`，测试 APK SHA-256 `70e547c47b8917bc9d42c36858cfb3d73443c89ad37224fdd7d290f088f36103`。测试后的已安装 APK 哈希一致；测试前哈希脚本因 Android 路径包含 `~` 被过严校验拒绝，已修正，未把这个未完成检查算作通过。
- 本地原始证据保存于忽略目录 `artifacts/expressions-rust-final.log`、`expressions-android-final-build.log`、`expressions-android-tests.log` 和 `expressions-device-provenance.json`，没有上传原始参考素材。
