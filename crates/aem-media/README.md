# Motion Studio · 音频后端

独立音频的 URI 导入、工程内源文件、片段编辑、真实波形和确定性 PCM 混音。纯音频是时间轴对象，没有图片尺寸、GPU 纹理或空间父子关系。

视频导入、动态取帧、原声及冻结画面接口见 [视频后端 API](VIDEO.md)。视频原声复用本文的 PCM、波形与冻结混音接口。

生产界面已接入音频导入、波形、音量、静音和播放。`AudioPlayback` 使用冻结混音与 AudioTrack 采样时钟；`AudioMux` 将同一合成时钟下的混音编码为 AAC，并与视频复用成 MP4。纯音频属性不显示空间变换与 3D 开关。

## 格式与资源边界

| 项目 | 支持范围 |
| --- | --- |
| 输入 | M4A/MP4 中的 AAC-LC、MP3、WAV/16-bit little-endian PCM |
| 声道与采样率 | 单声道或双声道；44.1 / 48 kHz |
| 输出 | 48 kHz、双声道、交错 `f32le` PCM；确定性限幅 `[-1,1]` |
| 源文件 | 默认最多 512 MiB / 1 小时；Rust `Limits` 可下调源文件及 PCM 缓存预算 |
| 工程源素材及备份解压 | 累计最多 4 GiB；图片仍限制每张 64 MiB |
| 导入任务 | 每会话最多两个未结束任务、64 个未释放记录；进程最多四个后台线程 |
| 混音块 | 每次最多 48,000 个双声道采样帧，384,000 字节 |
| 波形 | 源时间每 10 ms 一桶，每次最多 4,096 桶，实际 `min / max / rms` |
| 冻结混音 | 最多八个独立句柄，调用方必须释放 |

探测文件内容，不依赖扩展名或 URI 的 MIME。HE-AAC、多声道、其他采样率、浮点/24-bit WAV、复杂 MP4 编辑列表明确报错。支持常规 MP4 的单个媒体编辑，或先空白后媒体的两个编辑；处理 AAC 编码延迟及首尾裁剪，保留原时间中的空白。

源文件以 64 KiB 分块复制，解码只保留一个音频包，完整 PCM 放在 `cache/audio-v1/` 磁盘缓存。44.1 kHz 使用 32 tap、1,024 相位的加窗 sinc 重采样，按绝对采样位置计算相位，不累计帧时长。相同源文件的多个片段可使用不同源时间。成功导入的源文件保守保留，本版没有自动垃圾回收。

## Android 入口

生产桥接模块 `MediaBridge.kt` 提供以下 JNI 声明。普通会话接口须在创建会话的工作线程调用；冻结句柄可由独立音频或导出线程读取和释放。

```kotlin
package com.motionstudio.editor

object MediaBridge {
    init { System.loadLibrary("motion_engine") }
    @JvmStatic external fun request(id: Long, context: android.content.Context?, request: String): String
    @JvmStatic external fun readPcmInto(id: Long, startSample: Long, frames: Int, output: java.nio.ByteBuffer): String
    @JvmStatic external fun freezeAudio(id: Long): String
    @JvmStatic external fun readFrozenPcmInto(handle: Long, startSample: Long, frames: Int, output: java.nio.ByteBuffer): String
    @JvmStatic external fun releaseFrozenAudio(handle: Long): String
}
```

返回值复用 `{"ok":true,"data":...}` / `{"ok":false,"error":"..."}`。传入 Android `Context`，后端自己通过 `ContentResolver` 打开 `content://` 或 `file://`，支持有起始偏移和声明长度的 `AssetFileDescriptor`。前端无需转换绝对路径、复制文件或解码。

## 异步导入与取消

```json
{"op":"probe_media","request_id":"probe-1","kind":"audio","uri":"content://selected/audio"}
```

```json
{"op":"import_media","request_id":"import-1","kind":"audio","uri":"content://selected/audio","at_frame":30,"name":"音乐"}
```

`track` 可选，指定零起始轨道索引；默认选第一条支持的声音轨道，始终只解码一条，不混合多个语言音轨。目前元数据返回选定轨道，不提供多音轨列表。探测走同一条解码校验路径，不发布素材、图层或撤销记录。

```json
{"op":"media_status","request_id":"import-1"}
```

任务返回 `request_id / operation / state / phase / progress`，`progress` 是当前阶段的进度。导入依次经过 `opening / copying / decoding / waveform / awaiting_commit`；URI 提供方的打开操作也在后台线程。`state` 为 `running / ready / succeeded / failed / cancelled`。探测和缓存重建直接结束；导入到 `ready` 后，在会话工作线程提交：

```json
{"op":"finish_media_import","request_id":"import-1"}
```

返回 `task` 与最新 `state`。成功的 `task.edit_result` 包含 `op:"import_media"`、`kind:"audio"`、`asset / object / in_frame / out_frame / source_offset_us / has_audio / truncated_to_composition`。源文件、素材、片段与工程保存成功后才增加一次撤销；失败不改变已有工程。片段尾部按合成帧向上取整，超出工程长度返回裁短提示。

```json
{"op":"cancel_media_import","request_id":"import-1"}
```

取消与提交序列化：取消先成功时不能再提交；提交先成功时返回已有成功状态。终态可重复查询；`release_media_task` 释放记录后可复用请求 ID。关闭或切换工程取消旧会话任务，不能提交进新工程。后台线程在当前 I/O 或解码包结束后退出并清理暂存；阻塞的 URI 提供方读取不承诺即时中断。

```json
{"op":"release_media_task","request_id":"import-1"}
```

源文件放在 `assets/`；PCM 与波形是可重建缓存，不进入工程备份。重开或导入备份后，缺缓存时异步重建：

```json
{"op":"prepare_audio","request_id":"rebuild-1","asset":5}
```

轮询至 `succeeded` 后恢复读取。源文件实际长度或元数据不匹配、损坏或超限会失败，不产生无声的成功片段。

## 片段与波形

原有 `NativeBridge.command`：

```json
{"op":"set_audio","object":7,"volume":0.8,"muted":false}
```

`volume`、`muted` 可分别省略，音量为线性 `[0,2]`。`visible` 不静音，透明度不作为音量。音频拒绝 3D、空间变换及父子级；支持现有重命名、复制、移动、非破坏裁剪、分割、撤销、保存和备份。分割共享素材、源偏移及合成偏移，不从头播放；裁剪不能扩展到不可恢复源范围。`timeline_layers[].audio` 返回音频属性和派生源时间。

```json
{"op":"audio_waveform","asset":5,"first_bucket":0,"count":200}
```

返回 `asset / first_bucket / bucket_duration_us:10000 / source_duration_us / buckets`。波形对应未乘片段音量的源 PCM；空白的 MP4 编辑保留空白桶。

## 播放和导出读取

输出缓冲必须是足够大的、可写的 direct `ByteBuffer`。从缓冲起始处写入，不依赖 Java `position`，按 little-endian 解读。`frames` 是双声道采样帧数，缓冲尺寸 `frames * 2 * 4`。非法参数、容量不足或只读缓冲不会写入部分输出。

```kotlin
val buffer = java.nio.ByteBuffer.allocateDirect(1024 * 8)
    .order(java.nio.ByteOrder.LITTLE_ENDIAN)
val response = MediaBridge.readPcmInto(session, 48_000L, 1024, buffer)
```

`startSample` 使用统一的 48 kHz 合成时钟。输出为有效片段的混合声音，片段外或源结尾之后是静音。返回实际 `frames / bytes / sample_rate / channels / format`，以及 `start_sample / pts_us / end_of_stream`；最后一块可不足请求长度，必须使用实际帧数，避免编码旧缓冲尾部。

播放端将 PCM 交给 Android `AudioTrack`，暂停和跳转时停止并清理排队声音，再按绝对样本位置读取。本 PR 没有接入这条播放链路。

导出前在会话线程调用 `freezeAudio`，得到 `handle / total_frames / sample_rate / channels / revision`。导出线程按整数样本位置递增调用 `readFrozenPcmInto`，最后 `releaseFrozenAudio`。冻结实例拥有工程快照和独立文件游标，实时修改、跳转或关闭会话不会改变它；撤销也不删除其源文件。调用方仍需接入 AAC-LC 编码、轨道格式准备、统一时间轴的 MP4 复用和首尾延迟处理。

## 验证与依赖

```powershell
cargo test -p aem-core -p aem-media
python tools/generate_audio_fixtures.py  # 可选：用 ffmpeg 重建原创合成测试声音
python tools/build_android.py --task assembleDebug assembleDebugAndroidTest
```

Android 的 `AudioBackendApiTest` 调用真实 URI 提供方、原生 API 与 direct buffer。声音由本项目生成，没有外部媒体或研究材料。具体设备和结果以 PR 记录为准，MuMu 不能代表不同 Android 真机性能。

使用未修改的 [Symphonia 0.5.5](https://github.com/pdeljanov/Symphonia/tree/v0.5.5)，许可证 MPL-2.0；本项目的音频编排、缓存、工程事务、重采样和混音使用 MIT 许可证。
