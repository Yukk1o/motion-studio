# Motion Studio · 视频后端 API

异步导入工程内视频源文件，按实际 PTS 解码画面，并默认保留原声。视频作为一个时间轴对象参与变换、2D/3D、父子级、移动、非破坏裁剪、分割、复制、撤销、保存和备份。画面和原声使用同一源偏移与合成时钟，不分别把首帧、首个声音包归零。

生产界面已接入视频导入、动态画面、原声播放和带声音 MP4 导出。添加图层中的“视频”通过系统文件选择器导入；用户可明确关闭原声。打开工程时准备素材缓存，失败会显示具体原因并提供重建入口。

## 输入与资源范围

| 项目 | 支持范围 |
| --- | --- |
| 容器与画面 | MP4，H.264 Baseline / Main / High，8 位 4:2:0 SDR，方形像素 |
| 原声 | 一条选定的 AAC-LC 音轨；44.1 / 48 kHz，单/双声道；可显式丢弃 |
| 显示尺寸 | 每边不超过 1920，像素不超过 1920×1080；支持 0/90/180/270 度旋转 |
| 时间 | 最多 1 小时、标称最多 120 fps、最多 500,000 个不同 PTS；支持 VFR |
| 源素材 | 单文件最多 512 MiB，工程源文件累计最多 4 GiB |
| 导入 | 64 KiB 分块复制；最多两个未结束视频任务/会话、64 个任务记录、两个导入线程/进程 |
| 解码 | 最多四个实例/读取器、四个同时工作的 MediaCodec/进程；每实例一个待处理目标和一帧缓存 |
| 冻结读取 | 最多四个冻结视频句柄，需主动释放 |

探测内容而非后缀/MIME；当前要求 `ftyp` 为首个 MP4 box。加密、HEVC、HDR、非方形像素和不支持的声音编码报错，不静默产生成功的空白片段。`with_audio:false` 可以明确导入无原声画面。

NDK MediaExtractor/MediaCodec 在后台线程使用。按实际帧时间索引选择 `pts_us <= target < end_us`，向后或大幅跳转时从前一个同步帧继续解码到目标 PTS，保留 B 帧顺序和源帧率。优先从 YUV420 ImageReader 读取，兼容标准 I420/NV12 byte buffer，再尝试 RGBA8888/RGB565；未知厂商布局报错。补读 SPS/VUI 的颜色标记，避免设备漏报矩阵导致色差；YUV 按 BT.709/BT.601、full/limited 转换。RGB565 是设备的低精度兼容路径，`source_transfer` 会明确返回，不能视为完整 8 位输出。每次取帧返回实际解码器名称；`decode_us` 是成功解码及 RGBA 转换耗时，不含排队、打开、索引加载和格式重试，调用方应另测端到端耗时。MuMu 数据不代表真机性能。

## 导入与缓存

使用 [音频 API](README.md) 中的 `MediaBridge.request`，返回统一的 `ok/data` 或 `ok/error` 包装：

```json
{"op":"import_media","kind":"video","request_id":"video-1","uri":"content://selected/video","at_frame":30,"name":"片段","with_audio":true}
```

`track` 可指定零起始视频轨道，`audio_track` 可指定零起始声音轨道；缺省各选第一条支持的轨道，声音默认开启。`metadata.audio_tracks` 返回声音轨道列表；`supported` 是格式初筛，最终以实际 AAC 解码成功为准。`probe_media` 使用相同参数与校验路径，不添加图层或素材。

轮询 `media_status`，导入阶段为 `opening / copying / probing / decoding_audio / waveform / awaiting_commit`，进度只针对当前阶段。到 `ready` 后调用 `finish_media_import`；保存工程、素材与图层成功后只产生一次撤销记录。`edit_result` 返回 `asset / object / audio_asset / has_audio / in_frame / out_frame / source_offset_us / truncated_to_composition`。失败和取消清理暂存，不改变已有工程。`cancel_media_import`、`release_media_task`、会话切换与音频 API 具有相同规则，请求 ID 在两类任务之间也不能重复。

视频素材与原声素材具有不同 ID，引用**同一个**工程内 MP4；原文件权限失效不会影响后续读取。完整声音 PCM 与波形保存在磁盘，沿用音频后端。没有整段视频解码或每帧烘焙，源文件之外的画面缓存只保存 PTS 索引与第一帧 PNG。

```json
{"op":"video_thumbnail","asset":5}
```

返回 `path / source_time_us / width / height`，是实际首帧，已应用旋转。备份只包含工程与源素材，不含缓存。恢复备份后分别重建视频缓存及关联声音缓存：

```json
{"op":"prepare_video","request_id":"video-cache-1","asset":5}
```

```json
{"op":"prepare_audio","request_id":"audio-cache-1","asset":6}
```

轮询至 `succeeded` 后读取。缓存重建重新校验源文件及元数据，不修改工程或撤销记录。

## 按实例请求画面

生产 `com.motionstudio.editor.MediaBridge` 提供以下声明：

```kotlin
@JvmStatic external fun readVideoFrameInto(id: Long, objectId: Long, sequence: Long, output: java.nio.ByteBuffer): String
@JvmStatic external fun freezeVideo(id: Long): String
@JvmStatic external fun requestFrozenVideoFrame(handle: Long, objectId: Long, compositionFrame: Double, sequence: Long): String
@JvmStatic external fun readFrozenVideoFrameInto(handle: Long, objectId: Long, sequence: Long, output: java.nio.ByteBuffer): String
@JvmStatic external fun releaseFrozenVideo(handle: Long): String
```

普通会话请求须在创建会话的工作线程调用；原生解码由后台 worker 完成。

```json
{"op":"request_video_frame","object":7,"frame":45.5,"sequence":1}
```

`frame` 是**合成帧**，可为小数。`sequence` 是每对象单调递增的正 signed-64-bit 整数。改变目标时间必须增加序号；轮询同一目标则重复原序号。片段编辑/撤销后先重新请求再读，不能直接读取旧结果。返回 `pending / ready / failed / outside`、`sequence / source_time_us / width / height`。`ready` 额外返回 `pts_us / end_us / decode_us`。新请求会取代旧请求；陈旧序号报错，旧任务不能覆盖新结果。自动预览使用内部 generation，不推进调用方序号；共享实例只保留最新目标，预览转到其他时间后原请求可重新轮询。源画面区间之外返回 `outside`、`bytes:0`，应跳过该图层，而不是循环或保持末帧。

到 `ready` 后调用 `readVideoFrameInto`，传入容量至少为 `width * height * 4` 的可写 direct buffer。从 byte 0 写入 RGBA8，alpha 为 255，不读取 Java position。只读/不足容量/陈旧请求不会写入部分结果。返回实际 `bytes / format:"rgba8" / pts_us / end_us / width / height / source_transfer / decoder / decode_us`，调用方必须使用这些值。相同源的两个片段有独立目标与缓存，不会相互覆盖；同一 PTS 的 GPU 纹理复用，不每帧分配新纹理。

```json
{"op":"release_video_frames"}
```

释放当前会话的解码实例与序号记录。会话关闭/切换或渲染 Surface 分离也释放资源；缓存、源文件保留。播放/跳转中 `pending` 时应继续请求新的目标；暂停时要重试同一目标，不能把 `pending` 当作工程保存失败。原生 WebGPU 预览和 PNG 捕获已使用动态视频帧。

## 原声与时间轴

`set_audio {object,volume?,muted?}` 同时支持视频对象；线性音量 `[0,2]`，默认 1、不静音。隐藏画面与透明度不改变音量。移动、裁剪、分割和复制后原声随同一个片段采样，不从头重播。`timeline_layers[].video` 返回源时间、源偏移、尺寸、时长和是否有原声；`.audio` 返回关联声音 ID、音量/静音及同一派生时钟。

`audio_waveform` 使用关联**声音 asset ID**，原声与独立音频一起进入 `readPcmInto`、`freezeAudio` 和 `readFrozenPcmInto`。输出仍为 48 kHz 双声道 f32le。AAC 编码延迟、裁剪与空白编辑沿用音频后端处理。`AudioPlayback` 将这些 PCM 送入 AudioTrack，播放进度跟随已经输出的声音采样位置。

## 冻结画面与导出接入

会话线程调用 `freezeVideo`，得到 `handle / revision / project`。冻结对象拥有工程快照、独立解码器与游标，编辑、跳转或关闭原会话不改变结果。导出线程按合成时间调用 `requestFrozenVideoFrame`，轮询同一序号到 `ready`，再调用 `readFrozenVideoFrameInto`；最后 `releaseFrozenVideo`。图像 buffer 要求与实时接口相同，当前每次最多取得一个源画面，不是完整合成图。

冻结的 `project` 可用于创建独立几何采样会话，接入 `GeometryBridge.sampleGeometryInto`。几何参数索引 24：0 为白纹理，正值仍为图片下标 + 1，**负值**为视频实例槽 `-(layer.order+1)`。返回 `videoLayers:[{object,asset,texture_slot,source_time_us}]`，将对应实例帧上传至该槽后按既有三角形批次绘制。旧 `sampleInto` 遇到活动视频返回 -1，避免静态导出器误写空白或封面。前端需要同时接入动态画面、几何 API、冻结声音、AAC 编码和统一 PTS 的 MP4 复用，才构成完整带声音导出。

工程格式升级为 v5，加载 v1–v4 时在内存迁移，打开不自动覆盖原文件；旧引擎不能加载新视频工程。源文件保守保留，没有自动垃圾回收。

## 验证

```powershell
cargo test -p aem-core -p aem-media
cargo test -p aem-render --test video_instances
python tools/generate_video_fixtures.py
python tools/build_android.py --task assembleDebug assembleDebugAndroidTest
python tools/validate_video.py --adb <adb.exe> --serial <device> --output <ignored-evidence-directory> --latest-per-fixture
```

`VideoBackendApiTest` 使用真实 URI 偏移、NDK 解码、动态 PNG、原声 PCM、不同时间实例、冻结线程、旋转与 VFR、取消/回滚、缓存重建、direct buffer 和陈旧请求。`validate_video.py` 把真实 JNI 画面与 FFmpeg 在相同源 PTS 的输出比较，报告色差与耗时。Rust 事务测试显式注入探测回调，只验证事务与源文件生命周期；不将它当作真实视频解码证据。测试图像、视频和声音均由本项目算法生成。具体验收数值、设备与限制见 PR；尚未完成不同真机的持续性能、完整播放或 MP4 音画导出验收。
