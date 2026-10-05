# 编辑核心接口

## 图层片段

所有普通图层使用左闭右开区间 `[in_frame, out_frame)`。`timeline.offset_frame`
是有符号整数，动画局部帧 = 合成帧 − offset。小数帧按同一规则采样。
摄影机 `object: 0` 保持全合成语义，不支持片段命令。

```json
{"op":"move_layer_clip","object":2,"in_frame":40}
{"op":"trim_layer_clip","object":2,"in_frame":30,"out_frame":80}
{"op":"split_layer_clip","object":2,"frame":61}
```

移动同时平移显示区间和动画偏移；裁剪只改变显示区间；分割保留完整动画轨道、
偏移、锚点、父级绑定和素材引用。新右段插入原段之后，原 ID 留给左段。
父级无论是否在显示区间都参与变换计算，每个对象各自换算局部时间。

既有关键帧命令的 `frame / from / to` 仍为合成整数帧；持久化关键帧使用
有符号局部帧。图层的区间外关键帧保留，摄影机关键帧仍限制在合成内。
非法帧、锁定图层、摄影机、越界区间、数值溢出和图层上限均拒绝整次编辑。
批量失败回滚，拖动复用现有 gesture 历史机制，相同目标值不增加 revision。

JNI 沿用 `NativeBridge.command`。成功响应仍包含完整 `data.project`、revision
和撤销状态，新增只读 `data.timeline_layers`：

```json
{
  "object": 2, "in_frame": 40, "out_frame": 120, "offset_frame": 40,
  "active": true,
  "properties": {
    "position": {
      "value": [128,128,0],
      "keys": [{"frame":40,"local_frame":0,"value":[128,128,0],"ease":"linear"}]
    }
  }
}
```

`properties` 含 position、rotation、scale、opacity；曲线定义随关键帧返回。
`frame` 使用 64 位合成时间，`local_frame` 使用 32 位有符号存储时间，区间外
的关键帧也会返回。派生数据不落盘。`active` 表示该帧满足区间及 visible 条件；
绘制还受透明度和内容类型约束。

分割的命令响应另含一次性 `data.edit_result`：

```json
{"op":"split_layer_clip","left_object":2,"right_object":4}
```

批量分割同时返回 `edit_results` 数组。后续状态查询不保留上一次编辑结果。
撤销恢复原区间并删除右段，重做恢复相同 ID。

新格式版本为 2。缺省 timeline 表示全合成区间、偏移 0。版本 1 工程在内存中
升级，不因加载覆盖文件；显式保存或备份后保留新字段。未知版本明确报错。
每条轨道最多 36,000 个关键帧，局部整数溢出明确拒绝。

测试：`cargo test -p aem-core`，以及 Android `LayerClipTimelineTest`。
时间映射统一进入 Scene，用于预览、观察、投影点选、PNG 和冻结 MP4 输出。
此批只提供静态素材的片段能力；视频、音频、真实模型、多合成和预渲染尚未实现。

## 用户主动分离 XYZ

默认保留整向量轨道，既有工程加载时不自动分离。前端由用户选择后调用：

```json
{"op":"separate_dimensions","object":2,"property":"rotation"}
{"op":"set_component","composition":"comp-main","object":2,"property":"rotation","axis":"x","frame":30,"value":45}
{"op":"animate","object":2,"property":"rotation","axis":"y","frame":15,"enabled":true}
{"op":"move_key","object":2,"property":"rotation","axis":"y","from":15,"to":20}
{"op":"curve","object":2,"property":"rotation","axis":"x","frame":30,"easing":{"ease":"linear","curve":{"space":"progress","shape":{"kind":"elastic","oscillations":2.5,"damping":6}}}}
```

`separate_dimensions` 支持普通图层 position、rotation、scale，以及摄影机
position、target；摄影机环绕模式禁止 position 编辑。标量属性禁止分离。
已经分离再次调用为无变化；分离及后续编辑可撤销/重做。
单轴编辑前必须显式分离，否则报错，不自动改变存储模式。

分离时将原向量的每个分量、关键帧时间、ease 和 curve 逐项复制到对应轴，
不烘焙。每轴有自己的静态值、键时刻和相邻区间，角度保留多圈数值。
持久化只保留以下唯一有效数据，不另存过期的向量关键帧：

```json
{"axes":{"x":{"value":0,"keys":[]},"y":{"value":15,"keys":[]},"z":{"value":720,"keys":[]}}}
```

`animate / move_key / copy_key / delete_key / ease / curve` 均接受可选 axis。
指定轴只编辑该轴，省略表示明确的整体操作，绝不读取界面的轴选择。
整体关键帧编辑先验证所有轴的源键/区间；缺键或只有部分轴存在目标碰撞时
整体失败，不通过补帧凑齐条件。整体开启动画时三轴动画状态需一致。
移除单轴动画使用 `animate` + `axis` + `enabled:false`。

数值联动使用 `NativeBridge.command` 的数组提交两条 `set_component`，
每条明确指定要修改的轴、合成帧和数值。它们原子提交，不改其他轴或既有曲线。
复制曲线沿用 `easing` 定义：从源键读取定义后，对目标轴的区间起始键调用
`curve`；没有单独的全局曲线剪贴板，数值和键时间不包含在定义中。
一次拖动仍以 `NativeBridge.history(id,2)` 开始、3 提交、4 取消。

状态增加 `capabilities.separate_dimensions`，包含 supported、activation:explicit、
layer_properties、当前模式允许的 camera_properties 和 axes。每个派生向量属性
返回 `separated`；分离后 `axes.x/y/z` 各自返回 value、keys，以及每键的
64 位合成 frame、32 位 local_frame、ease/curve。顶层 keys 为空，不伪造一份
共享缓动的合成向量键。`sampledLayers` 和 `sampledCamera` 继续返回组合 XYZ。
摄影机派生向量轨道位于 `timeline_camera.position/target`。

请求可省略 composition，也可明确 `composition:"comp-main"`；其他 ID 报错。
当前只有一个主合成；capabilities 明确标记 multiple_compositions、video_import、
audio_import、model_import、prerender 为 false。分离轨道纳入现有 revision、
手势历史、时间编辑、父子关系、保存/备份和冻结输出；未来合成库与预渲染缓存
尚未提供，不能以此宣称相关验收已完成。

检查：`separate_dimensions.rs` 验证独立数据、无损转换与历史；`hot_path.rs`
验证分离后 20 图层逐半帧求值零分配；Android `SeparateDimensionsApiTest`
验证真实 JNI、协议能力、单轴曲线历史及实际 H.264/PNG 比对。
