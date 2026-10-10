# 形状组与中继器

矢量协议 3、工程格式 12 增加 `VectorSource::Group`。已有单路径或参数形状可通过 `vector` 命令的 `convert_to_group` 转换；原始路径、填充、描边及参数动画保留在 geometry 子项中。该能力用于通用形状创作，尚未取得固定 AE 版本的像素验收。

## 数据结构

```json
{
  "source": {
    "kind": "group",
    "group": {
      "id": 1,
      "name": "Group 1",
      "transform": {
        "position": {"value": [0, 0], "keys": []},
        "anchor": {"value": [0, 0], "keys": []},
        "scale": {"value": [100, 100], "keys": []},
        "rotation": {"value": 0, "keys": []},
        "skew": {"value": 0, "keys": []},
        "skew_axis": {"value": 0, "keys": []},
        "opacity": {"value": 100, "keys": []}
      },
      "items": []
    }
  },
  "fill": null,
  "stroke": null,
  "fill_rule": "non_zero"
}
```

items 按存储顺序求值。支持 group、geometry、fill、stroke、trim、repeater。组及子项 ID 在一个矢量图层内唯一且非零；组最多嵌套 8 级，共 512 个保存项。geometry 包含已有 VectorContent、局部 size 和 position 轨道；嵌套内容必须使用 group 子项。该组的绘制属性放入 items，外层 VectorContent 不再保存 fill/stroke/trim。

每项保留名称和独立轨道。组的位置、锚点、尺寸使用父组局部像素，Y 向下；正旋转为顺时针。缩放和不透明度以百分比计，角度以度计。

## 参数范围

| 参数 | 有效范围 | 默认值 |
| --- | --- | --- |
| position / anchor（XY） | −32768–32768 px | 0, 0 |
| geometry.size（XY） | 0–32768 px | 转换时原尺寸 |
| group.scale（XY） | −10000–10000% | 100, 100 |
| rotation / skew_axis | −360000–360000° | 0 |
| skew | −89–89° | 0 |
| group.opacity | 0–100% | 100 |
| repeater.copies | 0–1024 | 3 |
| repeater.offset | −1024–1024 份 | 0 |
| repeater.scale（XY） | −1000–1000% | 100, 100 |
| repeater.position（XY） | −32768–32768 px | 100, 0 |
| start_opacity / end_opacity | 0–100% | 100 |

这些范围是宿主有效范围。填充、描边和修剪沿用已有范围。连续参数支持关键帧和缓动，离散形状参数使用 Hold；不支持组参数表达式或分离维度。负缩放与非整数中继偏移可能没有实数结果，此时明确报错。采样越界不静默截断；控件滑动限于声明范围，输入越界显示错误。

## 绘制语义

- `composite=below` 把当前填充、描边或副本放在此前内容下方，`above` 放在上方。
- 填充/描边读取该项之前的轮廓。修剪作用于此前轮廓及其已有绘制属性；源节点不改写。各 geometry 保留自身样式。
- 组变换在描边生成后应用，使非等比缩放可以改变描边外观。不透明度在整组完成后应用一次；重叠绘制不会把组透明度重复相乘。
- 中继器保留虚拟副本，数量包含原件。非整数 copies 的最后一份按小数比例淡入；offset 按副本份数影响位置、旋转和缩放。连续两个中继器可以生成二维网格。
- 组透明度范围会生成独立的 MSAA 合成指令。副本分别保留组透明度，整个根组再应用根透明度。
- 临时纹理按尺寸和嵌套深度复用，仍计入 128 MiB 图层源/合成预算。缓存指纹包括几何、操作指令和根透明度。

组展开最多 4096 份轮廓实例、65536 个节点、4096 个绘制批次，最终三角顶点仍受 262144 限制。合法参数超过上述限制、设备尺寸或纹理预算会明确报错。

## 可见区域与两端执行

无启用效果、无蒙版、无图层图像取样依赖的 2D 图层可以按合成可见区域裁剪源纹理。区域通过当前图层矩阵反算，保留 authored 锚点及变换；图层移动后重新计算可见区域，避免网格副本被固定裁切。提交前剔除完全位于该纹理外的三角形。其他情况保持完整输入边界，超设备能力时报错。

帧协议版本 9 保留 144 字节头部，矢量记录跨度由 28 增加到 40 字节。新增 word 7/8 为指令表地址/数量，word 9 为根透明度 f32 位；顶点仍为 24 字节。指令为 16 字节：kind、start、end、opacity。kind 0 绘制 `[start,end)`，1 开始透明组，2 结束并合成；两端检查指令范围、深度、平衡和透明度。wgpu 与 GLES 消费相同顶点、绘制顺序和透明组指令。

## 编辑命令与安卓 UI

```json
{
  "op": "vector",
  "composition": "comp-main",
  "object": 12,
  "action": {
    "action": "set_group_parameter",
    "item": 7,
    "parameter": "copies",
    "frame": 30,
    "value": 5,
    "animated": true
  }
}
```

frame 为合成时间，后端转换到片段局部时间；value 支持标量、XY 或 RGBA，维数必须匹配。整体添加、删除和排序可用现有 replace 原子提交。返回采样的 `vector_layers[].group_parameters` 以保存项 ID 为键，提供当前值；不把全部虚拟副本节点送入 UI。

安卓“组织为形状组”进入内容编辑；支持分组、形状、独立填充/描边、修剪和中继器的添加、排序、删除。位置复用 TransformTouchpad 与数值输入，颜色复用颜色面板；动画继续使用现有时间轴和 CurveEditor。一次拖动一次撤销。桌面 UI 本次不修改。

当前安卓组内自由路径保留完整数据，但节点/切线编辑尚未接入该内容页；描边的完整虚线模式可通过数据接口设置。后续接入这两个编辑入口，不自动固定路径动画。组内混合模式、渐变绘制和 Merge Paths 尚未提供。

行为参考 [Adobe 形状属性、绘制和路径操作文档](https://helpx.adobe.com/after-effects/desktop/drawing-painting-and-paths/shapes-and-shape-attributes/shape-attributes-paint-operations-path.html)。复制、保存、恢复和冻结输出保存整个组结构；不等同于完整 AE 工程还原。
