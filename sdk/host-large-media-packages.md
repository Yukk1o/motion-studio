# 大媒体源文件与工程包接口

这一批解决单个 512 MiB 以上视频不能导入、工程包传入限制不一致，以及大型工程打包占用会话工作线程的问题。以主分支为基准，不依赖粒子编辑器或 AE 工程采集 PR。

## 资源上限

| 资源 | 上限／行为 |
| --- | --- |
| 音频、视频源文件 | 单文件 2 GiB（2,147,483,648 字节），包括共享原声的 MP4 |
| 视频解码和时间索引 | 沿用 4K 像素、SDR 8 bit、240 fps、1 小时等现有限制；实际解码仍需设备支持 |
| 音频 PCM | 沿用 1,382,400,000 字节磁盘缓存上限；不会随着源文件上限增长 |
| 项目源资源 | 去重后累计 4 GiB；音视频的同一路径只统计一次 |
| 工程包解压总量 | `project.json` 加唯一源文件累计 4 GiB，JSON 单独限制 16 MiB |
| 工程包容器 | 4 GiB + 4 MiB（4,299,161,600 字节），包括 ZIP 头和压缩开销 |
| 传输缓冲 | 64 KiB；源文件及 ZIP 资源按块读写，不整文件放入内存 |
| 图片 | 压缩源仍为每张 64 MiB；现有图片解码和 GPU 预算不变 |

只修改资源策略，不升级工程格式。已有小工程包仍可导入；含大源文件的新工程包可能被旧版应用的 512 MiB 限制拒绝。达到项目资源上限的工程还需要为包内 JSON 留出空间，打包前检查实际总量，超限明确失败。

视频导入继续用 `import_media`、`media_status`、`finish_media_import`。未知长度 URI 按实际复制字节检查，已知长度还要求最终长度一致。时间戳扫描由原固定 30 秒改为 `30 + ceil(源字节数 / 16 MiB)` 秒，按 2 GiB 源预算封顶为 158 秒，每个样本继续检查取消和索引数量；不跳过帧或缩小元数据。成功导入的源文件保存在 App 私有目录；视频与其原声、复制／分割的片段共享同一源文件。这不是按 URI 内容去重的媒体库，新导入同一 URI 仍会创建独立素材。

## 不依赖会话的上限查询

新增 `MediaBridge.packageLimits(): String`，可在打开工程前、恢复失败后及 I/O 线程调用。返回普通 `ok/data` envelope：

```json
{"ok":true,"data":{"schema_version":1,"max_source_bytes":2147483648,"max_payload_bytes":4294967296,"max_archive_bytes":4299161600,"transfer_buffer_bytes":65536,"async_export":true,"frozen_export":true,"async_import":false,"max_export_tasks_per_session":1,"max_export_workers":2}}
```

同一对象也在 `media_capabilities.project_package` 和会话状态 `capabilities.project_package` 返回。前端使用返回的字节数和分块大小，不再硬编码 512 MiB；现有工程包 URI 复制入口已接此查询。

## 后台冻结导出

在创建会话的工作线程通过 `MediaBridge.request` 调用：

```json
{"op":"export_project","request_id":"package-1"}
```

启动阶段验证资源、复制整个工程描述并打开源文件句柄，随后立即返回任务；源文件的复制、ZIP 写入、CRC 和落盘在专用后台线程进行。打开子合成时同样打包完整工程，保留全部合成和共享素材。

```json
{"ok":true,"data":{"schema_version":1,"request_id":"package-1","operation":"export_project","state":"running","phase":"queued","progress":0.0,"bytes_processed":0,"total_bytes":1000000,"source_count":1,"frozen_revision":42}}
```

示例中的字节数是占位值，实际值由快照计算。`total_bytes` 为未压缩 JSON 与唯一源文件大小，不是最终 ZIP 大小。`progress` 按该字节数单调前进；到 1 后可能仍在结束 ZIP、同步文件，只有 `state=succeeded` 才能分享输出。

```json
{"op":"media_status","request_id":"package-1"}
```

成功任务追加 `path`，是 App 私有目录下唯一的 `.aem` 文件；失败追加 `error`，不会返回可分享路径。状态为 `running / succeeded / failed / cancelled`，没有导入任务的 `ready` 和 `finish_media_import` 步骤。

冻结规则：启动后的图层编辑、撤销、保存、切换工程不会改变输出内容和 `frozen_revision`。源文件句柄保持打开，替换／移除原路径不改变已打开文件的内容；宿主自身不原地改写导入源文件。对同一已打开文件原地改写属于不支持的外部操作，大小变化会报错，等长内容修改没有逐字节锁定。效果包保持工程依赖记录，插件安装目录和缓存不打入工程包。

保留当前会话即可在切换工程后查询旧导出；状态、取消、释放请求可以省略 `composition`，携带时必须符合当前打开的合成上下文。销毁会话会请求取消尚未结束的任务。

## 取消、发布和释放

```json
{"op":"cancel_media_task","request_id":"package-1"}
```

这是既有 `cancel_media_import` 的通用别名，也可取消音视频任务。工程包取消通常先返回 `running / cancelling`，等后台关闭并移除暂存文件后才返回 `cancelled`。每个资源读块之间检查取消，正在进行的文件系统 I/O 或同步不能承诺立即中断。

取消和最终重命名共用任务锁：取消先发生就不发布；成功先发生则取消返回已有成功状态。完成前只写独占创建的 `.ms-package-*.tmp`，成功时原子改名；出错、放弃 prepared 包或取消会删除该任务暂存文件，已有输出不会被半成品覆盖。

```json
{"op":"release_media_task","request_id":"package-1"}
```

只有终态允许释放。每会话最多一个进行中的工程包导出、64 个未释放包任务记录，进程最多两个打包线程。请求 ID 在音频、视频、工程包任务之间不得重复。释放记录不删除成功的输出文件，调用方在分享完成后管理输出寿命。

## 前端接入步骤

1. 选中工程包输出时，在会话工作线程发 `export_project`，保存请求 ID。
2. 每 100–250 ms 在同一线程查询 `media_status`；不用阻塞等待，其他编辑请求仍能执行。
3. 将 `progress` 展示为字节进度；取消按钮发 `cancel_media_task`，等待终态后释放记录。
4. `succeeded` 后将 `path` 交给现有分享流程；失败显示 `error`。每条结束路径都释放记录。
5. 界面关闭若不继续导出，先请求取消；宿主销毁兜底取消。

本 PR 没有新增前端界面或进度控件。旧 `NativeBridge.pack` 为兼容保留，同样改用流式 Stored/ZIP64 写入，但仍同步执行；现有输出按钮在前端采纳上述任务接口前仍会占用会话工作线程。Android 工程包解压仍走旧同步入口，没有声明异步导入。

## Rust SDK 和存储兼容

`PackageSnapshot::new(root, project)` 创建快照，`prepare(output, callback)` 流式写入，返回的 `PreparedPackage::publish()` 才发布。纯 Rust 调用方也可使用 `export_package_with_progress` 和 `import_package_with_progress`，回调返回错误即可停止并清理本次暂存。导入仅接受新目的目录，完成校验后原子改名。

JSON 使用 Deflate，已编码图片、视频和音频使用 Stored，并预留 ZIP64 大小字段；同一音视频源只存一个 entry。旧 ZIP 包仍能读取。现有路径、重复条目、符号链接、缺失／额外资源和长度校验继续执行。临时纹理和素材 GPU 内存规则不在本批修改范围。

## 后续资源批次

大图片目前仍可能在导入的 64 MiB 解码检查或集中上传的 128 MiB GPU 预算处失败。下一批单独实现按需加载、稳定素材索引、预览代理及正式输出原尺寸读取；本批不声称目标 AE 工程已全部可导入或已还原。

验证结果与复现命令见 [资源改造验证记录](large-media-packages-validation.md)。
