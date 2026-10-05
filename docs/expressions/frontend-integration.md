# 前端接入：属性表达式

本分支只实现后端、JNI、构建及验收代码。前端开发负责入口、编辑器、错误显示和可视化，不需要在 Kotlin 或界面线程再次执行 JavaScript。

## 能力与线程

调用现有 `NativeBridge.state/command/history/seek/save/capture/renderPlanInfo/sampleRenderPlanInto`。会话仍需在创建它的同一个 worker 线程使用，表达式编译和求值不能放在 UI 线程。`state.data.capabilities.property_expressions` 包含：

```json
{"supported":true,"profile":"motion-studio-ae-js-1","engine":"QuickJS-NG","source_max_bytes":8192,"max_expressions":256,"cross_property_references":false,"opacity_unit":"percent"}
```

## 添加、编辑与启停

调用 `NativeBridge.command(session, json)`：

```json
{
  "op":"set_expression",
  "frame":7,
  "expression":{
    "target":{"kind":"property","object":2,"property":"position"},
    "source":"value + [time * 60, 0, 0]",
    "enabled":true,
    "seed":42,
    "profile":"motion-studio-ae-js-1"
  }
}
```

`frame` 为当前编辑的合成帧整数，不是秒或图层局部帧。启用时会先编译，并在这一个帧上验证结果；失败返回 `ok=false`，工程、revision 和历史不变。这个检查不保证其它帧都成功。

按同一 target 再次发送 `set_expression` 即替换源码或启停。禁用使用 `enabled=false`，保留 source、seed、profile 和原关键帧。禁用状态允许保存未完成或语法错误的草稿。锁定图层不能修改表达式。每次提交为一次撤销；不要每输入一个字符就提交。拖动、批量提交可复用现有 `history` gesture：2 开始、3 提交、4 取消；0 撤销、1 重做。

单轴表达式在 target 中加入 `"axis":"x"`（或 y/z）。不要求先分离维度，也不会改变原轨道结构。一个属性可以分别设置不同轴表达式，不能同时设置整个属性和该属性的轴表达式，包括禁用条目；先移除原条目。

连续效果参数使用：

```json
{"kind":"effect","object":2,"effect":1,"param":"p0003"}
```

效果实例 ID 只在图层内唯一。只支持已实现且可动画的连续参数；离散枚举、布尔和 Curves 曲线对象不支持表达式。禁用效果不运行它的参数表达式；再次启用可能暴露运行错误。

删除表达式：

```json
{"op":"remove_expression","target":{"kind":"property","object":2,"property":"position"}}
```

## 状态与错误

成功响应仍是 `{"ok":true,"data":{...state...}}`。`data.project.expressions` 为原始记录；`project.layers[].transform` 与效果 track 保留用户的原值、关键帧和缓动。`sampledLayers`、`sampledCamera`、`sampledEffects` 给出实际当前帧结果，单位沿用原后端（透明度是 0..1）。时间轴数据仍来自原轨道，不能用计算结果覆盖它。

表达式可能在其它帧、关键帧变更后或图层排序后失败。错误字符串包含目标 JSON、frame 与原因，例如 `expression {"kind":"property","object":2,"property":"position"} at frame 7: JavaScript: unknown is not defined`。显示源码和原关键帧，提供禁用、修改、删除，不要自动清空表达式。

命令若已经提交，但其它表达式在当前预览帧出错，仍返回 `ok=true` 并通过 `data.renderError` 报告，避免把成功存储的编辑误报为失败。`seek` 失败会返回 `ok=false`，目标 frame 仍成为当前 frame；随后取 `state` 显示错误。出现错误时采样字段可能是最后一次成功帧的结果，不应当作当前帧已更新。导入在第零帧失败的表达式仍可以打开并编辑；首次状态保留错误，初始几何来自原关键帧。

PNG、GLES 帧计划与 MP4 逐帧严格检查表达式，失败即阻止输出或中止任务，现有 MP4 清理逻辑移除不完整文件。预览保留最后成功画面并显示错误。启用效果参数的检查包括隐藏/非活动图层。表达式恢复正常采样后清除对应状态提示；绘制完成后读取 state 获取最终 renderError。

## 工程与生命周期

带表达式的工程使用格式 3。新建且从未添加表达式的工程仍是格式 2，旧格式 1/2 以空表达式列表加载。前端不要把格式 3 降级或在重新序列化时丢弃新字段。删除最后一个表达式仍保留格式 3。

复制图层、拆分图层、复制效果会复制相应源码和种子并重定向目标 ID；图层 ID 参与随机身份，因此副本的随机运动不同。删除图层、摄影机或效果会删除相应表达式，可以撤销。显式升级效果沿用现有“重置全部参数”的行为，同时移除该实例的参数表达式；前端升级说明需包含这一点。带 position 表达式的摄影机切换到 orbit 会被目标校验拒绝，先移除该表达式。

导出使用冻结的 Project 与插件引用，表达式源码、种子和关键帧一并冻结。导出中的编辑不改变任务。不要在 UI 单独调用 Math.random 或用 wall clock 计算结果。
