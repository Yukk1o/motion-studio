# motion-studio 效果系统交接

本分支提供 `.msfx`（Motion Studio Effects）插件包、工程与命令模型、wgpu/JNI/GLES执行链、52项核心效果、SDK和回归工具。核心库包含原有20项AE近似效果、16项参考Sapphire视觉行为的独立实现，以及16项常用AE效果近似实现。效果编辑界面由前端开发接入，当前没有新增面板或ViewModel。原有内部 `aem-*` crate 名与工程扩展名保持兼容。

另有 [SDK 2 场景效果与插件专用编辑器](../scene-effects/README.md)，预装6项镜头/粒子生成器。当前JNI渲染计划为版本2；旧SDK 1包与已有精确哈希继续保留。

| 接收者 | 先读 | 内容 |
|---|---|---|
| 前端开发 | [frontend-integration.md](frontend-integration.md) | 安装、目录、增删排序、参数、关键帧、手势撤销、错误与导出接口 |
| 效果开发 | [sdk.md](sdk.md) | 包格式、WGSL契约、资源/色彩/Alpha/边界、模板与CLI |
| 平台渲染开发 | [render-plan.md](render-plan.md) | 版本化JNI二进制计划、参数布局、GLSL反射、GLES纹理与坐标 |
| 工程与验收开发 | [migration.md](migration.md)、[validation.md](validation.md) | v1→v2、版本并存、冻结导出、参考采集与测试 |
| AE还原开发 | [compatibility.md](compatibility.md) | 逐项matchName、静态对照覆盖、已知差异与验收缺口 |
| 核心效果开发 | [common-library.md](common-library.md)、[core-library.md](core-library.md) | 52项核心效果、范围复核、1.2.0接入与纹理优化 |

实现入口：`crates/aem-effects` 管描述/校验/注册表/编译；`aem-core/src/effects.rs` 管实例/曲线/命令；`aem-render/src/effect_plan.rs` 管共享计划；`effect_gpu.rs` 管wgpu资源；Android的NativeBridge/GlEffects/VideoExporter提供平台后端。SDK模板在 `sdk/effect-template`。内置包为 `crates/aem-effects/library/core-effects.msfx`；AE参数采集、输入、输出与原始报告仅保存在本地 `crates/aem-effects/reference`，不随源码分发。

52项核心效果均标记 approximate；已还原并验收数量以兼容矩阵为准，当前为0。新增16项没有Sapphire原版渲染对照，不能显示为已还原。静态样本达标不能代替参数边界、动画、叠加、摄影机和物理设备验收。禁用的效果不阻止导出；启用效果缺失、未支持参数或执行错误会阻止PNG/MP4。预览保留输入继续处理并提供实例定位。

后续宿主能力按以下次序增加：

1. 补齐当前算法与参数差异、AE自定义Curves采集、边界/动画样本，以及物理Android设备性能报告。
2. 噪声/光效：当前已提供单帧、8 bpc范围内的确定性颗粒、损伤与光效；进一步扩展HDR/浮点目标、独立生成图层及相应输出边界能力。
3. 图层引用与蒙版：定义引用生命周期、采样坐标、依赖拓扑/循环拒绝、缓存失效、导出快照；同时增加图层引用参数类型。
4. 跨时间取帧：定义帧请求范围、缺帧与取消策略、缓存/内存预算、随机寻帧确定性与时间映射。
5. 每次增加能力都要更新SDK/渲染协议、预览与导出两端及设备回归。当前manifest请求未开放能力会明确拒绝。

参考采集、截图和原始报告保存在本地 reference/，不纳入 Git；干净检出不依赖这些资料。
