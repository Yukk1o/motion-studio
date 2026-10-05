# JNI 渲染计划协议 2

`renderPlanInfo(session)` 做依赖预检并返回 version=2、uniformBytes=624、bufferBytes（安全容量）、assetBytes（含白色纹理的素材字节数）和 programs。每个 program 为 key、glsl（vertex/fragment/blocks/textures）、resources（path/width/height）。通过 `pluginPixels(session,programIndex,resourceIndex)` 一次读取RGBA8；图层素材继续用 assetPixels。相同包hash与path的PNG跨pass复用，素材与插件资源合计最多128 MiB。整个导出期间保留同一 session。

`sampleRenderPlanInto(session,integerFrame,directByteBuffer)` 返回写入字节数，失败为-1。缓冲区为 native endian、direct，容量至少 info.bufferBytes；样本不通过 JSON。应用不能把 header 中的版本/长度错误当成空画面。

Header 为16个u32，共64字节：

| word | 含义 |
|---|---|
| 0 / 1 | magic=0x46584d53 / version=2 |
| 2 / 3 | draw数 / pass数 |
| 4 / 5 / 6 / 7 | draw字节offset / pass字节offset / uniform字节offset / 总字节数 |
| 8 / 9 / 10 | 纹理池宽 / 高 / slot位图 |
| 11 / 12 | LUT字节offset / LUT数 |
| 13 / 14 / 15 | batch 字节 offset / batch 数 / vertex 字节 offset |

Draw 为32个f32，共128字节。0～15列主序 view-projection（顶点已经位于世界空间），16～19线性色彩，20～21最终区域宽高，22图层透明度，24素材索引，25～26纹理UV比例，27处理结果slot（-1表示原素材），28～29本图层pass开始/结束索引。其它保留。slot处理和后续合成必须逐图层执行，不能先执行全部pass再合成；纹理池被不同图层复用。素材索引0是白色1×1，正值随后按工程assets顺序；负值为视频实例槽 `-(layer.order+1)`，从冻结视频句柄读取对应帧后上传。

Batch 是3个u32，共12字节：layerIndex、firstVertex、vertexCount。Vertex 是5个f32，共20字节：世界空间XYZ及UV。按 batch 顺序绘制三角形；同一图层可能被相交平面分成多批，纹理池被其他图层覆盖后须重新运行它的效果链。效果扩展区域也参与几何排序。上限为8192批、65536顶点。

Pass 为8个u32，共32字节：programIndex、input（按i32解释）、source（i32）、outputSlot、实际宽、实际高、uniform字节offset、LUT字节offset（0xffffffff无LUT）。input/source负值 `-(assetIndex+1)` 指素材，非负值指纹理池slot。

池有slot0～6，slot0及4～6为RGBA8 sRGB目标，1～3为RGBA8 UNORM；同尺寸高水位池受64 MiB限制。pass按实际尺寸设置viewport、清空全目标、禁用blend；输入不能与当前输出相同。输出slot0最后由原有线性预乘合成器绘制，UV比例限制到有效区域。参数块布局见SDK；LUT固定256×1 RGBA8、1024字节。

GLSL 的 blocks 名称由反射返回，绑定到一个624字节UBO；sampler名称映射为[group,binding]，group1的texture bindings0/2/4是input/source/LUT，group2的0/2/4/6是四个PNG。宿主布局允许资源未使用；不要猜测Naga变量名称。Naga顶点代码包含GL坐标修正，不要额外翻转效果纹理；素材首行与处理纹理的logical Y=0一致。

`GlEffects.kt` 和 `VideoExporter.kt` 使用此协议完成效果、几何和动态视频的统一导出。参数界面在 `EffectsPanel.kt`，沿用 `CurveEditor.kt` 的缓动曲线编辑。原sampleInto保留供无效果旧调用方使用，启用效果时明确失败。新增协议为后续可变数量参数和pass预留了版本检查。
