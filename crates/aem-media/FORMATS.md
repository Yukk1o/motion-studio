# Motion Studio · 媒体格式 API

格式扩展在 Rust 媒体服务与 Android 原生边界完成，生产前端无需自行解码。输入支持与输出编码分别声明；当前导出仍为 H.264 + AAC 的 MP4。

## 查询当前设备

```kotlin
val response = MediaBridge.request(0L, null, "{\"op\":\"media_capabilities\"}")
```

此操作不需要工程会话或 Context，返回统一 `ok/data` 包装。`data` 包含：

| 字段 | 含义 |
| --- | --- |
| `schema_version` | 当前为 1 |
| `decoders[]` | 当前设备实际注册的解码器，排除编码器 |
| `decoders[].name` | Android 解码器名 |
| `hardware_accelerated / software_only` | Android API 返回的加速属性，不能代替性能实测 |
| `types[].mime / profile_levels[]` | 支持 MIME、Android profile / level 整数常量 |
| `types[].backend_enabled` | 当前后端是否允许该 MIME；设备有 AV1 解码器时仍可能为 false |
| `types[].profiles_truncated` | 超过 64 个 profile/level 时为 true |
| `video / audio` | 后端容器、编码、profile、位深、采样率、声道范围 |
| `requires_probe` | 始终为 true；选定文件仍需实际探测 |
| `decoder_presence_guarantees_file_support` | 始终为 false |
| `max_source_bytes / max_duration_seconds / max_pcm_cache_bytes` | 导入资源上限，任一触及都可失败 |

查询完整设备目录，包含尚未启用的 MIME，前端应同时判断 `backend_enabled`。音频的 `portable_formats` 使用内置 Rust 解码器；`platform_formats` 依赖设备原生解码器。每条素材以异步 `probe_media` 成功作为最终判定，不能仅根据文件后缀或者目录中出现的 MIME 承诺支持。

## 输入矩阵

| 媒体 | 容器 | 编码 / 范围 |
| --- | --- | --- |
| 视频 | MP4 / MOV / 3GP / MKV | H.264 Baseline、Main、High |
| 视频 | MP4 / MKV | H.265 Main |
| 视频 | WebM / MKV | VP8、VP9 profile 0 |
| 视频原声 | 相应容器中的一条音轨 | AAC、Opus、Vorbis、FLAC、MP3 等；平台路径需设备解码器 |
| 音频 | M4A / MP4 | AAC-LC、ALAC；HE-AAC 按设备原生能力回退 |
| 音频 | MP3、FLAC、Ogg、AIFF | MP3、FLAC、Vorbis、PCM；Opus 使用 Android 解码 |
| 音频 | WAV | 整数 PCM 8/16/24/32 位、浮点 32/64 位 |
| 音频 | ADTS AAC、AMR | Android 原生解码；设备与编码 profile 需探测确认 |

所有视频限定 8 位 4:2:0 SDR、方形像素；源尺寸上限为单边 4096、4096×2160 像素预算，包含 UHD 3840×2160、DCI 4096×2160 和对应竖屏；源帧率最高 240 fps，PTS 索引最多 100 万帧。10 位、HDR、AV1、ProRes、透明视频与多声道音频未启用。视频仍按实际 PTS 按需解码，不按每帧烘焙或强制转为 30 fps。音频保留原生 8–192 kHz 单/双声道缓存，再确定性混为 48 kHz 双声道。192 kHz 双声道约 15 分钟会触及默认 PCM 缓存预算，源文件一小时上限不意味着任何采样率均能达到一小时。

## 源尺寸、工程与设备能力

导入源与工程尺寸、工程帧率、预览质量分别处理：4K/60 fps 素材可以进入 256×144/24 fps 工程，源文件、尺寸和 PTS 保持原样；采样时按工程时间找到源帧。画幅长宽比不采用固定列表；超宽、方形、竖长素材均可按原比例导入，仍受独立的边长、像素预算和设备支持限制。非方形像素（SAR）是另一项能力，本期仍未启用。源的 `nominal_frame_rate` 允许 59.94 等小数值。工程 `fps` 仍是整数，后端允许 1–240，常用值可从 `data.composition.fps_presets` 获取；默认 30 仅为初始值，23.976/29.97 等有理数工程帧率尚未提供。

`NativeBridge.create(root, projectJson)` 接收以上工程帧率。前端创建工程的帧率选项与导入说明需要消费新的能力数据；本次未改动生产前端。工程帧率不承诺同等实时预览速率，预览仍使用独立的质量策略。

可在原有查询中指定尺寸与帧率组合：

```json
{"op":"media_capabilities","video_query":{"mime":"video/avc","width":3840,"height":2160,"frame_rate":60}}
```

每种视频解码器的 `types[].video_capabilities` 返回 Android 的 `width_range / height_range / frame_rate_range` 和对齐要求。范围是各维的独立上下限，不可拼接为最大尺寸同时最大帧率的承诺。

`query_result.backend_eligible` 表示当前后端的资源边界是否允许该组合；`query_result.decoders[].capabilities.size_supported`、`size_and_rate_supported` 和 `frame_rates_for_size` 分别描述设备对尺寸及尺寸/帧率组合的报告。`real_time_guaranteed` 为 false，导入仍需要实际 `probe_media`：设备标称的实时吞吐与离线逐帧解码并非同一限制。位深、颜色和 profile 继续在文件探测时校验。

4K 预览沿用 YUV 上传和 GPU 颜色转换。每流帧缓存以 8 MiB 为起点，根据原尺寸保留一帧 RGBA 所需空间，最高 36 MiB；大帧会减少预取数量。`previewInfo.video.cacheBudgetBytes` 为当前活动流的预算和，`maxCacheBudgetBytes` 为四流的绝对上限。缓存上限不包含 MediaCodec/ImageReader 内部缓冲、GPU 纹理和单次显式 RGBA 读取，不能当作总进程内存上限。

`data.video.arbitrary_aspect_ratio` 和 `preserves_source_aspect_ratio` 均为 true，`square_pixels_only` 为 true；前端可以直接使用源的 `display_width / display_height`，无需把源画幅限制在工程预设比例中。

高分辨率验收使用 `VideoInputApiTest`，以及 `tools/generate_video_input_fixtures.py` 与 `tools/validate_video_inputs.py`；原创图样覆盖 H.264 UHD 4K/60、HEVC DCI 4K/60、竖屏 4K/60、1080p/240、59.94 fps、3840×480 超宽、2560×2560 方形与 480×3840 竖长画幅，并检查越界失败、重开、4K GPU 预览、冻结读取和原始 PTS。另用 H.264 DCI 4K/120 样片记录实际设备探测结果，允许设备拒绝，但工程与撤销历史必须保持不变；不能把后端资源上限当作设备对 4096 边长或 H.264 level 6 的支持承诺。

容器和编码是独立维度；表内组合需 Android MediaExtractor 支持。损坏文件、复杂编辑列表、动态输出格式变化仍可拒绝。HE-AAC 与 AMR 已接通原生能力路径；没有相应样片验收时不可将其写成已验证编码。

部分 Android 原生解码路径未暴露末包的填充裁剪信息，缓存可能保留少量末尾样本；当前样片中最长为 Opus 的 13.5 ms。API 返回实际保留的 `sample_frames / duration_us`，不能承诺所有容器都逐样本等同于桌面解码器。比对工具分别检查完整长度和共有区间的样本，不通过忽略长度来掩盖缺帧。

## 验证

`tools/generate_media_format_fixtures.py` 从本项目原创脉冲声音与颜色画面生成样片，记录 SHA-256、编码、采样率和颜色元数据。FFmpeg 仅用于开发机器生成与比对，不随 APK 分发。

```powershell
cargo test --locked -p aem-core -p aem-media
python tools/build_android.py --task assembleDebug assembleDebugAndroidTest
adb -s <测试设备> shell am instrument -w -e class com.motionstudio.editor.MediaFormatApiTest com.motionstudio.editor.test/androidx.test.runner.AndroidJUnitRunner
```

桌面测试校验真实解码、波形桶数、缓存删除后的重建、跨块采样一致性及 192→48 kHz 降采样抗混叠。Android 测试使用 ContentProvider 的真实 URI / FD 偏移，验证新格式的 PCM、帧像素、原声延迟、冻结读取、重开及缓存重建。测试会留下 `acceptance/media-formats-*/` 报告和原始 RGBA，便于对照 FFmpeg 的对应 PTS。MuMu 的验证结果不能代表不同真机的编码兼容性或持续性能。

`tools/validate_media_formats.py --adb <adb.exe> --serial <设备> --output <忽略的证据目录>` 将 19 组完成的素材报告与 FFmpeg 比对，包括 32 帧画面、19 条声音缓存。VP8 同时验证 BT.601 和显式 BT.709；FFmpeg 的 VP8 默认矩阵可能覆盖 WebM 标记，因此该原创 BT.709 样片的 oracle 按生成清单明确应用容器矩阵，不采用被测后端返回的颜色值。PTS 只容许两种微秒取整方式之间的 1 μs 差异。
