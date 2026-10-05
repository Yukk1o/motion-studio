# Motion Studio 属性表达式

后端首版采用 QuickJS-NG（rquickjs 0.14.0）执行真正的 JavaScript。SWC 解析源码并适配 AE 的向量运算与末尾语句结果。兼容配置固定为 `motion-studio-ae-js-1`；这不是 Adobe 的引擎，也不代表所有 AE 表达式已经兼容。未实现脚本自动化系统或表达式编辑界面。

- [前端接口及接入约定](frontend-integration.md)
- [AE 兼容范围与差异](compatibility.md)
- [运行时、资源与工程格式](runtime.md)
- [验证记录](validation.md)

可以直接使用的例子：

```js
// 图层位置（三维；单位像素）
value + [Math.sin(time * 2) * 50, 0, 0]
```

```js
// 图层缩放
wiggle(2, 10)
```

```js
// 有关键帧的属性，最后一个关键帧之后往返循环
loopOut("pingpong")
```

```js
// 图层透明度：表达式使用百分比，后端工程仍存 0..1
linear(time, 0, 1, 0, 100)
```

```js
// 多行语句的最后一个表达式为结果
var t = time - key(1).time;
var delta = [t * 30, 0, 0];
value + delta;
```

许可：QuickJS-NG 和 rquickjs 为 MIT，SWC 为 Apache-2.0。随 APK 打包的许可文本位于 `android/app/src/main/assets/third-party/`。AE 宿主函数由本项目独立实现，没有嵌入 Adobe 的运行时或拷贝第三方收费表达式库。Adobe 公开文档用于理解行为，不等于 Adobe 引擎代码的分发授权。

参考：[Adobe 表达式引擎说明](https://helpx.adobe.com/cn/after-effects/desktop/work-with-expressions/expression-basics/legacy-and-extend-script-engine.html)、[Adobe 表达式语言参考](https://helpx.adobe.com/after-effects/desktop/work-with-expressions/expression-language-reference/expression-language-reference.html)、[QuickJS-NG](https://github.com/quickjs-ng/quickjs)、[rquickjs](https://github.com/DelSkayn/rquickjs)、[SWC](https://github.com/swc-project/swc)。
