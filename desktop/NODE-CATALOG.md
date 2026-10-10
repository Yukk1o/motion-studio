# 节点目录与实施分期

状态：待实现规格。本文的“移动候选”表示目标出口，不表示已经通过 Android 验收。

**PC 编辑节点，手机只运行导出的 `.msfx`。** 手机可调整公开参数、给参数添加关键帧；手机不提供节点图编辑器。节点源图保存在桌面 `.msgraph` 文件中，Group 在导出时展开。

## 能力标记

| 标记 | 含义 |
| --- | --- |
| M | 移动候选：可在现有 2D fragment 效果契约上实现，仍需 shader / GLES / 设备验收 |
| B | 移动预算节点：有采样、迭代或 scratch 成本，导出需要成本报告与明确上限 |
| S | SDK 扩展：现有资源、上下文或输出 ABI 不足 |
| R | 渲染管线扩展：需要网格、材质、光照、阴影或深度能力 |
| P | v1 产品策略限定 PC，手机出口禁止；PC 实现也可能依赖新的渲染能力 |

首版手机出口同时约束 Android wgpu 预览与既有 GLES 导出路径。不按节点名称直接宣称所有设备稳定，也不以固定半径或纹理张数代替资源预算。

## 1. 输入

| 节点 | 标记 | 契约 / 前置能力 |
| --- | --- | --- |
| Time | M | 合成时钟，秒；预览与冻结导出使用一致时间 |
| FrameIndex | M | 明确使用当前帧语义；对齐 `fx.clock`，不从低精度时间猜整数帧 |
| Resolution | M | 区分源图层尺寸、合成尺寸和当前 pass 尺寸 |
| UV | M | 标准 UV；偏移、缩放和翻转由 UV 变换节点组成 |
| VertexPosition | S/R | 2D 图层局部坐标可另设衍生节点；实际网格顶点输入需要材质 shader 上下文 |
| WorldPosition | S/R | 需要对象变换及世界空间片元上下文 |
| Normal | S/R | 区分网格法线、世界法线和法线纹理解码结果 |
| Tangent | S/R | 网格切线、手性与 TBN 契约 |
| Depth | B/S/R | 深度图片可先作为数据纹理；场景深度需要新的资源入口和编码约定 |
| CameraDistance | S/R | 世界位置与摄影机上下文，不等同于屏幕中心距离 |
| Custom Float | M | 公开浮点参数，范围、默认值与手机滑块 |
| Custom Color | M | 公开颜色参数，声明颜色空间与 alpha |
| Custom Vec2 / Vec3 | M | 公开向量参数，稳定参数 ID 和分量顺序 |
| Boolean Toggle | M | 公开开关参数；选择分支必须保留成本上界 |
| Video Input | B | 使用宿主解码后的图层输入；YUV 转换由宿主执行，保留正确颜色空间 |
| Layer Input | B/S | 使用目标 SDK 的图层资源引用，明确 source / effects 阶段，校验循环依赖 |

现有主渲染对象以图层平面、摄影机和图像效果为主。材质上下文输入不能仅增加一个节点名称便宣称完成。

## 2. 数学

下列节点均为 M，支持其声明的标量 / 向量端口。除法、平方根、幂、归一化及三角函数的异常输入处理必须公开定义，防止 NaN 沿整图传播；高级 WGSL 节点保留自己的数学语义。

| 节点组 | 节点 |
| --- | --- |
| 基础算术 | Add、Subtract、Multiply、Divide |
| 幂与根 | Power、Square、Sqrt |
| 数值操作 | Modulo、Abs、Sign、Min、Max、Clamp、Fract、Floor、Ceil、Round |
| 三角函数 | Sin、Cos、Tan、Atan2 |
| 插值 | Lerp、Smoothstep、Smootherstep、Remap |
| 向量操作 | Dot、Cross、Normalize、Length、Distance |

纯数学链可以合并为一个 pass。超越函数数量仍计入编译成本，不因“无纹理采样”被视为无成本。

## 3. 颜色

| 节点 | 标记 | 说明 |
| --- | --- | --- |
| RGB/RGBA Combine、Split | M | 通道合并与拆分，alpha 为独立端口 |
| HSV To RGB、RGB To HSV | M | 明确色相范围与无饱和颜色的行为 |
| Bloom Threshold | M | 高光提取；完整 Bloom 由提取、模糊、混合组成 |
| Brightness / Contrast | M | 明确作用于 sRGB 还是线性颜色 |
| Gamma / Inverse Gamma | M | 创作性幂函数调整 |
| sRGB Encode / Decode | M | 标准分段传递函数，单独于 Gamma 节点 |
| Color Mix / Blend | M | Normal、Multiply、Screen、Overlay、Soft Light、Difference；“滤色/屏幕”合并为 Screen |
| Tint | M | 染色与混合强度 |
| Invert | M | 可选择通道，默认保留 alpha |

编译器必须处理 straight / premultiplied alpha 及 working space。颜色转换不能由节点排列顺序自行猜测。

## 4. 纹理、采样和程序化生成

### 采样

| 节点 | 标记 | 说明 |
| --- | --- | --- |
| Texture Sample | B | 图像 / 数据纹理，UV、边缘模式与采样方式；资源受当前 SDK 插槽限制 |
| Depth Sample | B/S | 普通深度图素材可先解码为单通道数据；场景深度需新增宿主接口 |
| Gradient | M | 一个节点的 Linear / Radial / Conic / Diamond 模式，后续可扩展 |
| Checkerboard | M | 分辨率无关图案，抗锯齿与边缘模式明确 |
| Blur 2D | B | 小核、可分离核与降采样策略；大半径由成本决定移动是否可接受 |

大半径高斯模糊可以通过可分离、降采样或多级算法控制成本。v1 可提供受限移动预设；不自动将任意大半径替换成小半径。

### 2D SDF：有符号距离场

优先实现，标记 M；重采样或大量重复组合时升级为 B。距离端口携带坐标单位，约定内侧为负、边界为零、外侧为正。

| 分组 | 节点 |
| --- | --- |
| 基础形状 | Circle、Rounded Rectangle、Capsule、Segment、Regular Polygon（三角 / 五边 / 六边等）、Star、Ring |
| 布尔操作 | Union、Intersection、Difference |
| 平滑与轮廓 | Smooth Union、Dilate / Erode、Round、Offset |
| 着色输出 | Distance To Fill、Distance To Stroke、Distance To Gradient Mask |
| 描边扩展 | Inner Stroke、Outer Stroke、Gradient Stroke |

抗锯齿按最终像素覆盖率和导数 / 像素尺度计算，不将“分辨率无关”理解为无限精度或零渲染成本。非均匀缩放与 UV 扭曲后必须标记距离是否仍为严格距离；描边宽度不能悄悄改变单位。

### 噪声与规则图案

| 分组 | 节点 | 标记 |
| --- | --- | --- |
| 基础噪声 | Value Noise、Perlin / Gradient Noise、Simplex Noise | M/B |
| 分形噪声 | FBM、Turbulence | B；固定 octave 上界，预览与导出使用确定性 seed |
| 细胞纹理 | Voronoi | B；固定邻域搜索范围 |
| 规则图案 | Stripes、Dot Grid、Hex Grid、Ripple、Scan Lines | M |
| 磨损 | Edge Damage、Weathering、Aging | B；优先由 SDF、噪声和颜色节点组构成 |

磨损效果先作为可展开的官方 Group / 预设发布，避免为每种组合复制相同 shader。

### UV 变换

| 节点 | 标记 | 说明 |
| --- | --- | --- |
| Offset、Scale、Flip、Tile、Mirror | M | UV 变换，保持输入图像与坐标数据分离 |
| Wave、Twist、Distort | M/B | 解析变换优先；噪声驱动按成本统计 |
| Polar / Rectangular Coordinates | M | 合并为坐标转换节点的方向模式 |
| Displacement | B/S | 输入位移纹理；图层来源依赖目标 SDK 的资源引用能力 |

UV 链可驱动 SDF 和纹理；不能把每个 UV 运算生成为独立纹理 pass。

### T1：受控迭代生成

| 节点 | 标记 | 移动规则 |
| --- | --- | --- |
| Mandelbrot、Julia | B | 明确最大迭代次数、分辨率预设和早退出；不开放无限迭代 |
| Procedural Starburst / Lens Flare | M/B | 按解析光芒、采样与遮挡能力分别实现 |
| Glow / Light Diffusion | B | 复用受控模糊和混合计划，报告实际 scratch |

## 5. 3D、光照与材质

| 节点 | 标记 | 前置能力 |
| --- | --- | --- |
| Albedo、Roughness、Metallic、Emission | M（数值）/ R（材质） | 数值端口可先实现；接入真正材质需要材质输出 ABI |
| Ambient Occlusion Texture | B/R | AO 素材采样与真正的实时 AO 计算分别定义 |
| Normal Map | B/S/R | 数据纹理解码可先实现；切线到世界空间转换需要 TBN |
| Directional Light | S/R | 世界法线、灯光参数及渲染上下文 |
| Point Light | S/R | 世界位置、距离与衰减单位 |
| IBL | B/S/R | 明确预计算辐照度 / 反射贴图、分辨率和采样上界；高质量 PC 版本独立预设 |
| Fresnel | M（数学）/ S/R（上下文） | 公式可作为普通向量节点；视线方向需相机上下文 |
| Shadow Sample | B/S/R | 阴影贴图入口与深度比较；PCF 的核数固定且计入采样成本 |
| Parallax | B/S | 先做高度图驱动的 2.5D UV 偏移；有界 POM 单独设预算等级 |

高度图、视频相对深度、线性视深度和 GPU Z-buffer 使用不同数据含义，不能直接比较做遮挡。需要编码、近远平面、尺度与方向的明确转换。

## 输出与 Group

| 节点 | 标记 | 规则 |
| --- | --- | --- |
| Color Output | M | 现有效果出口，输出 RGBA，首期优先 |
| PBR Material Output | S/R | 输出材质结构；需要网格渲染和新的材质 SDK 契约 |
| Depth Output | S/R | 深度写入 / 纹理输出需要新格式与遮挡阶段，不能用颜色出口冒充 |
| Group | M/B/S/R/P 继承子图 | 暴露参数、保存自定义子节点；禁止递归引用，展开后重新做完整校验 |

每张图必须有一个明确的目标输出节点。不同输出种类选择不同的编译目标；PBR 和 Depth 不能以普通 RGBA 包宣称完整支持。

## v1 PC 专用策略（T2）

下列节点在 v1 手机出口标为 P：SSS、体积雾 / 体积光、PCSS、超预算的多次 / 双向模糊、路径追踪 / 光线反射、高阶迭代分形、高迭代递归噪声、3D SDF / 体素、3D 体纹理，以及光线步进体积雾。

这是产品出口策略。基础 3D 距离公式等可能只需要少量数学运算；真正昂贵的是遍历、采样和渲染流程。3D 纹理也需区分宿主 SDK 是否提供资源入口与设备资源限制，不能将策略限制写成所有移动 GPU 均无法支持。

## 导出校验

1. 展开 Group，检查循环、端口类型、输出类型和上下文能力。
2. PC 专用或缺失能力默认阻止导出，定位到节点。可提供明确的替代方案，由用户确认后生成另一份移动图；原图保留。禁止静默剔除。
3. 分开统计唯一纹理绑定数、纹理采样次数、纹理字节数、pass 数、`loop_work` 与实际 scratch。八张纹理可作为软提示示例，硬限制以目标 SDK / 设备上限为准。
4. 纯表达式节点合并 pass；复用中间图像的生命周期。报告可合并项与实际合并结果。
5. 重新载入生成的 `.msfx`，跑桌面与 Android 后端验收，再标记为该 profile 的可发布节点。

## 实施批次

- **首批 N1**：Color Output、UV / Time / 参数、基础数学与颜色、2D SDF 基础与布尔、填充 / 描边、线性 / 径向渐变，形成单 pass 导出闭环。
- **N2**：可停靠节点界面、Group、UV 变换、规则图案、值噪声 / Perlin、资源采样和可编辑公开参数。
- **N3**：FBM / Voronoi / Turbulence、磨损预设、小核模糊、多 pass、受控分形、视差、移动成本报告和降级预设。
- **N4**：网格 / 材质 / 深度接口与对应输出；再接 WorldPosition、Tangent、灯光、PBR、阴影和 IBL。
- **PC 扩展**：SSS、体积、光线步进与高阶分形等，各自验证后开放；不阻塞首批移动效果。

节点标题与说明从第一批开始提供中英文；节点 ID、端口 ID、参数 ID、WGSL 和包协议保持语言无关。MCP / 内置 agent 使用同一节点文档与编译接口，用户可继续在图中修改生成结果。
