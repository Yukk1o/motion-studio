# 运动粒子：SDK 5 宿主与前端交接

首批提供 `com.motionstudio.effects.particles` / `1.0.0` 的 `particle_emitter`，中文名“运动粒子”，一级分类“粒子”。这是独立粒子实现，参考通用运动发射器工作流，不声明 Trapcode Particular 文件、预设或算法兼容。原有场景包 `com.motionstudio.effects.scene` 1.0.0 / 1.1.0 保留原字节、版本和局部空间行为。

## 编辑与调用

使用既有插件 catalogue、add、editor_open、editor_message、editor_asset、editor_close 接口。新效果声明 `renderer: "particle_emitter"`、`sdk_version: 5`、能力 `particle_birth_history`，场景默认值为：

```json
{"particle_space":"world_birth","occlusion":false,"elements":[]}
```

该包通过 `native_editor` 声明原生 UI 插槽，没有 HTML/JS 资源。Android 点击效果名称直接进入全屏原生粒子设计页，也可从详情菜单选择“专用编辑器”；上方复用 wgpu 预览，下方按发射、运动、外观、变换分组，预览区域带共享合成时间轴。详见 [原生插件 UI 契约](native-plugin-ui.md)。完成一次提交整页修改，可在外面一次撤销；取消或返回恢复进入前的工程。JNI 渲染计划版本保持 4。

绑定其他图层或 Null 的路径：

```json
{"op":"scene","revision":12,"settings":{"particle_space":"world_birth","source_layer":42,"occlusion":false,"elements":[]}}
```

该请求放入现有 `editor_message` 的 `message` 字段，并携带 editor_open 返回的 token。清除 `source_layer` 或设置为 null 即跟随效果所属图层枢轴。源图层删除后保留引用并报告错误，不自动改绑；Audio 不能作为源。编辑器 state.layers 新增 `particle_source` 布尔值供筛选。

给发射位置偏移开启动画：

```json
{"op":"animate","revision":13,"param":"position","enabled":true}
```

时间轴上的添加/删除键帧使用同一个 token 和参数轨道：

```json
{"op":"key","revision":13,"param":"position"}
```

如果当前参数尚无动画，此请求创建首个关键帧；已有动画时，在当前合成帧添加关键帧，若该时刻已有关键帧则删除。核心负责将合成时间换算为片段局部时间。

自定义 PNG 精灵引用工程图片资产，`sprite_asset` 与 `source_layer` 可同时设置：

```json
{"op":"scene","revision":14,"settings":{"particle_space":"world_birth","source_layer":42,"sprite_asset":7,"occlusion":false,"elements":[]}}
```

图片可以通过既有导入流程先加入工程，再从专属编辑器的“粒子精灵”槽选择。清除 sprite_asset 恢复柔光粒子。保持图片宽高比和透明轮廓，以 size 定义最长边；颜色按生命周期乘到图片上。多图层粒子系统共享该素材的纹理，素材不逐粒子复制、不逐帧重新上传。首期是单张图片，不含精灵图集动画或插件页面内新增素材导入。

当前帧设置向量：

```json
{"op":"set","revision":14,"param":"position","value":[120,20,0,0]}
```

路径通过图层位置关键帧或上述局部偏移关键帧控制。源图层的父级、bind、2D/3D 开关和片段 offset 均参与出生时采样。普通图层采用固定二维投影；将效果所属图层设为 3D 后才使用合成摄影机。首期没有插件内钢笔路径编辑器。

## 参数范围与语义

全部范围为硬限制，数值输入和宿主均校验；滑块可能显示便于操作的较小区间。

| 参数 | 范围 / 单位 | 默认值 | 动画 |
|---|---|---|---|
| rate | 0–10,000 粒子/s | 360 | 否 |
| lifetime | 0.001–120 s | 2 | 否 |
| position | 各轴 ±100,000 px，源枢轴局部偏移 | [0,0,0] | 是 |
| direction | 各轴 -1–1，采样后归一化，零向量无定向速度 | [0,-1,0] | 是 |
| speed | ±10,000 px/s，负值反向 | 45 | 是 |
| spread | 0–10,000 px/s，球面随机速度 | 12 | 是 |
| inherit_velocity | 0–4 倍 | 0.5 | 是 |
| gravity | 各轴 ±10,000 px/s²，合成坐标 | [0,30,0] | 否 |
| wind | 各轴 ±10,000 px/s，合成坐标 | [0,0,0] | 否 |
| drag | 0–100 s⁻¹ | 0.8 | 否 |
| extent | 各轴 0–20,000 px，盒/椭球的完整直径 | [0,0,0] | 是 |
| shape | 0 点、1 盒、2 球 | 0 | 否 |
| size / end_size | 0–4,096 px | 14 / 2 | 是 |
| color / end_color | RGBA 各分量 0–1，sRGB 颜色输入 | [0.15,0.8,1,1] / [0.8,0.15,1,0] | 是 |
| fade | 0–0.5，寿命两端淡入淡出比例 | 0.04 | 是 |
| prewarm | 0 关闭、1 开启 | 0 | 否 |

`ceil(rate * lifetime) <= 20,000`；单帧可见实例总数不超过 65,536。合法单个参数的组合可能超过容量，此时明确报错。rate=0 清除该效果缓存并不产生粒子。

## 出生、时间、力场与外观

- 以效果所属图层的局部秒数 t 定义恒定发射时钟，整数 birth_id 对应出生时间 birth_id/rate。片段移动通过 offset 映射回合成时间采样源枢轴。修剪可见片段不重置其局部发射时钟。
- 出生时固定世界位置、初始速度、起止尺寸、起止颜色和 fade。修改路径或参数会使相关出生缓存失效并重建；播放中新关键帧的当帧数值仅影响在该时刻出生的粒子，存活粒子保留其出生快照。
- Y 正向下，Z 正向远离观察者；初始速度受源的出生变换影响。尺寸继承出生时最大轴缩放，随后为面向摄影机的柔光 billboard，采用加色混合。layer opacity 在效果链后作用。
- 速度继承取源枢轴加 position 偏移的世界速度，以 ±1/120 s 的中心差分估计；关键帧折点处取两侧平均。预热负时间按轨道已有端点外推规则采样。
- 力场满足 `dv/dt = gravity + drag * (wind - v)`，通过解析解计算每个年龄；drag=0 时风不施力。无需按帧积分，也不依赖此前播放顺序。
- 生命周期尺寸和颜色首期采用起止值线性插值。RGB 转换到线性空间后生成预乘 sprite 输出，与 GLES 共用同一 WGSL/Naga shader 和 48 字节实例布局。
- 屏幕外粒子仍计算年龄和轨迹，仅剔除 draw instances；移动摄影机后可重新出现。出生缓存总容量限制为 65,536 个槽位，超出时淘汰其他缓存并按需重算，不降低速率或寿命。二维效果忽略粒子 Z 投影，避免随机速度将粒子裁到平面外。

## 能力、错误和后续范围

运行时 `capabilities.scene_effects` 报告 SDK 5、`particle_birth_history: true`、`particle_history_expressions: false`、`particle_rate_animation: false`。发射器及其空间父级、所绑定源的属性表达式、粒子参数表达式首期明确不支持历史采样。预览沿用问题效果输入并显示诊断，正式导出拒绝问题效果。未绑定为源的其他图层/摄影机表达式仍按现有规则采样。

首期交付运动发射器、PNG 精灵和独立粒子拖尾，不包含连续 ribbon、精灵图集动画、碰撞、湍流、Aux 系统、流体、动画发射速率/寿命或历史 JS 表达式。后续应扩展出生时钟、历史表达式采样和寿命曲线，再增加拉伸粒子、连续轨迹、辅助发射和力场编辑器。

## 构建与验证

`python tools/generate_particle_library.py` 重建新包，不修改历史包。`effect_tool check` 校验 WGSL/GLES shader 与包契约。`cargo test --workspace -- --test-threads=1` 包括出生位置、参数快照、缓存复用、跳帧、父级和片段时间、速度继承、力场、保存恢复、撤销及 GPU 出图用例。

Android NativeParticleEditorTest 验证原生控件、共享 wgpu 预览、播放与定位、内外关键帧同步、整页提交/撤销/重做和取消。旧 WebView 插件编辑器及浏览器测试继续为原有场景包提供兼容支持。

`cargo run -p motion-render --bin particle_probe -- <输出目录> [--sprite]` 输出 6 s / 30 fps / 960×540 路径示例、工程和每秒渲染读回 FPS。该指标含场景采样、渲染和同步读回，不含 PNG/MP4 编码，也不等同于手机预览帧率。Android SceneEffectsTest 对七种生成器（含运动发射器的自定义 PNG）进行未编码 wgpu/GLES RGB/Alpha MAE ≤3 的对照。
