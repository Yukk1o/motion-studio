# SDK 5 原生插件 UI 插槽

插件声明 UI 数据，宿主创建原生控件。首批落地为运动粒子的全屏设计页：合成预览、共享时间轴、分组参数和完成/取消操作。布局和控件由宿主掌握，效果包无需携带 HTML、JS 或 Android 可执行代码。新 UI 与效果算法、工程数据和文件版本绑定。

## 描述

EffectDefinition 新增可选 `native_editor`；SDK 5 必须声明能力 `native_plugin_editor`。同一效果不能同时声明旧 `editor` 与 `native_editor`。旧包和旧 WebView 编辑器不变。

```json
{
  "id":"particle_emitter",
  "renderer":"particle_emitter",
  "required_capabilities":["particle_birth_history","native_plugin_editor"],
  "native_editor":{
    "id":"com.example.particle-editor",
    "protocol":1,
    "title":"粒子设计器",
    "sections":[
      {"id":"preview","title":"预览","slots":[
        {"kind":"preview","max_size":512},
        {"kind":"timeline"}
      ]},
      {"id":"emitter","title":"发射器","slots":[
        {"kind":"layer_source"},
        {"kind":"parameters","params":["position","rate","lifetime"]},
        {"kind":"seed"}
      ]},
      {"id":"appearance","title":"外观","slots":[
        {"kind":"image_sprite"},
        {"kind":"parameters","params":["size","color"]}
      ]}
    ]
  }
}
```

此为描述片段；完整粒子包仍须包含生成器规定的参数、场景模式和 shader。分组最多 16，所有槽合计最多 128，每组 1–32 槽。分组 ID 稳定、唯一；参数只能绑定存在的 manifest 参数，不能重复绑定。宿主安装阶段拒绝未知字段、未知槽、重复单例和错误参数引用。未来新控件应升级协议/能力，不以任意字符串注册动态原生代码。

| kind | 原生职责 | 约束 |
|---|---|---|
| preview | 复用当前工程 wgpu Surface 和摄影机 | 一个；max_size 1–512 为可选缩略图接口上限，不限制实际 GPU 视口 |
| timeline | 共用合成播放位置和选中参数轨道；显示键帧、定位和添加/删除 | 一个；合成帧换算由核心完成 |
| parameters | 按 manifest 类型、硬范围、单位、动画标记生成控件 | 每槽 1–32 个 ID；protocol 1 支持数值/向量/颜色/布尔/枚举，不支持 CurveObject |
| layer_source | 选择发射器所在图层/其他图层/Null 枢轴 | 一个；particle_emitter；禁止 Audio |
| image_sprite | 选择工程内已导入的 PNG 图片，保留缺失 ID | 一个；particle_emitter；首期单张精灵 |
| seed | 数值种子控件 | 一个；场景生成器 |
| transform | 编辑所属图层位置、旋转、缩放 | 一个；不修改其他对象 |
| note | 插件提供的文字说明 | 每段 1–2048 字节 |

## 会话与同步

仍使用 editor_open / editor_message / editor_close；editor_open 返回 definition.native_editor、token 和当前 state。原生 UI 不调用 editor_asset，不加载 WebView。全部编辑请求通过同一个版本/哈希固定的 scoped session，包含 revision；过期修改、错误 token、锁定对象和越界值返回错误。

state 在原有字段之外返回 `frames`、`fps`、`timeline_offset`，以及 `images:[{id,width,height}]`；layers 新增 `particle_source`。params 是工程中的原始轨道，values 是当前帧采样。专属页与外面编辑同一组 EffectInstance.params，不建立额外时间轴或另存插件动画。

### 原生颜色槽与局部取消

`parameters` 中的 `kind=color` 使用宿主共用的颜色参数行和色盘，提供吸管、收藏及用户配置的常用色块。进入色盘覆盖专属页参数区域，保留上方预览和时间轴；采用相同的色板、色环、透明度及 HEX/RGBA 控件。吸管允许在专属页的预览中取色，保留该参数的 Alpha。

专属页已经拥有一次完整的撤销事务，因此选色不会再开启嵌套历史，也不会通过外部编辑命令关闭插件会话。宿主对连续选色保持一项正在发送的请求和一个最新待发送值，避免拖动产生请求堆积；确认或取消等待当前请求完成。

```json
{"op":"color_begin","revision":12,"param":"color"}
```

要求已有页面事务、未锁定对象及颜色参数。后端保存此参数的完整原始轨道和当前合成帧；`state.color_edit` 返回 `{param,frame}`，未选色时为 `null`。在选色期间，`set`、`key`、`animate` 只接受该参数和该帧，其它编辑先结束选色。读取 `state` 不受影响。

```json
{"op":"color_finish","revision":16,"commit":false}
```

`commit=true` 保留颜色修改；`false` 完整恢复该颜色轨道，包括新增的中间帧关键帧和缓动，保留专属页此前修改的出生速率、精灵及其它参数。它不结束页面事务。完成专属页后仍形成一次撤销；取消整个专属页恢复全部页内编辑。页内定位、播放及时间轴关键帧操作先完成当前选色，再使用同一合成时钟和轨道。

Android PluginEditorHost 在原生页打开时 begin，完成时 editor_close(commit=true)，取消或返回时 commit=false；一次完成形成一个撤销记录。首期撤销/重做通过退出后的主编辑器提供。只允许原生页面经受控播放/定位入口在该事务内移动播放头；旧 WebView 拖动期间仍阻止外部定位。编辑关键帧时暂停播放，先固定当前合成帧，再提交参数请求。

关键帧切换请求：

```json
{"op":"editor_message","token":"editor-42","message":{"op":"key","revision":5,"param":"position"}}
```

核心规则为“无动画则创建首键；有动画且当前帧无键则添加；当前帧有键则删除”。在片段 offset 下读写局部轨道；保存、复制和撤销继续沿用已有工程机制。参数改变后主编辑器的 property 目标 `effect:<instance>:<param>` 与专属页选中参数保持一致。

## 前端接入与布局

NativePluginEditorView 可复用于其他原生插件页。点击声明 native_editor 的效果名称直接进入；效果详情菜单中的“专用编辑器”也保留。页面替换当前编辑工作区，保留工程上下文，不创建第二个项目或渲染器。MainActivity 为 preview 槽提供现有 Preview Surface，切换时校验 detach 所属 Surface，避免旧页面销毁回调断开新预览。

竖屏固定上方预览和时间轴、下方分组参数滚动；低高度横屏采用全页滚动，所有控制和完成/取消均可访问。触控最小高度 48 dp，枚举和素材使用原生选择菜单，数值使用原生输入对话框并校验 manifest 的硬范围。

后续可扩展曲线编辑、交互式发射器 gizmo、多发射器列表、图集、纹理导入和模块连接视图。优先扩展宿主原生槽及受控数据操作，再让插件声明需要哪些槽。Particular 的模块化 Designer 是工作流参考：[Maxon Designer 概览](https://help.maxon.net/rg/en-us/Content/html/04-Trapcode-Particular-overview-of-the-designer.html)。
