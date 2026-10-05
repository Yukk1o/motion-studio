# motion-studio：前端效果系统接入

界面由前端开发实现。本分支提供 Rust 工程模型、命令、插件注册表、wgpu 预览、JNI 接口和 GLES/MediaCodec 适配；没有新增效果面板、参数控件、ViewModel 或页面入口。

## 接口与线程

`NativeBridge` 的工程会话必须在创建它的同一个 worker 线程调用。导入读文件、编译、安装和 PNG 捕获也放在 worker；不要在主线程调用。所有 String 接口返回 JSON：

```json
{"ok":true,"data":{}}
```

失败返回 `{"ok":false,"error":"layer 2, effect 1 (tint): missing plugin ..."}`。只在 `ok=true` 后更新状态；命令失败不会部分修改工程。现有 `command/history/seek/state/save` 调用方式保持兼容。

## 插件管理

新增 `NativeBridge.plugin(session, requestJson)`。以下请求中版本和 hash 必须来自目录接口，不能硬编码内置包的 hash。

```json
{"op":"catalogue"}
```

返回 `data.packages=[{"manifest":{...},"hash":"64位SHA-256","enabled":true}]` 和 `data.errors=["加载失败原因"]`。每个 manifest 包含插件身份、精确版本、效果数组；每个效果包含中英文名、类别、参数、pass、兼容状态和已知差异。版本可以并存。禁用版本仍显示在管理列表。

```json
{"op":"install","path":"App私有临时目录/incoming.msfx"}
{"op":"enable","plugin":"example.motionstudio.gain","version":"0.1.0","hash":"完整hash","enabled":false}
{"op":"uninstall","plugin":"example.motionstudio.gain","version":"0.1.0","hash":"完整hash"}
```

用系统 `OpenDocument` 选择 `.msfx`，将 URI 流复制到 App 私有临时文件，累计读取最多 16 MiB；随后调用 install，并在 finally 删除临时文件。安装返回工程快照；再调用 catalogue 刷新列表。同版本同 hash 安装幂等；同版本不同 hash 拒绝。内置 AE 包可以禁用，不能卸载。工程不会随安装自动升级。

新增实例由后端生成默认值和实例 ID：

```json
{"op":"add","object":2,"plugin":"com.motionstudio.effects.ae2021","version":"1.0.0","hash":"目录返回的hash","effect":"tint"}
```

返回完整 state 快照。一次增加效果形成一次撤销记录。仅图片、文字和纯色图层支持效果；摄像机、空对象和锁定图层不能增加。

显式切换版本：

```json
{"op":"upgrade","object":2,"instance":1,"plugin":"example.motionstudio.gain","version":"0.2.0","hash":"所选版本hash","effect":"gain"}
```

此操作保留实例 ID、链位置，重置该实例的全部参数、关键帧和种子，可撤销。前端必须在操作说明中清楚告知重置行为。首期没有自动参数迁移。

## 工程与显示状态

工程格式为 version 2，`layers[].effects` 是有序数组，`plugin_dependencies` 是精确依赖的去重列表。不要让前端自行拼装/更新依赖清单；通过 effect 命令修改。

实例包含 `id/plugin/effect/version/hash/enabled/seed/params`。实例 ID 在一个图层内稳定且唯一。复制图层后可以出现相同的实例 ID，因此 UI 的 key、选中状态和错误定位必须使用 `(layerId, instanceId)`。

`params` 是以稳定参数 ID 索引的对象：`kind/animatable/min/max/implemented/default/track/curve`。`track.value` 永远为 4 个数；`track.keys` 沿用现有 `{frame,value,ease,curve?}` 格式。曲线对象在独立的 `curve.value/curve.keys` 中，不能从数值 track 推断它的动画状态。

`state.data.sampledEffects` 给出当前帧值：

```json
[{"layer":2,"instance":1,"values":{"p0003":[75,0,0,0]}}]
```

AE Color 参数的第四分量是采集记录中的保留值（通常为0），内置调色算法只使用RGB；前端色块应使用RGB并以不透明Alpha显示，编辑保留原第四分量。自定义color按插件约定解释RGBA。

控件显示这里的采样值，编辑时保留其余分量。`graphics` 存在时 `effectErrors` 为当前预览的错误字符串数组；没有 GPU 时该字段为 null。seek/command 返回的快照是在绘制之前取得，绘制完成后再取 state 才能得到新一帧 GPU 诊断。不要每个显示刷新帧序列化完整工程，可在停止、拖动结束、状态变化或低频轮询时读取。

按插件 ID、版本、hash 和 effect ID 联合解析描述。缺失实例仍显示其固定依赖、参数和关键帧，允许禁用、删除及保存。预览对问题效果采用输入继续链处理；正式输出阻止所有启用的缺失/不兼容/失败效果，包括隐藏图层上的效果。不要自动删除或自动替换。

兼容字段：`verified`=已验收、`approximate`=近似实现、`unsupported`=不支持。内置首版全部为 approximate。只有有完整验收报告的 AE 效果可显示已还原；自定义效果不应被计入 AE 还原数量。`implemented=false` 的参数显示只读及未支持提示；不要提供可编辑控件。

## 效果链命令

通过 `NativeBridge.command(session, json)`，外层 `op=effect`，object 为图层 ID：

```json
{"op":"effect","object":2,"action":{"kind":"move","effect":1,"index":0}}
{"op":"effect","object":2,"action":{"kind":"duplicate","effect":1}}
{"op":"effect","object":2,"action":{"kind":"remove","effect":1}}
{"op":"effect","object":2,"action":{"kind":"enable","effect":1,"enabled":false}}
{"op":"effect","object":2,"action":{"kind":"set","effect":1,"param":"p0003","frame":12,"value":[75,0,0,0]}}
{"op":"effect","object":2,"action":{"kind":"animate","effect":1,"param":"p0003","frame":12,"enabled":true}}
```

`index` 是零开始的最终位置。数值/向量/颜色的 value 固定 4 个数，不能传 scalar 或短数组。开启动画会在当前整数帧建立第一帧；已有动画时 set 在当前帧增加或更新关键帧。仅 `animatable=true` 可开启动画。布尔值为 0/1；枚举实际值从 manifest.min 开始，`options[value-min]` 是标签。布尔/枚举只能保持插值。

当前AE枚举options保留原生数值编号，菜单名称仍待采集；展示兼容记录中的此项差异。后续由已确认的AE菜单采样更新标签并增加包版本，前端不要凭序号猜测未确认的菜单名称。

拖动开始调用 `history(session,2)`，拖动期间连续发送 set，结束调用 `history(session,3)`，取消调用 `history(session,4)`。同一 worker 串行排队，整个拖动只产生一次撤销记录；拖动结束后再 save。捕获拖动开始时的图层、实例、参数、帧和原始向量，避免延迟回复改变目标。

关键帧动作：

```json
{"op":"effect","object":2,"action":{"kind":"delete_key","effect":1,"param":"p0003","frame":12}}
{"op":"effect","object":2,"action":{"kind":"move_key","effect":1,"param":"p0003","from":12,"to":30}}
{"op":"effect","object":2,"action":{"kind":"copy_key","effect":1,"param":"p0003","from":12,"to":30}}
{"op":"effect","object":2,"action":{"kind":"curve","effect":1,"param":"p0003","frame":12,"easing":{"ease":"in_out"}}}
```

curve 的 frame 是区间左侧关键帧，必须有右侧相邻关键帧。easing 复用已有 `{ease,curve?}` 契约，支持 quadratic/cubic/elastic 和 progress/velocity。布尔/枚举不提供曲线入口。移动/复制到已有帧将覆盖该帧。

## Curves 独立曲线编辑

不要把 Curves 拆成滑块。`CurveObject={"channels":[RGB,R,G,B,Alpha]}`，每个通道是 `[x,y]` 点数组，2～64 个点，值都在 0～1，X 严格递增，第一点 X=0、最后一点 X=1。首期点之间线性连接，后端生成 256×1 RGBA LUT；与 AE 私有自定义曲线数据的重建尚未验收。

```json
{"op":"effect","object":2,"action":{"kind":"set_curve_object","effect":1,"param":"p0001","frame":12,"value":{"channels":[[[0,0],[0.5,0.7],[1,1]],[[0,0],[1,1]],[[0,0],[1,1]],[[0,0],[1,1]],[[0,0],[1,1]]]}}}
```

关闭曲线动画时，后端在 value 中附加可选的 `sampled_lut`（256 个 RGBA 数组），精确保留当前帧的插值结果；再次开启动画也保留该结果。曲线编辑器可据此显示采样线，用户提交新的控制点时省略 sampled_lut，改用 channels 生成 LUT。

先应用 RGB 主曲线，再应用 R/G/B 各通道；Alpha 独立。动画在两帧的派生 LUT 之间使用原有缓动插值，随机寻帧结果稳定。编辑可先使用本地曲线草稿，应用一次形成一次撤销；实时拖动则使用相同 history 手势事务。动画关键帧的位置由 `curve.keys` 管理。

## 输出与前端验收

PNG 使用已有 capture；MP4 使用已更新的 VideoExporter。输出前严格校验插件依赖，不允许前端忽略错误继续。MP4 的独立 native 会话在启动时持有精确包内容，期间编辑、升级、禁用及卸载不会改变该任务。不要在导出过程中重新加载注册表。PNG 同步捕获期间持有当前引用。

旧 sampleInto 接口对启用效果返回失败，不能拿它输出一个丢失效果的视频。新渲染协议见 [render-plan.md](render-plan.md)。工程备份不打包 `.msfx`，前端应提示接收者需要另行安装目录中的固定依赖。

前端完成后验收：窄屏/横屏长参数列表、48 dp 触控目标、中英文名称、近似/未支持提示、添加/排序/复制/删除/启停、一次拖动一次撤销、数值与离散关键帧、独立曲线编辑、缺失插件保存恢复、显式版本重置、导入取消/非法包反馈、导出错误及预览错误定位。当前分支的测试覆盖后端；界面验收由前端交付时补充。
