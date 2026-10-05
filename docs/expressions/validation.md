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
