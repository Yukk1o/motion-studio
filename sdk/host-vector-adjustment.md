# 调整图层、矢量路径与形状：前端接入协议 1

工程格式 **6**；GPU 帧计划 **4**。本次交付包含核心模型、编辑命令、wgpu 预览和 GLES 导出适配。通用的添加菜单、钢笔工具、控制点、样式面板和参数面板由前端实现。

## 能力与形状目录

读取 `NativeBridge.state(session)` 的 `data.capabilities`：

```json
{
  "adjustment_layers": {
    "supported": true,
    "command": "add_adjustment",
    "composite": "lower_layers",
    "mask": "transformed_rectangle",
    "background": "excluded",
    "three_d": false
  },
  "vector_drawing": {
    "supported": true,
    "protocol": 1,
    "command": "vector",
    "coordinates": "centered_canvas_pixels_y_down",
    "max_paths": 64,
    "max_nodes": 2048,
    "fill_rules": ["non_zero", "even_odd"],
    "stroke_caps": ["butt", "round", "square"],
    "stroke_joins": ["miter", "round", "bevel"],
    "shape_catalog": []
  }
}
```

`shape_catalog` 实际返回 25 项，包含 `id/name/english_name/category/parameters`。一级分类为 `basic`（基础）、`polygons`（多边形）、`symbols`（符号）、`curves`（曲线）。参数描述含 `id/min/max/default/unit/discrete`，前端直接据此生成滑块、输入框和计数器。

| 分类 | 形状 ID |
| --- | --- |
| 基础 | rectangle, rounded_rectangle, ellipse, circle, line |
| 多边形 | triangle, polygon, pentagon, hexagon, octagon, trapezoid, parallelogram, diamond |
| 符号 | star, arrow, double_arrow, plus, heart, chevron, gear |
| 曲线 | ring, arc, sector, teardrop, flower |

参数范围是 Motion Studio 的原生契约；这些形状采用独立数学实现，没有声明 AM 或 AE 的参数兼容性。

## 创建图层

下面的 JSON 传给 `NativeBridge.command(session, json)`。ID 由前端分配，必须非零且在工程内唯一。层列表按从底到顶排列，新建图层追加到顶端。

```json
{"op":"add_adjustment","id":10,"name":"调整图层"}
```

新调整图层为全合成大小，位置在合成中心，2D、默认启用。通过现有 `NativeBridge.plugin` 的 `add` 请求添加图像效果，通过现有变换、排序、剪裁时间、启停和历史命令编辑。

```json
{"op":"add_shape","id":11,"name":"星形","shape":"star","size":[200,200],"position":[540,960,0]}
```

形状默认白色填充；线段和圆弧默认无填充、4 px 白色圆头描边。`size` 是未变换画布尺寸，`position` 是合成坐标。形状允许启用 3D、父子关系和已有图像效果。

## 路径与画布坐标

`content.kind` 为 `vector`，`content.vector` 保存全部可编辑数据。坐标以图层画布中心为 `(0,0)`，右为 +X，下为 +Y，单位为未变换图层像素。屏幕触点必须先逆投影、逆图层变换，再转入这一坐标系；不能直接保存预览像素坐标。

一条路径包括稳定的 `id`、`closed` 和节点数组。节点 ID 在同一条路径内唯一；路径 ID 在图层内唯一。节点几何是可动画轨道，六个分量依次为：

```text
[x, y, incoming_dx, incoming_dy, outgoing_dx, outgoing_dy]
```

切线相对节点位置保存。相邻节点的三次贝塞尔控制点为前节点加出切线、后节点加入切线。双方切线为零时使用直线段。移动节点不改变相对切线；平滑／对称节点由前端联动更新两个切线。

`state.data.vector_layers` 返回当前可见且处于时间范围内的矢量图层采样结果，每项包含 `id/canvas_size/source_rect/mvp/paths/fill/stroke`；路径及节点保留稳定 ID，`nodes[].geometry` 是当前帧六分量数值。参数化形状提供从 1 开始的预览节点 ID，转换为路径后才允许节点编辑。该数据用于显示控制点，不应覆盖工程中的关键帧轨道。

`mvp` 为按列排列的 16 个浮点数，已包含父级、锚点和摄影机。将控制点 `[x,-y,0,1]` 乘以该矩阵，除以 W，再把 NDC 转为合成画面坐标；切线端点使用 `[x+dx,-(y+dy),0,1]`。拖动反向用逆矩阵的近／远射线与局部 Z=0 平面求交，最后将局部 Y 取负。零尺度或射线平行时禁用拖动，避免保存无穷值。触点应先扣除预览画面的缩放和留黑区域。

闭合路径至少有三个节点，开放路径可保留零／一个节点以支持绘制过程；不足两个节点时不产生像素。只有闭合路径参与填充，开放路径参与描边。复合路径共享填充规则，`even_odd` 可以制作镂空，`non_zero` 按绕数决定填充。

画布外路径及描边会扩展居中的源纹理边界，图层锚点、位置和原画布尺寸保持原数据；宿主按控制点包围盒保守外扩。效果处理扩展后的源矩形，外扩部分的源区域坐标可以为负；效果内的位置仍从原画布左上角开始，画布中心参数不会因路径或描边扩展而漂移。前端编辑坐标是原画布中心坐标。

## 编辑路径

创建空钢笔图层可先用 `add_shape` 创建占位形状，再用 `set_paths` 改为空路径。也可以使用现有 `add` 命令一次提供完整 `Layer`。

```json
{
  "op":"vector","object":11,
  "action":{
    "action":"set_paths",
    "paths":[{
      "id":1,"closed":true,
      "nodes":[
        {"id":1,"geometry":{"value":[-80,-60,0,0,40,0],"keys":[]}},
        {"id":2,"geometry":{"value":[80,-60,-40,0,0,40],"keys":[]}},
        {"id":3,"geometry":{"value":[0,80,40,0,-40,0],"keys":[]}}
      ]
    }]
  }
}
```

新增／删除／排序节点、开闭路径：修改路径数组，保留未变更节点的 ID 和轨道，发送 `set_paths`。该命令替换整个路径源，会替换参数化形状。节点拖动用：

```json
{"op":"vector","object":11,"action":{"action":"set_node","path":1,"node":2,"frame":30,"value":[80,-40,-40,0,0,40],"animated":true}}
```

`frame` 是合成帧，宿主转换为图层局部帧。`animated:true` 创建／更新关键帧；`false` 会解除该轨道动画并设置常量，前端应传当前动画状态。拖动开始调用 `NativeBridge.history(session,2)` 开启手势，连续发送命令，松手用操作 3 提交，取消用操作 4 回滚；操作 0/1 分别为撤销／重做。一次拖动对应一条撤销记录。命令失败保留工程和历史，显示响应的 `error`。

## 填充与描边

```json
{
  "op":"vector","object":11,
  "action":{
    "action":"set_paint",
    "fill":{"value":[0.1,0.8,0.7,1],"keys":[]},
    "fill_rule":"even_odd",
    "stroke":{
      "color":{"value":[1,1,1,1],"keys":[]},
      "width":{"value":6,"keys":[]},
      "cap":"round","join":"round","miter_limit":4
    }
  }
}
```

`fill:null` 或 `stroke:null` 表示关闭该样式。颜色为 sRGB 非预乘 `[R,G,B,A]`，各分量范围 0–1。描边宽度 0–4096 px；斜接限制 1–100。宿主线性预乘合成并用 4× MSAA 栅格化，描边画在填充上方。颜色和宽度复用现有 `Track`、缓动和曲线 JSON；首期不支持渐变、虚线、笔刷、路径布尔编辑及逐点自由绘制。

## 参数化形状与动画

```json
{"op":"vector","object":11,"action":{"action":"set_parameter","parameter":"inner_ratio","frame":0,"value":0.45,"animated":false}}
```

| 参数 | 有效范围 | 单位／行为 |
| --- | --- | --- |
| corner_ratio | 0–1 | 圆角半径 / 半短边 |
| points | 3–32 | 整数；点／齿／瓣数，动画强制 Hold |
| inner_ratio | 0.01–0.99 | 内半径 / 外半径 |
| shaft_ratio | 0.01–0.99 | 箭杆／十字粗细比例 |
| angle | −3600–3600 | 度，所有形状支持 |
| start_angle | −3600–3600 | 圆弧／扇形起始角，度 |
| sweep_angle | −360–360 | 有向圆弧角度，度；零为无图形，±360 为整圆 |

具体可用参数和默认值读取目录；不属于该形状的参数会被拒绝。关键帧值和采样值都检查范围，曲线越界不自动截断。

```json
{"op":"vector","object":11,"action":{"action":"convert_to_path","frame":30}}
```

转换保存指定帧的几何快照，生成稳定的路径／节点 ID，填充与描边轨道保留。形状参数动画转换为该帧静态路径，前端应明确提示；可撤销。形状旋转参数属于几何，图层旋转属于空间变换。

修改完整轨道（缓动、删除／移动关键帧）可读取 `project.layers[].content.vector`，调整目标轨道，再发送 `{"op":"vector","object":11,"action":{"action":"replace","vector":...}}`。轨道 `keys[].frame` 为图层局部帧；便捷设置命令的 `frame` 为合成帧。设置样式用 `set_paint` 可避免覆盖其他几何字段。

## 调整图层的合成语义

调整图层处理它下方所有可见且处于时间范围内的图层合成，包括下方的其他调整图层和 3D 平面。上方图层随后绘制；调整层是 3D 排序分组边界。合成背景色在最后加入，不参与调整效果。

效果输入是整个合成画面，效果中的中心、半径等参数使用合成像素坐标。调整层的矩形、位置、锚点、2D 旋转、缩放决定作用区域，边界使用像素抗锯齿；首期没有羽化或任意路径蒙版。透明度在预乘线性空间混合原画面和效果结果，因此半透明素材不会被重复叠加。

无有效启用效果时调整层是恒等操作。调整层不接受粒子／光效场景生成器，也不接受 3D 开关。插件错误沿用预览旁路、错误标记和正式导出阻止的现有行为。

## 缓存、预算与导出

静态矢量采样、三角化和源纹理缓存按图层内容／画布尺寸／渲染分辨率更新。空间变换不会重新生成源纹理；几何或样式动画更新对应资源，尺寸不变时继续使用已有源纹理。同尺寸矢量实例共享 MSAA 临时资源。删除／隐藏后的资源在后续渲染清理。普通无效果素材仍保留直接绘制路径。

每层最多 64 条路径、总计 2048 个节点；节点坐标／相对切线各在 ±32768 px 内；每帧矢量展开顶点总数最多 262144。源纹理与素材共享 128 MiB 上限，并检查 MSAA 更新的瞬时峰值。调整层使用可复用的两个合成纹理，与效果临时纹理合计不超过 64 MiB。超限明确报错，不能缩小合法参数或裁切输出。

PNG 和 MP4 使用同一模型和三角网格。版本化帧计划 v4 增加矢量资源表与源类型；已有固定 32 浮点／几何接口对新源类型返回错误，请使用 `renderPlanInfo` / `sampleRenderPlanInto`。

v4 头部 112 字节，原 v3 字段位置保留：byte 80 为平面顶点数；84/88 为矢量资源表地址／记录数；92 为矢量数据地址；96 为顶点跨度 24；100/104 为合成宽高；108 为记录跨度 28。矢量记录为七个 u32：绘制索引、宽、高、顶点地址、顶点数、缓存指纹低／高 32 位。顶点是六个 f32：裁剪空间 XY、线性预乘 RGBA。`draw.words[31]`：0 为已有源，1 为矢量源，2 为调整层；调整层矩阵是作用区域的逆模型矩阵，16–19 为效果输出区域，20–21 为矩形大小，22 为混合透明度，25–26 为效果纹理有效 UV 比例。

GLES 使用相同三角网格，处理 FBO 与插件输入的 Y 方向差异。全屏三角形与精灵角点由算术／条件表达式生成，避免部分 GLES 软件驱动动态索引常量数组时输出空白；GLES 3.0 设备采用不请求计算着色器的图形能力下限。导出沿用独立工程与插件快照，正在编辑的路径不影响已启动任务。

## 迁移与验收

格式 1–5 打开时在内存迁移为 6，不修改旧文件，保留已有图层、效果、媒体和关键帧；格式 6 的新内容不能被旧宿主读入。前端识别 `vector/adjustment` 后使用上述能力描述生成控件，避免把它们当作纯色层。

回归用例见 `crates/aem-core/tests/vector_layers.rs`、`crates/aem-render/tests/vector_adjustment.rs` 和 Android `VectorAdjustmentTest`。前端接入应覆盖：创建与排序、开放到闭合、节点增删和切线拖动、样式动画、转换提示、一次拖动一次撤销、工程保存恢复、调整层区域变换、错误展示与冻结导出。
