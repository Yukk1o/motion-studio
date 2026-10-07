# Motion Studio 子合成与预合成：前端接入契约 v1

此接口由原生运行时和 Kotlin 适配层提供，编辑器已接入合成管理、预合成、面包屑、片段与设置页面。入口为
`CompositionBridge.request(engine, json)`，必须在引擎所属线程调用。
工程格式为 7；单节点效果计划为 v4，嵌套 GPU 执行包为 v1。
预渲染尚未实现，`prerender=false`。子合成接口不依赖预渲染。
测试范围、结果与设备边界见 [验收记录](compositions-validation.md)。

## 能力、身份与作用域

`NativeBridge.state` 的 `capabilities.multiple_compositions`、`precompose` 为 true，
`composition_api.version=1`。`composition_api` 还声明深度、数量、首期限制及
`reference_3d=true`、`collapse_transformations=false`。

`comp-main` 是主合成的稳定 ID，其他 ID 是不透明字符串。ID 在保存、重新打开和
撤销/重做中保留。图层 ID **只在所属合成内唯一**，素材 ID 在图片、音频、视频三张
共享资源表中全局唯一。前端不能把两个合成的同号图层当成同一对象。

状态中的 `project` 是当前打开合成的可编辑视图，同时包含完整 `compositions` 图和
共享素材表。`composition` 是当前合成 ID；`compositions` 提供所有节点摘要。
`main_composition` 恒为 `comp-main`，不能将它用作所有编辑请求的固定目标。

所有 v1 请求包含 `version:1`、`composition` 和 `op`。
响应成功为 `{ "ok":true, "data":... }`；失败为
`{ "ok":false, "error":"...", "error_detail":{ "code","composition","message","details" } }`。
查询其他合成可直接使用其 ID；涉及当前预览、编辑器和媒体任务的操作先打开该合成。
作用域不匹配会拒绝操作，不自动把内容添加到当前另一个合成。

## 列表、打开与上下文

```json
{"version":1,"composition":"comp-main","op":"list"}
```

返回 `compositions[]`：`id/name/width/height/fps/frames/main/references/children/next_layer_id`，
以及 `revision`。`references[]` 是引用此节点的 `{composition,object}`；
`children[]` 是此节点引用的 `{composition,object}`。`info` 返回一个摘要。

```json
{"version":1,"composition":"comp-1","op":"open","path":["comp-main","comp-1"]}
```

返回完整编辑状态：`project`、`sampledLayers`、`sampledEffects`、`timeline_layers`、
`timeline_camera`、采样摄影机、`frame`、`revision`、`canUndo/canRedo`。
`composition_context` 包含 `selection`、`timeline` 和 `path`，供面包屑使用。
`path` 的相邻节点必须存在实际引用；省略时表示直接打开该节点，路径为 `[id]`。
同一子合成可以有多个父级，所以不能假设只有一条父路径。

```json
{"version":1,"composition":"comp-1","op":"context","selection":[2,3],"timeline":{"zoom":2,"scroll":48}}
{"version":1,"composition":"comp-1","op":"seek","frame":12.5}
{"version":1,"composition":"comp-1","op":"state"}
```

时间轴对象由前端定义，原生只保存该 JSON；选择会校验所属合成。
每个节点保存本次会话中的时间、选择、时间轴与返回路径。它们是会话状态，不写入工程。
编辑或历史改变图结构后，返回状态过滤已删除的选择；失效面包屑退回当前节点路径。
切换合成不产生撤销记录；历史是**整个工程一条栈**，不能为每个节点伪造独立历史。
正在拖动参数或打开插件专属编辑器时，应先提交/取消拖动并关闭插件编辑器。

## 创建、引用和预合成

```json
{"version":1,"composition":"comp-main","op":"action","action":{"kind":"create","settings":{"name":"子合成","width":1920,"height":1080,"fps":30,"frames":300}}}
{"version":1,"composition":"comp-main","op":"reference_candidates"}
{"version":1,"composition":"comp-main","op":"action","action":{"kind":"reference","target":"comp-1","at_frame":30}}
{"version":1,"composition":"comp-main","op":"action","action":{"kind":"precompose","objects":[2,3],"name":"预合成","range":"composition"}}
```

空节点采用透明背景。新引用使用子合成尺寸，居中，变换为单位变换；默认二维。
引用片段从 `at_frame` 开始，到父合成结束；超出子合成源时长的部分透明、静音。
候选列表已经排除自引用、循环、深度/实例数量或图层数量超限的目标。

编辑响应带 `edit_results[]`；其中 `op="composition"`，`result` 例如：

```json
{"kind":"precompose","source":"comp-main","composition":"comp-1","object":4,"selection":[4],"layer_mapping":[{"source_composition":"comp-main","source_object":2,"composition":"comp-1","object":2},{"source_composition":"comp-main","source_object":3,"composition":"comp-1","object":3}]}
```

图层按原栈顺序移动，引用插入原选区的位置。所选图层的 ID、片段起点、偏移、关键帧、
缓动、效果、允许的表达式和选区内父子关系不变。子合成沿用父合成的尺寸、帧率和完整
时长，背景透明；一次操作只产生一次全工程撤销记录，失败不会移动部分图层。

首期仅支持“移动所有属性”，范围为完整合成，限制如下：

| 情况 | 行为 |
| --- | --- |
| 非连续栈位置 | `noncontiguous_selection`，必须重新选择连续区间 |
| 锁定图层 | `locked_layer`，错误详情包含图层 ID |
| 选中摄影机或重复 ID | `invalid_selection` |
| ID 不属于源合成 | `cross_composition_selection` |
| 父子级跨选区或摄影机依赖所选父层 | `external_parent` |
| 所选图层为 3D | `unsupported_mode`，首期不自动转换或烘焙摄影机 |
| 所选图层有场景生成器 | `unsupported_mode`，避免改变外部场景依赖 |
| 未选中效果引用所选图层 | `external_reference` |
| 任意启用表达式源码包含 `index` | `expression_context`；不重写源码，禁用的草稿保留 |
| 不同片段起点、负本地关键帧 | 原样保留，使用完整合成时钟 |

`thisComp` 的尺寸、帧率、时长以及表达式 `time` 在本次全范围预合成中不变。
`index` 检查采用保守文本检查，可能拒绝字符串/注释中出现该词的表达式。
不支持“只移动内容、保留属性”、选区裁切范围、时间重映射、折叠变换或跨图层表达式引用。

## 片段时钟、3D 与音频

引用内容为：

```json
{"kind":"composition","clip":{"composition":"comp-1","source_start_frame":0,"volume":1,"muted":false}}
```

若父合成采样帧为 `F`，图层片段 `offset_frame=O`，父/子帧率为 `P/C`，则：

`child_frame = source_start_frame + (F - O) × C / P`。

保留小数帧；不做先取整再采样。片段、子合成有效范围均左闭右开，
`child_frame<0` 或 `>=child.frames` 时透明、静音，不冻结首尾帧，不循环。
30 fps 父合成的 10.5 帧对应 60 fps 子合成的 21 帧（偏移为零）。

```json
{"version":1,"composition":"comp-main","op":"action","action":{"kind":"set_clip","object":4,"source_start_frame":12,"volume":0.5,"muted":false}}
```

源偏移使用子合成帧数，范围 ±36,000；音量 0–4。图层移动/裁剪/拆分复用已有命令，
子合成内部动画与表达式按其自身帧率采样。音频使用有理数累计各级时间，最后在
48 kHz 的绝对采样边界向下取整，再与父/子片段及子合成时长相交；不会先用
`48000 / fps` 的整数结果逐帧累加。多实例各自保留时间和音量。参考图层的透明度与视觉
可见性不控制音量，与现有音频行为一致；静音使用 `clip.muted`。

引用图层和图片一样，可以通过已有命令切换 3D：

```json
{"op":"set_layer_3d","composition":"comp-main","object":4,"enabled":true}
```

此时子合成先由自身摄影机绘制为平面，再由父合成摄影机投影；支持 Z 位移、X/Y 旋转、
缩放、不透明度、父子级和图像效果。不会把子合成内部的 3D 图层展开到父合成空间。

## 所有领域操作的合成 ID

新的 Kotlin 适配入口均要求显式 `composition`：

| 方法 | 用途 |
| --- | --- |
| `CompositionBridge.command(id, composition, JSONObject)` | 图层/片段/关键帧/曲线/效果/表达式编辑，注入路由 ID |
| `CompositionBridge.media(id, composition, context, JSONObject)` | 导入、素材/帧/波形查询；校验当前上下文 |
| `CompositionBridge.plugin(id, composition, JSONObject)` | 效果目录及插件编辑器；校验当前上下文 |
| `CompositionBridge.history(id, composition, op)` | 复用 0 撤销、1 重做及已有拖动事务操作 |
| `CompositionBridge.render/seek/drag(id, composition, ...)` | 播放、定位和预览拖动；render 不生成完整状态 JSON |
| `CompositionBridge.readPcmInto/freezeAudio/freezeVideo(id, composition, ...)` | 实时音频及独立冻结媒体读取器 |
| `CompositionBridge.capture(id, composition)` | 严格 PNG 输出，递归检查插件与读取视频 |
| `CompositionBridge.freezeProject(id, composition)` | 获取完整不可变 JSON，传入已有 `VideoExporter` |
| `CompositionBridge.sampleFrameBundleInto(...)` | GLES/自定义导出适配器读取分层执行计划 |

旧入口保留供单合成调用方迁移。**原始编辑命令省略 `composition` 时仍指向
`comp-main`，不是当前页面。** 前端必须把活动合成 ID 接入所有编辑入口，包括图片、
文字、效果与表达式。旧直接预览/PCM/几何查询依赖已打开的上下文；不得混用其他节点的
图层 ID。PNG、MP4 的新适配方法先校验上下文，导出 JSON 中的当前节点就是导出根节点。

异步媒体导入在提交任务时绑定合成。打开其他节点不会把任务结果导入错误的节点；
`finish_media_import` 需要回到绑定合成，否则返回 `context_mismatch`。
`release_media_task` 清除任务绑定。素材路径和 PCM 缓存是工程共享资源。
播放音频应使用状态里的 `has_audio`；不能只扫描当前 `layers` 是否含音频图层。

## 现有合成设置：先预览影响，再确认提交

```json
{"version":1,"composition":"comp-1","op":"settings_preview","settings":{"name":"更新的子合成","width":1280,"height":720,"fps":60,"frames":600,"timing":"preserve_seconds","shorten":"reject"}}
```

预览不修改工程，也不产生撤销。返回 `valid`、`expected_revision` 和 `results`；
可应用时 `results[].result.impacts[]` 提供重定时关键帧、裁剪片段、删除图层、
摄影机关键帧、保留但超出有效时间的本地关键帧以及受影响的引用层。
不可应用时返回 `error` 与结构化 `error_detail`；缩短失败的详情同样包含影响列表。

前端展示这些影响，让用户确认后提交**同一份设置**和版本号：

```json
{"version":1,"composition":"comp-1","op":"settings_apply","expected_revision":7,"settings":{"name":"更新的子合成","width":1280,"height":720,"fps":60,"frames":600,"timing":"preserve_seconds","shorten":"reject"}}
```

任何编辑造成 revision 变化时拒绝陈旧请求（`stale_revision`），需要重新预览。
宽高范围 1–8192，整数帧率 1–240，帧数 1–36,000，名称最多 1024 UTF-8 字节。
尺寸改变保留像素位置和变换，不自动居中或缩放内容；引用层基础尺寸同步更新。
延长合成时长保留已有图层的原出点，不自动拉长片段，新增时间区域可能为空。

| 策略 | 行为 |
| --- | --- |
| `timing=preserve_seconds`（默认） | 片段和关键帧按新/旧 fps 比例取最近整数帧；引用此节点的源帧偏移同步换算 |
| `timing=preserve_frames` | 所有帧编号不变，播放秒数随 fps 改变；源偏移保持帧编号 |
| `shorten=reject`（默认） | 若新时长截断片段或摄影机关键帧，拒绝并返回影响 |
| `shorten=trim` | 明确裁剪片段尾端、删除完全位于结束之后的图层、移除超时的摄影机键；仍拒绝移除必需父级 |

60→30 fps 导致两帧关键帧合并时返回 `keyframe_collision`，不静默覆盖；可以选择保留
帧编号。图层/效果的本地关键帧保留，即使暂时位于片段之外；摄影机使用合成时间，其
超时键在明确 trim 后删除，可能改变末段插值。表达式源码中的数字不会自动重写。
引用边界和时间变化会影响父合成画面，影响列表列出所有入站引用。

## 删除、存储、资源与错误

```json
{"version":1,"composition":"comp-1","op":"delete_check"}
{"version":1,"composition":"comp-main","op":"action","action":{"kind":"delete","target":"comp-1"}}
```

删除仍被引用的节点返回 `composition_in_use` 和引用层列表。主合成及当前打开节点不能
删除；先返回另一个合成。删除引用图层不会自动删除子合成或共享素材。

保存和 `.aem` 打包均将 `comp-main` 写为顶层，完整保存其余节点和共享媒体资源；
无论保存时打开的是哪个节点，重新打开都从主合成进入。格式 1–6 的单合成工程自动
迁移为格式 7，内容与动画保留，原文件在加载时不覆写。格式 6 的矢量与调整图层
保留内容、动画和效果，可以放入子合成后继续编辑。

数量上限 32 个节点、嵌套深度 8（包含根）、一个根的展开实例 64（重复引用分别计数）、
每节点沿用 128 图层限制。活动视频解码实例最多 4。素材、视频平面及子合成输出共享
128 MiB GPU 资源预算，效果临时纹理仍为 64 MiB；执行包最多 32 MiB。
达到资源/设备尺寸限制时明确失败，不降低参数或裁剪输出。

常见结构化代码：`composition_missing`、`cycle`、`resource_limit`、`invalid_range`、
`invalid_settings`、`unsupported_mode`、`invalid_selection`、`external_parent`、
`external_reference`、`expression_context`、`keyframe_collision`、`context_busy`、
`context_mismatch`、`stale_revision`、`invalid_path`、`sample_failed`、`invalid_request`。
普通 JNI 旧接口仍可能返回原有字符串错误；v1 图编辑错误保证提供 `error_detail`。
正式输出递归检查所有可达节点中启用效果的依赖，失败时阻止输出；预览沿用失败效果旁路。

## GPU 执行包 v1

`sampleFrameBundleInto(id,composition,frame:Double,buffer)` 使用可写 DirectByteBuffer、
小端字段，返回写入字节数；容量不足返回 `-requiredBytes`，错误返回 -1。
先使用 128 KiB 缓冲，按返回值扩容，再复用；最大 32 MiB。-1 时读取 `renderError`。
旧 `sampleRenderPlanInto` 对活动嵌套画面明确报错，请使用新执行包。

头 32 字节：8 个 u32：magic `0x4243534d`、版本 1、节点数、总字节数、根节点索引、
目录起点 32、节点步长 80、视频步长 40。目录按后序排列，子节点先于父节点。

节点目录：偏移 0 u64 引用渲染实例 ID；8 u32 父目录索引（根为 `0xffffffff`）；
12 i32 父节点动态纹理槽；16/20 u32 宽高；24 f32[4] sRGB 背景；40/44 u32 单节点计划
起点/长度；48/52 u32 视频表起点/数量；56 f64 采样帧；64 u32 fps，其余保留。
单节点数据是 v4 效果/几何计划（112 字节头、128 字节 draw、独立几何区）；draw word 30 表示源类型：0 静态、1 视频、2 子合成。draw word 31 为调整图层/矢量标记，矢量几何和调整累计画面沿用单合成协议。

视频目录：0 u64 渲染实例 ID；8 u64 共享素材 ID；16 u64 源时间微秒；24 i32 当前节点
动态纹理槽；28/32 u32 素材显示宽高；36 保留。动态槽为 `-(图层栈位置+1)`。
子节点渲染实例 ID 是临时标识，**不能作为编辑图层 ID 保存**；同一子合成的多个实例
有独立的输出纹理和视频时间。纹理保留在 GPU，既不返回整帧 JSON 像素，也不进行
子合成 CPU 读回。GLES 导出已实现此执行包，包含子节点背景、3D 平面、效果和音视频。

冻结视频的新接口：`requestFrozenCompositionFrame(handle,frame,sequence)` 请求根的
整组嵌套视频，待 `state=ready` 后用
`readFrozenCompositionVideoInto(handle,renderInstanceId,sequence,buffer)` 读取源视频
像素。同一 sequence 不能用于不同帧；过期请求被拒绝。冻结音频接口无需改变，混音器
现在递归展开引用，并保持独立文件游标。

## 前端接入顺序与后续能力

1. 维护活动合成 ID，接入列表、创建、引用、返回路径和每节点选择/时间轴恢复。
2. 多选预合成提交完整 ID 集合，显示明确限制，成功后采用后端返回的新选择和映射。
3. 所有图层、媒体、效果、表达式操作改用显式 ID；播放读取 `has_audio`。
4. 设置页接入影响预览、确认及陈旧 revision 重试。
5. 导出冻结当前节点的完整 JSON，使用更新后的 VideoExporter；PNG 使用 capture 适配。

后续独立能力：折叠变换、保持属性预合成、3D 选区摄影机保持、时间重映射、自动处理
非连续选区、跨图层表达式引用以及预渲染任务（范围/规格/缓存键/进度/取消/失效规则）。
