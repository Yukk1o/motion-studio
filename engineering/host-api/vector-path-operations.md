# 矢量修剪与虚线描边

`capabilities.vector_drawing.protocol=2` 增加 `path_modifiers`。共享 model/core/render 与 Android 前端支持修剪路径、虚线描边；桌面 UI 本次未修改。嵌套形状组和 Repeater 尚未提供。

## 数据与范围

`VectorContent.trim` 可选，包含 `start`、`end`、`offset` 三条标量轨道和 `mode`。`Stroke.dashes` 可选，包含交替的线段／间隔轨道数组 `pattern` 和偏移轨道 `offset`。轨道复用 `{value,keys}` 及既有 `ease/curve`；不支持维度分离或表达式。

| 参数 | 范围 | 默认值 | 单位 |
| --- | --- | --- | --- |
| trim.start | 0–100 | 0 | % |
| trim.end | 0–100 | 100 | % |
| trim.offset | −360000–360000 | 0 | 度 |
| dashes.pattern 偶数索引 | 0.1–32768 | 12 | 图层局部像素 |
| dashes.pattern 奇数索引 | 0–32768 | 8 | 图层局部像素 |
| dashes.offset | −32768–32768 | 0 | 图层局部像素 |

上述是 Motion Studio 的有效范围，不声明为 AE 属性的完整有效范围。pattern 必须为 2、4 或 6 条轨道，即一至三组线段与间隔。零长度线段形成的圆点尚未支持；零间隔支持连续描边。越界键值与缓动导致的越界采样明确报错，不静默截断。编辑命令的 `frame` 是合成时间，后端转换为片段局部时间。

## 执行语义

固定顺序为源路径 → 修剪 → 虚线 → 填充与描边 → 图层效果链。源路径和稳定节点 ID 保持不变，节点编辑与参数形状转换仍操作原始路径。

- `simultaneously`：每条轮廓分别应用同一百分比区间。
- `individually`：按存储顺序、按弧长把轮廓视为一个整体区间。
- start/end 交换不改变可见区间；二者相等隐藏全部，差为 100 保留全部。offset 按 360° 周期循环。跨过闭合轮廓起点的区间连接为一条开放描边，不在原始起点增加端帽。
- 贝塞尔弧长使用自适应表定位，实际片段通过 de Casteljau 裁切，保留三次曲线。它是数值近似，不是按节点数量分配长度。
- 虚线相位在每条修剪后的轮廓起点重新开始；正偏移推进周期。闭合路径首尾连续的线段合并，端帽和拐角沿用现有 Stroke 配置。
- 默认没有修剪时，开放轮廓保持现有不填充的行为。启用修剪后，填充隐式闭合剩余片段；虚线只影响描边。
- 每次路径操作最多生成 16384 个节点，弧长表最多 65536 个采样点，虚线周期遍历最多 131072 步。超限错误包含图层定位，不生成截断的画面。

两端使用同一三角网格，无新增 shader 或 JNI 渲染计划版本。静态参数沿用现有缓存；动画、任意寻帧或编辑使相应网格指纹更新。

## 命令示例

```json
{
  "op": "vector",
  "composition": "comp-main",
  "object": 12,
  "action": {
    "action": "set_trim",
    "trim": {
      "start": {"value": 0, "keys": []},
      "end": {"value": 100, "keys": []},
      "offset": {"value": 0, "keys": []},
      "mode": "simultaneously"
    }
  }
}
```

```json
{
  "op": "vector",
  "object": 12,
  "action": {
    "action": "set_modifier_parameter",
    "parameter": "trim_end",
    "frame": 30,
    "value": 50,
    "animated": true
  }
}
```

```json
{
  "op": "vector",
  "object": 12,
  "action": {
    "action": "set_dashes",
    "dashes": {
      "pattern": [{"value": 12, "keys": []}, {"value": 8, "keys": []}],
      "offset": {"value": 0, "keys": []}
    }
  }
}
```

参数 ID：`trim_start`、`trim_end`、`trim_offset`、`dash_offset`、`dash_0`…`dash_5`。`animated` 省略时保留动画状态；显式 false 在该帧固定为输入值并清除键。`set_modifier_curve` 接受同样的 parameter、合成 frame 和已有 `Easing` 对象；该帧必须有键及后续相邻键。

`set_trim` / `set_dashes` 的对象为 null 时移除能力；设置虚线必须先有描边。锁定、参数缺失、越界和资源超限沿用结构化宿主错误响应；命令批次保持原子提交。采样响应 `vector_layers[].modifier_parameters` 提供当前帧值，`paths` 保持编辑源几何。

## Android 前端与保存

“形状／路径 → 路径操作”提供修剪开始、结束、偏移及多路径模式。“样式 → 描边 → 虚线描边”提供线段／间隔组和偏移。复用 NumericWheel、InputDialog、CurveEditor、现有时间轴以及手势事务；一次连续拖动对应一次撤销。颜色控件沿用原组件。

启用任一修饰能力时，工程格式升级到 11；无新能力的旧工程不主动升级或插入空字段。格式 1–10 仍可读取，格式 11 的旧版本宿主应拒绝打开。复制、保存、工程打包与冻结导出保留全部轨道。

## 验证与兼容状态

回归覆盖长度分配、非均匀参数化三次曲线、闭合起点、偏移、端帽、多路径、原子撤销、局部时间、保存恢复、缓存和 wgpu/GLES 未编码输出。用例位于 `motion-core/tests/vector_path_ops.rs`、`motion-render/tests/vector_adjustment.rs`、Android `VectorPathOperationsTest.kt`。

行为参考 [Adobe 形状属性与路径操作文档](https://helpx.adobe.com/after-effects/desktop/drawing-painting-and-paths/shapes-and-shape-attributes/shape-attributes-paint-operations-path.html)。当前实现属于独立实现；未取得 AE 版本固定的像素对照验收，不计为已验收的 AE 还原项。
