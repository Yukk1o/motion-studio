# 编辑核心接口

独立音频的异步 URI 导入、波形、PCM 及冻结混音接口见 [音频后端](../aem-media/README.md)。

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

新格式版本为 3。缺省 timeline 表示全合成区间、偏移 0。版本 1 工程在内存中
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

## 显式开启图层 3D 与交叉遮挡

新工程格式为 3。新图层的 `three_d` 缺省为 false，包括空对象。用户选择后调用：

```json
{"op":"set_layer_3d","composition":"comp-main","object":2,"enabled":true}
```

开关只改变图层模式，可撤销、重做、批量原子提交。锁定图层拒绝修改，
摄影机不使用此开关。关闭时保留位置 Z、旋转 X/Y、缩放 Z 的值、关键帧和
独立轴曲线；这些分量暂不参与本图层的局部变换，再开启时恢复求值。
2D 只使用位置 XY、旋转 Z、缩放 XY，以固定正交画布投影显示，摄影机和观察
视角不改变其投影。父级关系仍保留：2D 子层使用父级世界矩阵的 XY 投影，
因此 3D 父级倾斜仍可投影为子层缩短/倾斜，但不让子层获得三维深度。
非破坏移动、裁剪、分割、复制、保存和冻结采样都保留模式。

旧版本 1/2 的图层全部按其原有 3D 语义在内存中迁移到版本 3，加载不覆盖文件。
这能保留既有镜头与父级效果。之后新加图层仍默认 2D，不继承旧工程的默认值。
空间练习模板显式声明其示例图层为 3D，空工程不创建摄影机。
`project.layers[]`、`sampledLayers[]`、`timeline_layers[]` 都返回 `three_d`。
能力字段 `capabilities.layer_3d` 包含 default:false、activation:explicit 和命令名。

绘制按图层栈从后到前合成。一个实际参与绘制的 2D 图层形成合成边界，其两侧
的连续 3D 组各自求遮挡，不跨越该 2D 图层重新排序。在一个 3D 组中，BSP
沿平面交叉线拆分凸多边形，保留连续 UV，远到近绘制各片段。透视模式按
眼点所在的平面侧排序；顶视/侧视正交模式按平行视线方向排序，避免大平面
中心越过有限眼点后将可见部分的遮挡画反。`CameraPose.projection` 显式区分
这两种投影，不改变工程数据或 JNI 二进制布局。
实色、半透明、带透明区域的图片/文字使用同一预乘透明度路径；不再依靠整张
图层中心排序。共面区域按用户图层顺序合成。零缩放的空间平面不参与绘制。
当前支持平面素材；真实模型/体积、逐像素光照和立体网格尚未提供。

每次最多 8,192 个片段节点、131,072 个内部顶点、65,536 个输出顶点。
超过预算明确报错，不降级到错误的中心排序。Scratch 在帧间复用；20 个带
独立轴动画和相交平面的场景，预热后半帧采样与拆分均检查零 CPU 分配。

前端可增加以下声明调用新的后端接口；本 PR 不改界面或既有前端桥接文件：

```kotlin
package com.motionstudio.editor
object GeometryBridge {
    init { System.loadLibrary("motion_engine") }
    @JvmStatic external fun sampleGeometryInto(
        id: Long, frame: Double, parameters: java.nio.ByteBuffer,
        vertices: java.nio.ByteBuffer
    ): String
    @JvmStatic external fun hitCandidates(id: Long, x: Double, y: Double): String
}
```

`sampleGeometryInto` 以摄影机视角、精确合成帧（可为小数）采样。两个 buffer 必须
是独立、可写的 direct ByteBuffer，以 nativeOrder 读写，必须有足够容量且不可重叠。
失败通过现有 `{ok:false,error}` 返回，不部分写入 buffer。复用容量上限分别为
`8192*128` 和 `65536*20` 字节；返回 `{ok:true,data:{batches,vertices,
parameterBytes,vertexBytes,batchStrideBytes:128,vertexStrideBytes:20}}`。

每批参数是 32 个 f32（128 字节），所有批次已经按正确顺序排列：

| f32 索引 | 内容 |
| --- | --- |
| 0–15 | 列主序 view_projection；顶点已为世界坐标，不再乘图层 model |
| 16–19 | 线性色彩 RGB + 原始 alpha |
| 20–21 | 原始图层宽、高（新顶点着色器不以此缩放顶点） |
| 22 | 图层 opacity |
| 24 | 纹理序号，0 为白纹理，正值为 project.assets 下标 + 1，视频为负实例槽，见 [视频 API](../aem-media/VIDEO.md) |
| 25–26 | firstVertex、vertexCount（GL_TRIANGLES） |
| 27–28 | 原始栈序号、three_d（0/1） |
| 其他 | 保留，当前为 0 |

每个顶点是 5 个 f32：世界 position XYZ、UV；步长 20 字节。着色器以
`view_projection * vec4(position,1)` 投影。WebGPU 的深度范围是 0..1，
GLES 消费时执行 `clip.z = clip.z*2 - clip.w`。纹理资源仍通过 `assetPixels`
一次读取，RGB 已在线性空间预乘 alpha。片段输出与现有 plane shader 相同，
使用 ONE / ONE_MINUS_SRC_ALPHA 混合。静态图片只传输一次；动态视频按实例取帧并上传，见 [视频 API](../aem-media/VIDEO.md)。

冻结导出应在独立 `NativeBridge.create(root,frozenProject)` 会话中复用这些
buffer。原 `sampleInto` 仍支持未拆分的四边形，遇到交叉拆分返回 -1，防止
旧导出器继续写出错误遮挡。**前端的 MP4 导出器需接入新几何 API 后才支持
交叉平面导出**；本 PR 只完成后端提供与预览/PNG 渲染。

`hitCandidates` 输入画布像素，返回 `{candidates:[{id,uv,depth,order,three_d}],
coordinates:"composition_pixels",selection:"geometry_bounds"}`，从最前方开始。
它按点击处的平面交点排序，包含透明区域的几何边界，不读回纹理 alpha。
前端可以据此选择图层或展示候选列表。投影框继续来自 `projectedLayers`。

检查：`layer_dimensions.rs`、`plane_intersections.rs`、`hot_path.rs`，以及 Android
`Layer3dApiTest`（真实 JNI、PNG、候选顺序、buffer 验证和冻结几何采样）。
