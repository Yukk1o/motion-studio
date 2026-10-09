# 图层源蒙版与原生编辑

本批提供普通可见图层的源蒙版，作用于效果链之前。支持纯色、图片、视频、文字、矢量和合成引用图层；调整图层的合成区域蒙版、轨道遮罩和图层混合模式是后续能力，不能用源蒙版字段冒充。

## 数据与坐标

`Layer.masks` 是有序数组，单层最多 16 个蒙版、合计 2048 个节点。稳定 ID 在层内唯一。每个蒙版包含 `id/name/path/mode/enabled/inverted/opacity/feather/expansion`。路径使用已有 VectorPath/PathNode 格式；节点 `geometry=[x,y,in_x,in_y,out_x,out_y]`，切线相对节点。

坐标为未变换源图左上角像素，Y 向下；图层变换、父子级和摄影机不改存储坐标。源图边界裁剪羽化。原生钢笔工具使用相同投影／逆投影，界面适配时把中心坐标与源像素坐标互相转换；扩展只改覆盖区域，不改路径或锚点。

- 不透明度：0–100%；羽化：X/Y 各 0–32000 px；扩展：−32000–32000 px。路径数值沿用宿主矢量路径范围。
- opacity、feather、expansion 和节点几何复用已有轨道、缓动和层时间。模式、启停、反转为离散编辑。
- opacity 和 feather 的有限动画采样钳制到物理范围；保存值仍必须合法。非有限值、资源／设备超限明确报错。不能将资源超限解释成静默缩小输出。
- 开放路径、None 模式及停用蒙版保留数据，不影响层 Alpha。可继续编辑钢笔路径，闭合后再参与覆盖。

工程格式为 8；旧工程迁移为空蒙版数组。与独立颜色 PR 的格式 8 扩展集成时，需要同时保留两套字段及相应验证，不以相同版本号代替合并验证。

## 增量命令

```json
{"op":"mask","object":12,"action":{"kind":"add","mask":{"id":1,"name":"蒙版 1","path":{"id":1,"closed":false,"nodes":[]}}}}
{"op":"mask","object":12,"action":{"kind":"options","mask":1,"inverted":true}}
{"op":"mask","object":12,"action":{"kind":"set","mask":1,"property":"opacity","frame":30,"value":[50]}}
{"op":"mask","object":12,"action":{"kind":"animate","mask":1,"property":"opacity","frame":0,"enabled":true}}
{"op":"mask","object":12,"action":{"kind":"set","mask":1,"property":{"node":2},"frame":30,"value":[540,960,0,0,0,0]}}
```

`options` 的 mode/inverted/enabled 可只提交要改变的字段。`set.animated` 可省略，保留后端当前动画状态；显式 true/false 分别开启／关闭动画。不要根据未返回的旧 UI 快照覆盖整份蒙版或轨道，否则连续操作会丢失值和关键帧。

支持 remove、replace、reorder、path、delete_key、move_key、copy_key、curve。replace 用于完整实例替换／导入；普通参数和动画使用增量命令。frame/from/to 使用合成帧，宿主一次转换为图层局部帧。一次参数拖动使用现有手势事务，取消恢复原值；一个批次失败不会部分修改工程。锁定层禁止编辑。

## 预览、导出与资源

mask_layers 状态返回蒙版采样、路径节点及源模型 MVP，界面与渲染共用层时间。原生面板提供矩形／钢笔、节点／曲柄编辑、模式、启停、反转、不透明度、双轴羽化、扩展、复制／排序／删除和关键帧／曲线。

wgpu 和 GLES 使用同一几何、Gaussian 羽化与覆盖合并着色器。缓存 R8 覆盖纹理，静态路径复用三角形，逐帧不重新编译着色器；删除／停用后清理无效资源。源图／覆盖纹理纳入 128 MiB 资源预算，蒙版临时纹理与效果临时纹理共用 64 MiB 上限。

Render plan 版本为 5、头部 128 B；112/116/120/124 是蒙版表偏移、记录数、顶点偏移及 64 B 记录步长。每个记录绑定 draw layer index、ID、尺寸、几何位置与指纹、模式、反转、不透明度和双轴羽化；顶点步长 24 B。JNI renderPlanInfo 返回 maskPrograms、maskBytes、maskVertexBytes、maskSourceToken；GLES 使用共享编译结果。正式输出冻结原有工程与资源引用。

带效果的源在进入效果链前乘蒙版，随后模糊／发光可外扩；最终不能再次乘原蒙版而裁掉外扩。无效果图层直接绘制时读取覆盖，不分配额外 RGBA 效果输入。

## 对照范围

基础模式和图层顺序参考 [Adobe 蒙版说明](https://helpx.adobe.com/after-effects/desktop/work-with-transparency-and-compositing/work-with-alpha-channels-and-masks/alpha-channels-masks-mattes.html)。当前实现的合并公式为 Add=union、Subtract=A×(1−B)、Intersect=A×B、Lighten=max、Darken=min、Difference=A+B−2AB；路径扩展以圆角填充／描边构造，羽化使用分轴 Gaussian。

这些连续 Alpha、扩展和抗锯齿行为尚未完成 AE 原始帧采集，不宣称已与 AE 逐像素一致。验证应分别记录有效区与边缘区；轨道遮罩／调整区域处理会以独立能力和验收用例交付。
