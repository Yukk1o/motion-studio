# 图层混合与轨道遮罩

工程新增 `Layer.blend` 与可选 `Layer.track_matte`。编辑这些字段时升级为格式 10；未使用新能力的旧工程保留原版本和线性正常混合行为。GPU 帧计划升级为版本 8、144 字节头，末尾每图层 20 字节的混合记录由两端共用。

纯数据与校验位于 `motion-model::compositing`，`motion-core` 保留编辑命令并重导出类型；GPU 路径位于 `motion-render`，导出与能力查询由共享 `motion-host` 提供。render 和 media 的生产依赖继续保持与 core 及 JS 工具链解耦。

通过已有 `NativeBridge.command` 或带合成 ID 的 `CompositionBridge.command` 调用：

```json
{"op":"set_layer_blend","composition":"comp-main","object":2,"mode":"multiply","space":"srgb"}
{"op":"set_track_matte","composition":"comp-main","object":2,"matte":{"source":1,"mode":"alpha","hide_source":true}}
{"op":"set_track_matte","composition":"comp-main","object":2,"matte":null}
```

混合模式：`normal`、`add`、`multiply`、`screen`、`overlay`、`darken`、`lighten`、`difference`、`exclusion`、`subtract`、`divide`、`color_dodge`、`color_burn`、`hard_light`、`soft_light`。空间为 `linear` 或 `srgb`；包括正常模式在内，sRGB 按编码颜色执行颜色与不透明度合成。背景在图层混合及调整之后应用。

遮罩类型：`alpha`、`alpha_inverted`、`luma`、`luma_inverted`。读取来源图层的蒙版、效果、不透明度、变换及摄影机投影；不读取它的图层混合模式。来源也可使用轨道遮罩，深度最多 8 层。亮度为显示 sRGB 的 Rec.709 权重乘 Alpha。来源超出片段时间为零覆盖率，反转时为完整覆盖率。

来源和目标须为同一合成内的普通视觉图层，支持图片、视频、文字、矢量和子合成。拒绝自身、循环、过深引用、空对象、音频、调整图层及锁定目标。`hide_source` 默认 true，不改写来源的可见性或不透明度轨道。仍被引用的来源不能删除，需要先清除关联。

同一批复制中的来源映射到副本，外部来源保留引用；失效外部来源拒绝粘贴。预合成要求关联两端一起移动。编辑、复制和预合成保持原有原子命令、撤销、保存及工程包行为。参考值来自 `capabilities.layer_compositing`。

wgpu 与 GLES 共用 WGSL、参数布局和计划，效果后像素快照与置换输入共享。投影遮罩采用 R8，计入源资源预算；混合源与双累加器合计计入 128 MiB 合成预算，效果临时纹理仍使用独立设备预算。CPU 不回读逐帧遮罩。普通无新能力的图层保留原有直接绘制路径。

编辑器提供“混合”面板和原有时间轴、预览及撤销入口。
