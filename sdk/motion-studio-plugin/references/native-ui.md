# 原生 UI 插槽与共享组件

SDK 5 声明 `native_editor` 与能力 `native_plugin_editor`，协议 1 用描述数据创建专属页，无需 HTML、JS 或 Android 可执行文件。

```json
{"native_editor":{"id":"example.gain.editor","protocol":1,"title":"增益编辑器","sections":[{"id":"view","title":"预览","slots":[{"kind":"preview","max_size":512},{"kind":"timeline"}]},{"id":"values","title":"参数","slots":[{"kind":"parameters","params":["gain"]}]}]}}
```

| 槽 | 能力 |
| --- | --- |
| `preview` | 当前合成预览，缩略图上限 max_size 为 1–512 |
| `timeline` | 同一合成时钟、参数轨道与关键帧，不另建图层列表 |
| `parameters` | 根据类型、范围、单位与动画声明生成原生控件 |
| `transform` | 所属图层的位置、旋转、缩放 |
| `note` | 1–2048 UTF-8 字节的说明 |
| `layer_source` | particle_emitter 的图层发射源 |
| `image_sprite` | particle_emitter 的工程 PNG 精灵 |
| `seed` | 场景生成器的种子 |

单例槽不能重复。最多 16 组、合计 128 槽，每组 1–32 槽；参数槽引用 1–32 个现有参数 ID，不可重复绑定。参数槽支持数值、向量、颜色、布尔和枚举，协议 1 不支持 `curve_object`。

一个 `color` 参数复用颜色行、吸管、收藏、常用色块及覆盖式色盘；保留预览与时间轴，连续修改实时生效。取消选色恢复该颜色轨道，保留页内其他编辑。专属页与外部面板编辑同一实例；完成形成一次撤销，取消恢复进入前状态。

协议 1 通过 `parameters` 复用现有控件，不开放任意组件注册或独立 `wheel` / `easing` 槽。需要更高协议时确认 App 已声明能力，不使用尚未发布的槽名。
