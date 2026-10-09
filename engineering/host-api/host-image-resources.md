# 大图片、按需纹理与预览代理（接口版本 1）

本批基于 `origin/main`，不依赖粒子编辑器或大媒体包 PR。通用前端页面不在本批范围内；现有图片导入与 GLES 导出适配器已接入这些接口。

## 原图与代理

- PNG 导入按 64 KiB 块复制原始文件，保留压缩字节、原始尺寸与透明度。不会通过完整 Android Bitmap 解码、重压缩大 PNG。
- 原图文件上限 64 MiB，尺寸每边 1–16,384。编码大小、完整解码大小、设备纹理尺寸是不同限制。
- 自动、均衡、省电预览使用最长边至多 2,048 的代理，短边向上取整，不放大小图。高质量预览、PNG 捕获、GLES 视频输出使用原图。
- 代理只改变采样精度。`Asset.width/height`、图层局部坐标、锚点、变换、效果参数像素单位与工程序列化均保持原来的含义。放大预览可能看见代理较软；用户可以切换高质量模式。
- 原图不可在同一路径原地修改。替换资源使用新文件路径；可以保留素材 ID。工程/资源目录切换会使相同 ID 的旧纹理失效。
- PNG 支持 RGB、RGBA、灰度、灰度 Alpha、调色板透明度、16 位转 8 位与 Adam7。代理在**线性预乘 Alpha**空间按整数区间做 box 平均，避免透明 RGB 形成边缘色晕。
- PNG 普通扫描线解码只需要代理输出和一行累加器；完整 PNG 输出直接写目标 RGBA 缓冲区。Adam7 代理还需要代理尺寸的四通道浮点累加器，2048² 时为 64 MiB，另加至多 16 MiB 输出与 16 MiB 解码器预算。
- 既有 JPEG 资源仍可读取，但 JPEG 解码器需要完整源图，其解码预算为 128 MiB。Android 的非 PNG 导入继续走原有归一化 PNG 路径，Bitmap 预算仍为 64 MiB，保留 ImageDecoder 的方向处理。

## 工作集与加载时机

素材表始终为 `[0, ...project.assets 的 ID]`。纹理是否驻留不改变渲染计划中的素材槽位。

当前采样场景及其所有子合成中的图片、文字栅格素材形成工作集。排除未引用素材与不在当前片段内的图层；保留离屏图层，因为效果可以移动其显示范围，光效也可能需要其遮挡信息。多个图层或子合成实例引用同一素材时共用一份静态纹理。

打开、编辑资源、撤销和切换工程仅登记素材，不会解码或上传全部图片。预览每个 Renderer 同时最多一个后台解码任务，拥有 GPU 的线程轮询并上传结果。稳定工作集不重新解码或上传静态图片。图片和视频请求在同一次 render 内同时发起，不会等待图片完成后才启动视频解码。首个缺图帧返回 `render=false`，保留最后已呈现画面，前端继续原有 dirty 重试。过期结果不会装入新的工程或错误的清晰度模式。

自动、均衡、省电预览保留近期闲置图片纹理，按最近使用顺序淘汰，上限 **32 MiB**。这部分与活动图片、视频、子合成纹理和插件资源**共用既有 128 MiB 预算**，不会额外扩大预算。活动内容需要空间时先回收闲置图片；活动素材不会被缓存策略淘汰。同一 Renderer 倒序寻帧时，驻留命中不读文件、不解码、不上传。高质量预览、正式捕获仅保留当前原图工作集，不保留闲置代理。不同 Renderer 不共用 GPU 纹理。

预览完成当前画面的资源准备及呈现后，按片段起点预取未来 **0.5 秒**内最多 **2 项**图片依赖，支持文字栅格素材和子合成时间/帧率映射。仅在闲置预算与剩余资源预算都允许时预取，不淘汰近期纹理来给预取让位。预取只检查静态片段依赖，不额外采样表达式或推进粒子时间；暂停画面也可以准备附近片段。单次 render 至多启动一个后台任务，下一次 render 轮询结果。

当前帧缺图、寻帧、清晰度切换、工程/资源切换会取消不再需要的任务。普通 PNG 按行、源 SHA-256 按 64 KiB 检查取消；保持至多一个任务，不通过丢弃句柄并启动第二线程来抢占。JPEG 内部解码及单次 I/O 不能立即中断，完成当前步骤后检查取消。后台任务销毁不会等待线程。预取失败不会影响当前画面，也不会逐帧重试同一坏资源；资源真正进入当前工作集后仍必须报告解码错误。

`.cache/image-proxies-v1/` 保存可丢弃的预乘 RGBA 代理，上限 64 MiB（单进程写入串行、按文件生成时间淘汰）。键包含完整源文件 SHA-256、代理尺寸及算法版本，文件有独立内容校验；损坏缓存会重建。缓存 I/O 失败不影响源图解码，缓存不进入工程包。未驻留素材的缓存命中仍在后台计算源 SHA-256，不能把磁盘缓存命中当成零耗时。

活动纹理沿用 128 MiB 资源预算和设备尺寸限制；效果 scratch 仍单独限制 64 MiB。正式输出遇到合法原图超限明确失败，**不会把代理用于正式导出**。这不等于实现了任意大图的分块正式渲染。

## JNI

### Stateless：可在导入 I/O 线程调用

```kotlin
NativeBridge.imageInfo(absolutePath: String): String
NativeBridge.prepareImage(root: String, path: String): String
```

`imageInfo` 只检查文件大小、格式和尺寸；不保证压缩像素完整。`prepareImage` 的 `path` 必须是工程内 `assets/...` 相对路径；它验证像素解码并生成/复用预览代理。失败后不要注册素材，删除调用者自己创建的导入文件。

请求示例：

```kotlin
val info = nativeData(NativeBridge.prepareImage(projectRoot, "assets/import-uuid.png"))
val asset = JSONObject().put("id", assetId)
    .put("path", info.getString("path"))
    .put("width", info.getInt("width")).put("height", info.getInt("height"))
// 在会话所属线程、同一合成中批量 register_asset + add。
```

成功响应（外层沿用 `ok/data`）：

```json
{"ok":true,"data":{"version":1,"path":"assets/import-uuid.png","width":7952,"height":3273,"format":"Png","bytes":20770401,"validated":true,"proxyWidth":2048,"proxyHeight":843,"proxyCached":false}}
```

切换工程或关闭会话期间，前端必须检查导入时捕获的工程目录与 generation，不能将旧导入提交给新工程。现有导入适配器已有该检查。

### Session：必须在创建会话的工作线程调用

```kotlin
NativeBridge.assetPixelsInto(id: Long, asset: Long, buffer: ByteBuffer): String
```

缓冲区必须是可写 DirectByteBuffer，容量至少 `width * height * 4`；JNI 从**地址起点**写入，忽略 position/limit。只写声明图像字节数。预检失败不会开始解码；解码失败可能留下部分像素，调用者必须丢弃该结果。成功数据为：

```json
{"version":1,"asset":7,"width":7952,"height":3273,"bytes":104107584,"resolution":"original"}
```

像素为 sRGB 编码、在线性空间预乘 Alpha，匹配既有 WGPU/GLES 合成契约。输出仍受单图 128 MiB 限制。旧 `assetPixels` 保留兼容，但 GLES 导出已使用本接口，避免额外整图 Java ByteArray。

`renderPlanInfo.data.imageResources` 声明 `version=1`、`demandLoading=true`、`previewMaxEdge=2048`、`fullResolutionExport=true`、`directBuffer=true`。

`renderPlanInfo.data.assetBytes` 现在为初始化时白色纹理的 4 字节，`declaredAssetBytes` 为全部素材原图 RGBA 大小。前端不能用 declared 总量否定工程导出，必须根据实际帧计划在活动工作集上检查预算。

GLES `EglMovieRenderer.prepareBundle` 递归检查 bundle 内各个计划，加载需要的原图，保留素材槽位、去重并释放失效纹理。调用順序保持 `sampleFrameBundleInto → prepareBundle → uploadVideo → drawBundle`。不能在逐帧函数外预先加载整个素材库。

`previewInfo` 提供 `imageResolution`、`imageDecodes`、`imageProxyCacheHits` 与 `imageUploadBytes`，并新增：

| 字段 | 含义 |
| --- | --- |
| `imageMemoryCacheHits` | 重新进入活动工作集时复用驻留图片的次数，按素材去重 |
| `imageIdleBytes` | 当前闲置图片 GPU 纹理字节数，不含活动图片 |
| `imageIdleBudgetBytes` | 闲置上限，当前为 33,554,432 字节；资源压力下可以更少 |
| `imagePrefetches` | 启动的预取任务数，包括被取消或失败的任务 |
| `imagePrefetchSeconds` | 预取时间窗口，当前为 0.5 秒 |

计数为当前 Renderer 累计，不是设备峰值内存或完整播放帧率。`imageDecodes`、`imageProxyCacheHits` 统计成功装入纹理的解码/磁盘命中；丢弃结果不计入。加载与缓存自动接入现有 render/dirty 流程，前端无需提交额外预取请求。

## 前端交接

现有导入按钮、选择状态和属性面板布局无须改变。新前端接入时复用 `ImageImport.prepare` 的流式复制/验证流程，并维持原始宽高。PNG 尺寸超过原 Bitmap 预算时，应允许尝试代理预览；正式导出若超设备能力，展示后端给出的资源错误。高质量预览也可能因原图资源限制而失败，不能以自动预览成功推断导出必然成功。
