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

所有视频限定 8 位 4:2:0 SDR、方形像素、最多 1080p 像素预算。10 位、HDR、AV1、ProRes、透明视频与多声道音频未启用。视频仍按实际 PTS 按需解码，新增编码不会按每帧烘焙。音频保留原生 8–192 kHz 单/双声道缓存，再确定性混为 48 kHz 双声道。192 kHz 双声道约 15 分钟会触及默认 PCM 缓存预算，源文件一小时上限不意味着任何采样率均能达到一小时。

容器和编码是独立维度；表内组合需 Android MediaExtractor 支持。损坏文件、复杂编辑列表、动态输出格式变化仍可拒绝。HE-AAC 与 AMR 已接通原生能力路径；没有相应样片验收时不可将其写成已验证编码。

## 验证

`tools/generate_media_format_fixtures.py` 从本项目原创脉冲声音与颜色画面生成样片，记录 SHA-256、编码、采样率和颜色元数据。FFmpeg 仅用于开发机器生成与比对，不随 APK 分发。

```powershell
cargo test --locked -p aem-core -p aem-media
python tools/build_android.py --task assembleDebug assembleDebugAndroidTest
adb -s <测试设备> shell am instrument -w -e class com.motionstudio.editor.MediaFormatApiTest com.motionstudio.editor.test/androidx.test.runner.AndroidJUnitRunner
```

桌面测试校验真实解码、波形桶数、缓存删除后的重建、跨块采样一致性及 192→48 kHz 降采样抗混叠。Android 测试使用 ContentProvider 的真实 URI / FD 偏移，验证新格式的 PCM、帧像素、原声延迟、冻结读取、重开及缓存重建。测试会留下 `acceptance/media-formats-*/` 报告和原始 RGBA，便于对照 FFmpeg 的对应 PTS。MuMu 的验证结果不能代表不同真机的编码兼容性或持续性能。

`tools/validate_media_formats.py --adb <adb.exe> --serial <设备> --output <忽略的证据目录>` 将 19 组完成的素材报告与 FFmpeg 比对，包括 32 帧画面、19 条声音缓存。VP8 同时验证 BT.601 和显式 BT.709；FFmpeg 的 VP8 默认矩阵可能覆盖 WebM 标记，因此该原创 BT.709 样片的 oracle 按生成清单明确应用容器矩阵，不采用被测后端返回的颜色值。PTS 只容许两种微秒取整方式之间的 1 μs 差异。
