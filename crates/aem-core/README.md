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
