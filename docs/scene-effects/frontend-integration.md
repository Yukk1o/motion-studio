# 专用插件编辑器接入

宿主提供通用窗口容器；HTML、CSS、JavaScript 及交互由插件提供。SDK 不规定只能使用滑块，可以自行实现元件列表、预设浏览、曲线、节点、Canvas 操纵柄或分页编辑器。本仓库提供可运行的内置插件页面及后端桥接，App WebView 容器交给前端实现。

所有调用继续使用 `NativeBridge.plugin(handle, requestJson)`，返回既有 `{ok,data}` / `{ok:false,error}` 信封。NativeBridge 会话在所属工作线程串行访问；WebView 回调不能直接从 UI 线程调用 JNI。

## 1. 创建图层与发现编辑器

先通过原有 `command` 的 `add` 创建合成尺寸的透明 Solid 图层，变换位置为合成中心。通过 `catalogue` 查找精确插件版本/哈希及 `definition.renderer`，再使用原有 `plugin add`。`renderer=particles/lens_flare` 是场景生成器；`category` 是一级分类，`editor` 声明自定义窗口。

```json
{"op":"editor_open","object":2,"instance":1}
```

`data` 含 `protocol:1`、`token`、完整 `definition` 和 `state`。state 包含 revision、frame、values、参数轨道、scene、seed、图层变换、合成尺寸、摄影机、可引用图层列表及该效果的表达式。`values` 是表达式执行前的基础参数采样；表达式仍影响实际预览。新增的 `transform_values={position,rotation,scale}` 使用当前合成帧换算后的图层局部时间，给出三分量基础变换采样；不能直接用 `transform.position.value` 代替动画当前值。

只能同时打开一个插件编辑器。token 是当前挂载窗口的路由标识，不能代替 WebView 的来源隔离。窗口绑定 object、instance、effect、精确版本及包哈希；实例删除、升级或工程替换后必须关闭并重新打开。

## 2. 加载 UI 文件

```json
{"op":"editor_asset","token":"editor-…","path":"ui/editor.html"}
```

返回 `{mime,base64}`。仅允许当前编辑器 `files` 中列出的资源，不能读取 WGSL、工程素材、私有目录或其他插件文件。

推荐通过 AndroidX WebViewAssetLoader 或等价的本地 HTTPS 路由，映射 `https://<本地宿主>/plugins/<hash>/ui/...`。宿主解码资产，在页面外用 Bitmap/Base64 等工具处理预览。不要使用 `file://`，不要把实际私有目录路径暴露给插件。

容器必须落实以下约束，不能仅依赖插件自行提供的 meta 标签：

- 禁止网络导航、外部子框架、下载、弹出窗口、文件和内容 URI 访问；本地路由只提供清单内的资源。
- 每个插件窗口隔离来源与会话，不挂载通用 NativeBridge，不暴露安装、导出、Shell、文件读取、任意 command 等接口。
- 注入 CSP：`default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'`。
- 使用只允许主框架与指定来源的 WebMessageListener，或具有同等来源校验的通道。不要向不受信任子框架暴露 JavascriptInterface。

## 3. 连接与消息

内置页面使用 `window.MotionStudioHost.postMessage(string)`，收到消息：

```json
{"protocol":1,"token":"editor-…","id":"7","message":{"op":"state"}}
```

前端检查版本、来源、token、id、长度与窗口生命周期，再转发：

```json
{"op":"editor_message","token":"editor-…","message":{"op":"state"}}
```

调用页面的 `window.motionStudioReply({token,id,ok,result,error})` 回传结果。`result` 对应 JNI 的 `data`。初始化调用 `window.motionStudioConnect({token,definition,state})`；使用正确的 JSON 序列化，不拼接用户字符串为 JavaScript 代码。

建议消息不超过 256 KiB，单窗口只保持少量待处理请求。内置页面的超时为 10 秒。撤销、重做、寻帧或普通 Inspector 修改后，发送新的 state；陈旧 revision 会被后端拒绝。

1.1.0页面使用ES module。等页面加载完成且 `window.motionStudioConnect` 已存在后再初始化；加载资源时将所有四个UI文件按相同来源提供。插件内部串行提交修改，每条请求读取前一条返回的revision及参数向量。宿主收到消息仍须串行访问Native会话。

撤销、重做、寻帧和外部编辑后，调用 `window.motionStudioUpdate(newState)`。已有拖动事务期间固定宿主当前帧，不让一次拖动写入多个帧；先提交/取消或关闭该编辑器，再切换工程/效果实例。窗口关闭时先调用 `editor_close(commit=false)`，再调用 `window.motionStudioDisconnect()` 并移除WebView；页面的pagehide取消只是尽力处理，不能替代宿主关闭接口。

页面会拒绝旧token、断开后的消息和更旧revision；预览只有一个请求在途，丢弃旧revision/旧frame的PNG，并补发最新请求。宿主预览失败时保留上次画面和当前工程，不阻止修复参数。需要恢复状态时先查询state，错误编辑不会自动重放。

## 4. 允许的消息

除 state、preview 外，消息必须包含最新 `revision`。编辑时间使用宿主当前帧，效果参数自动换算图层局部时间。

| op | 额外字段 | 行为 |
|---|---|---|
| state | 无 | 查询实例、参数与状态 |
| set | param、value（4 个数字） | 编辑当前效果的参数 |
| animate | param、enabled | 开关参数关键帧；遵守可动画性 |
| curve | param、value（CurveObject） | 编辑支持的曲线对象 |
| scene | settings（完整 SceneSettings） | 原子替换镜头元件、光源引用和遮挡设置 |
| seed | seed（u32） | 设置确定性随机种子 |
| transform | property、value（3 个数字） | 仅编辑所属图层的 position / rotation / scale |
| begin / commit / cancel | 无 | 一次拖动对应一次撤销；取消恢复整个事务 |
| preview | width、height | 获取 1..512 像素尺寸的合成预览 |

示例：

```json
{"op":"set","revision":12,"param":"intensity","value":[2,0,0,0]}
```

preview 返回 `{width,height,png,revision,frame,instances}`，png 是 PNG 的 Base64，instances 包含 alive、visible、culled、upload_bytes。图像是当前完整合成，使用合成摄影机；显示时保留比例。请求使用缓存的 Renderer、素材和捕获目标，不写入 exports。

预览由宿主限制频率、合并请求、丢弃陈旧图像；建议交互期间最多 10 次/秒，松手补一帧。预览是 GPU 操作，单个后端请求不提供中途取消；关闭窗口后丢弃返回结果。

## 5. 事务与关闭

拖动开始发送 begin，按最新 revision 串行发送更新，松手 commit；取消、页面退出或崩溃发送 cancel / editor_close。编辑器持有事务时，普通 command 和其他插件修改操作会拒绝混入。

```json
{"op":"editor_close","token":"editor-…","commit":false}
```

默认取消尚未提交的事务；不撤销之前已经提交的独立编辑。关闭释放预览 GPU 缓存。窗口关闭必须撤销桥接监听、取消前端待处理消息并移除 WebView。工程切换在后端撤销未完成事务并使旧窗口失效。

## 6. 图层变换及首版边界

粒子在发射器的局部空间运动，发射器 position/rotation/scale 改变会带动已出生粒子。插件的 transform 消息不允许编辑其他图层；跟随光源的外部图层由宿主 Inspector 编辑。

镜头元件的列表数据是静态结构，强度、总尺寸、光源位置等总控参数支持轨道和表达式。若引用光源被删除，保留引用并报错；用户通过编辑器重新绑定或选择手动位置。保持编辑器可打开，以便修复。

此变更没有实现 App 中打开窗口的按钮、WebView 容器、3D 操纵柄或 ViewModel；无需合并现有前端 PR 才能使用后端 API。

## 7. 已交付的插件页面

场景包1.1.0自带粒子与镜头专用编辑页。粒子支持分组参数、RGBA拾色、合法范围输入、种子、动画开关、发射器变换和轻盈/均衡/密集起点；镜头支持绑定/缺失引用修复、遮挡、元件选择/复制/增删/启停/上下移动、形状、颜色与几何编辑。恢复默认仅重置效果参数（镜头包含元件/绑定默认配置）；动画参数只改当前帧，不删除其他关键帧。种子及所属图层变换由各自控件编辑。

拖动使用begin/set/commit，Esc或pointercancel使用cancel；预设、恢复默认和RGB拾色在单个事务内提交，一次撤销还原。硬范围来自精确包，超出范围或粒子容量时明确反馈；不调整用户输入使其通过。布尔/枚举按实际整数值提交。缺失光源保留原引用，锁定图层只允许查看与预览。

320px起采用上下布局，768px起采用左右布局；桌面参数区独立滚动，手机页面正常滚动；交互控件最小48px。预览是当前合成，不能在插件里把用户寻帧替换成模拟动画或导出视频。具体测试和剩余宿主责任见 [editor-validation.md](editor-validation.md)。
