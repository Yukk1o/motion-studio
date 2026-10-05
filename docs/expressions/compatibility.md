# AE 表达式兼容配置 1

基准是 AE 的现代 JavaScript 表达式写法（Adobe 文档说明基于 ECMAScript 2018），不是 Legacy ExtendScript 或 `.jsx` 自动化脚本。当前属于有回归测试的兼容子集，还没有用本机 AE 18.0.1 逐项做数值对照；不声称与 AE 输出完全一致。

| 能力 | 支持与差异 |
|---|---|
| JavaScript 语法 | QuickJS-NG 执行变量、函数、数组、对象、Math、条件、同步循环等；SWC 解析源码。异步函数/await 不支持，无浏览器、Node 或 Adobe 应用对象。 |
| 多行结果 | 末尾表达式语句、block 或 if/else 分支的末尾表达式作为结果；以函数声明、变量声明、switch/循环结尾不会自动推导最后值，需在之后单独写结果表达式。 |
| 向量运算 | `+ - * / %` 二元运算、负号、变量名上的复合赋值按分量处理；数组维度不足填零，标量广播，最大 4 维。成员的复合赋值保留标准 JS 语义，不把 getter 或索引执行两遍。动态 eval/Function 的字符串没有经过向量转换。 |
| time / value / index | time 是合成秒数；value 为表达式前关键帧采样；index 为从顶层起 1 的图层序号，排序会改变。摄影机 index 为 0 的本宿主扩展。 |
| numKeys / key / nearestKey | 索引 1 起，key 返回 `{time,value,index}`。时间为合成秒数，图层移动将偏移应用到关键帧时间。等距 nearestKey 取较早关键帧。没有关键帧时 key/nearestKey 报错。 |
| valueAtTime / velocityAtTime / speedAtTime | 只读取当前属性的原轨道，不递归执行表达式。保留原缓动/自定义曲线；速度采用 1/100 帧中心差分，是近似值。分离维度后整个向量的关键帧时间是三轴时间并集，单轴只使用该轴。 |
| thisProperty | value、numKeys、key、nearestKey、valueAtTime、velocityAtTime、speedAtTime、velocity、speed、loopIn/Out、wiggle。 |
| thisComp / thisLayer | 提供宽高、frameRate/frameDuration/duration，以及 name/index/inPoint/outPoint/startTime。thisLayer 还提供基础数学 helper。尚未实现 layer()、effect()、transform、toWorld/toComp 等对象引用。 |
| loopIn / loopOut | cycle、pingpong、offset、continue 与 numKeyframes；少于两关键帧返回当前 value，超出数量取现有区间。continue 使用关键帧内侧差分速度近似。 |
| wiggle | 相同函数参数，1..8 octaves，连续确定性噪声；算法独立实现，噪声序列与 AE 不相同，freq 不允许负数。支持显式 t 和 seedRandom 对噪声身份的修改。 |
| seedRandom / random / Math.random | 种子由工程 seed 和稳定属性身份生成，每帧重置；timeless 保持跨时间随机序列。序列与 AE 不同；正序、倒序、随机寻帧一致。没有 gaussRandom。 |
| linear / ease / easeIn / easeOut | 支持 3 参数及 5 参数形式、标量/向量。ease 为 smoothstep，easeIn/Out 为二次曲线；尚未完成 AE 数值对照。 |
| 向量与单位 helper | add/sub/mul/div/clamp/length/normalize/dot/cross、degreesToRadians/radiansToDegrees、framesToTime/timeToFrames。 |

属性支持范围：图层 position/rotation/scale 三维向量、opacity 标量；单轴 x/y/z；显式摄影机 position/target/roll/fov/radius/azimuth/elevation；效果连续数值/向量/颜色参数。position/scale 等整个属性必须返回恰好 3 个数，单轴或标量必须返回一个数，效果必须匹配描述的维度。两个分量的表达式应用到三维属性时需补第三项。

position 使用图层父级局部空间的像素，scale 使用百分比，rotation/roll 使用度，opacity 对表达式暴露 0..100、最终限制到 0..100（工程继续存 0..1）。效果参数按插件元数据的单位和范围。fov/orbit 参数是 Motion Studio 扩展，不等同于 AE camera zoom；rotation 是 XYZ 向量，不等同于 AE 的单个 Z Rotation 标量，应选择 z 轴 target。

当前不支持跨图层/跨属性依赖、递归表达式、mask/path/text/document 等非数值结果、posterizeTime、smooth、temporalWiggle、loopInDuration/loopOutDuration、外部表达式库和 JSXBIN。不支持的宿主函数调用报错，不以空值假装执行成功。后续对象引用需要增加依赖图、循环诊断与属性对象模型。
