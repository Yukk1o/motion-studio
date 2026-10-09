# 图层源蒙版验证

本批基于主分支 `0ca9665`，分支 `codex/layer-masks-mattes-20261008`，同时提供 Rust 数据／采样／wgpu、JNI／GLES 导出和原生 Compose 编辑面板。

| 检查 | 结果 |
|---|---|
| Rust 工作区 | 199 通过、0 失败、1 忽略，62 个套件；包含 3 个蒙版核心与 3 个 GPU／计划用例 |
| 原生与 Android | ARM64/x86_64 编译、APK、测试 APK、10 项 JVM 与 lint 通过 |
| Android 蒙版 | 普通 1080p 工程、1080×2400 手机屏幕下 3 项完整组合检查通过；另单独运行过各用例 |
| 代码复核 | 一位子 agent 检查双端及界面状态；修复旧 UI 快照覆盖、动画越界与钢笔路径选择问题 |

核心覆盖合法性、重复 ID／错误排序拒绝、开放／None 路径、图层局部时间、增量参数／启停状态、保存恢复、手势撤销／重做、弹性 Alpha 的物理范围。GPU 覆盖相加／相减、反转、不透明度、羽化、正／负扩展、静态几何复用、蒙版移除释放，以及先蒙版再模糊的外扩。

真实 Android 对照分别运行 wgpu PNG 与 GLES 未编码缓冲：普通矩形覆盖一致；双轴羽化、第二个半透明 Subtract 蒙版及 Gaussian Blur 组合，在 49 个内区／边缘采样点的 Alpha 差值不超过 3。读取 GLES 原始数据时按 bottom-row-first 翻转坐标。该检查不扩大为全部参数、所有边缘或 AE 全图逐像素一致。

界面覆盖矩形创建、实际预览节点、逐帧采样、增量连续编辑不丢前值、关键帧／撤销及已有路径选择。截图保存在忽略的 `artifacts/mask-panel.png`。

验证过程曾出现一次 Android ART 原生进程退出（QuickArgumentVisitor 空地址），单用例与完整 3 项重跑均通过，未复现。保留原失败记录，不将一次重跑视为所有手机的稳定性证明；真机连续播放、复杂项目及长时间资源记录仍需进一步验收。

原始结果：`artifacts/layer-masks-workspace-tests-final.log`、`layer-masks-final-build.log`、`layer-masks-combined-confirmation.log`、`layer-masks-ui-isolated-tests.log`、`layer-masks-parity-isolated-tests.log`。源工程、参考素材、截图、日志与 APK 不提交到 Git。

完整契约见 [host-layer-masks.md](host-layer-masks.md)。轨道遮罩、调整图层区域蒙版、混合模式及 AE 参考帧验收尚未交付，不计入本批完成项或“佩丽卡已还原”。
