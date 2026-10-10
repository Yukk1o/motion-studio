import {EditorBridge} from "./bridge.js";

const $ = id => document.getElementById(id);
const clone = value => JSON.parse(JSON.stringify(value));
let tab = "emitter", selectedElement = null, drag = null, batch = false;
let previewBusy = false, previewDirty = false, previewTimer, previewEpoch = 0;
const bridge = new EditorBridge(() => { if (!drag) render(); else renderChrome(); });
const particleTabs = [["emitter", "发射"], ["motion", "运动"], ["appearance", "外观"], ["transform", "变换"]];
const lensTabs = [["source", "光源"], ["elements", "镜头元件"], ["appearance", "总控"]];
const shapeNames = {glow: "光晕", halo: "光环", ghost: "鬼影", streak: "光条", star: "星芒"};
const shapeIcons = {glow: "●", halo: "◎", ghost: "◉", streak: "━", star: "✦"};
const softRanges = {rate: [0, 500], lifetime: [.01, 12], speed: [-600, 600], spread: [0, 400],
  size: [0, 100], end_size: [0, 100], intensity: [0, 6], scale: [0, 300], reference_distance: [1, 4000]};

function node(tag, text, parent, className) {
  const result = document.createElement(tag);
  if (text != null) result.textContent = text;
  if (className) result.className = className;
  if (parent) parent.append(result);
  return result;
}
function status(text, error = false) { $("status").textContent = text; $("status").classList.toggle("error", error); }
function busy() { return !bridge.state || bridge.state.locked || batch; }
function attempt(work) {
  if (busy() || drag) return;
  const epoch = bridge.epoch;
  Promise.resolve().then(work).then(() => { if (epoch === bridge.epoch) { status("已更新"); schedulePreview(); } })
    .catch(error => { if (epoch === bridge.epoch) status(error.message, true); });
}
function notice(text, parent, error = false) { return node("p", text, parent, "notice" + (error ? " error" : "")); }
function button(text, parent, action, className = "quiet") {
  const result = node("button", text, parent, className); result.type = "button";
  result.disabled = busy(); result.addEventListener("click", action); return result;
}
function group(title, parent) { const box = node("fieldset", null, parent, "group"); node("legend", title, box); return box; }
function valid(value, min, max, integer = false) {
  if (!Number.isFinite(value) || value < min || value > max || (integer && !Number.isInteger(value)))
    throw new Error(`请输入 ${min}～${max} 之间的${integer ? "整数" : "数值"}`);
  return value;
}
function parameterBuild(id, component, value) {
  return state => {
    const p = bridge.definition.params.find(item => item.id === id);
    valid(value, p.min, p.max, p.kind === "enum" || p.kind === "bool");
    const next = state.values[id].slice(); next[component] = value;
    if (bridge.definition.renderer === "particles" && (id === "rate" || id === "lifetime")) {
      const rate = id === "rate" ? value : state.values.rate[0];
      const life = id === "lifetime" ? value : state.values.lifetime[0];
      if (Math.ceil(rate * life) > 20000) throw new Error("出生速率 × 寿命超过 20,000 个粒子，请调整其中一项");
    }
    return {op: "set", param: id, value: next};
  };
}
function inputNumber(value, min, max, name, parent, changed, integer = false) {
  const input = node("input", null, parent); input.type = "number";
  input.min = String(min); input.max = String(max); input.step = integer ? "1" : "any";
  input.value = Number.isInteger(value) ? String(value) : String(Number(value.toPrecision(7)));
  input.setAttribute("aria-label", name);
  input.disabled = busy(); input.dataset.focus = name;
  input.addEventListener("input", () => input.removeAttribute("aria-invalid"));
  input.addEventListener("change", () => {
    try {
      if (!input.value.trim()) throw new Error("数值不能为空");
      changed(valid(Number(input.value), min, max, integer));
    } catch (error) { input.setAttribute("aria-invalid", "true"); status(error.message, true); }
  });
  return input;
}
function heading(name, units, parent, parameter) {
  const row = node("div", null, parent, "control-heading");
  const label = node("label", name, row); if (units) node("span", units, label, "unit");
  if (parameter?.animatable) {
    const active = Boolean(bridge.state.params[parameter.id]?.track?.keys?.length);
    const key = button(active ? "◆" : "◇", row, () => attempt(() => bridge.edit(state => ({op: "animate", param: parameter.id,
      enabled: !state.params[parameter.id].track.keys.length}))), "key");
    key.setAttribute("aria-label", `${parameter.name}关键帧`); key.setAttribute("aria-pressed", String(active));
    key.title = active ? "关闭动画，保留当前帧数值" : "在当前帧开启动画";
  }
  return label;
}
function slider(name, value, min, max, parent, build, parameter) {
  const row = node("div", null, parent, "number-row"), range = node("input", null, row);
  const soft = softRanges[parameter?.id];
  range.type = "range"; range.min = String(soft ? Math.max(min, Math.min(soft[0], value)) : min);
  range.max = String(soft ? Math.min(max, Math.max(soft[1], value)) : max);
  range.step = String(parameter?.step || .01); range.value = String(value);
  range.setAttribute("aria-label", `${name}滑块`); range.disabled = busy();
  const number = inputNumber(value, min, max, name, row, v => attempt(() => bridge.edit(build(v))));
  range.addEventListener("pointerdown", event => startDrag(event, range, number, build));
  range.addEventListener("input", () => {
    number.value = range.value;
    if (drag?.range === range) {
      drag.latest = Number(range.value);
      if (!drag.timer && !drag.sending) drag.timer = setTimeout(flushDrag, 90);
    } else attempt(() => bridge.edit(build(Number(range.value))));
  });
  range.addEventListener("pointerup", () => finishDrag(false));
  range.addEventListener("pointercancel", () => finishDrag(true));
  range.addEventListener("lostpointercapture", () => { if (drag?.range === range && !drag.ending) finishDrag(true); });
  return number;
}
function startDrag(event, range, number, build) {
  if (busy() || drag || event.button !== 0) return;
  range.setPointerCapture(event.pointerId);
  drag = {range, number, build, latest: null, timer: null, sending: false, ending: false, epoch: bridge.epoch};
  const current = drag;
  current.started = bridge.edit(() => ({op: "begin"}));
  current.started.catch(error => { if (drag === current) { status(error.message, true); finishDrag(true); } });
  renderChrome();
}
function flushDrag() {
  const current = drag; if (!current) return;
  clearTimeout(current.timer); current.timer = null;
  if (current.sending || current.ending) return;
  const value = current.latest; current.latest = null;
  if (value == null) return;
  current.sending = true;
  current.sent = bridge.edit(current.build(value)).then(() => schedulePreview()).catch(error => {
    current.error = error;
    if (drag === current) { status(error.message, true); finishDrag(true); }
  }).finally(() => {
    current.sending = false;
    if (drag === current && !current.ending && current.latest != null && !current.timer)
      current.timer = setTimeout(flushDrag, 90);
  });
}
async function finishDrag(cancel) {
  const current = drag; if (!current || current.ending) return;
  current.ending = true; clearTimeout(current.timer);
  try {
    await current.started.catch(() => { cancel = true; });
    await current.sent;
    if (current.error) cancel = true;
    if (!cancel && current.latest != null) {
      const value = current.latest; current.latest = null;
      await bridge.edit(current.build(value));
    }
    await bridge.edit(state => state.gesture ? {op: cancel ? "cancel" : "commit"} : null);
    if (bridge.epoch === current.epoch)
      status(current.error ? current.error.message : cancel ? "已取消本次拖动" : "已更新 · 一次撤销", Boolean(current.error));
  } catch (error) {
    if (bridge.epoch === current.epoch) {
      if (bridge.state?.gesture) { try { await bridge.edit(() => ({op: "cancel"})); } catch { /* Host close cancels if transport is unavailable. */ } }
      status(error.message, true);
    }
  }
  finally {
    if (drag === current) { drag = null; render(); schedulePreview(); }
  }
}
function parameter(id, parent) {
  const p = bridge.definition.params.find(item => item.id === id), value = bridge.state.values[id];
  if (!p || !value) return;
  const box = node("div", null, parent, "control"), label = heading(p.name, p.units, box, p);
  if (p.kind === "enum" || p.kind === "bool") {
    const select = node("select", null, box, "select"), options = p.kind === "bool" ? ["关闭", "启用"] : p.options;
    options.forEach((name, index) => { const option = node("option", name, select); option.value = String(p.min + index); });
    select.value = String(value[0]); select.disabled = busy(); select.setAttribute("aria-label", p.name);
    select.addEventListener("change", () => attempt(() => bridge.edit(parameterBuild(id, 0, Number(select.value)))));
  } else if (p.kind === "float") {
    const input = slider(p.name, value[0], p.min, p.max, box, v => parameterBuild(id, 0, v), p);
    input.id = `param-${id}`; label.htmlFor = input.id;
  } else if (p.kind === "color") {
    color(value, p.name, box, (c, v) => parameterBuild(id, c, v));
  } else {
    const count = p.kind === "vec2" ? 2 : 3, row = node("div", null, box, "vector");
    for (let i = 0; i < count; i++) {
      const axis = node("label", null, row, "axis"); node("span", ["X", "Y", "Z"][i], axis);
      inputNumber(value[i], p.min, p.max, `${p.name} ${["X", "Y", "Z"][i]}`, axis,
        v => attempt(() => bridge.edit(parameterBuild(id, i, v))));
    }
  }
  if (bridge.state.expressions?.some(e => e.enabled && e.target.param === id))
    node("p", "表达式已启用：此处编辑基础值，预览使用表达式结果。", box, "hint");
}
function color(value, name, parent, build) {
  const row = node("div", null, parent, "color-heading"), picker = node("input", null, row);
  picker.type = "color"; picker.value = "#" + value.slice(0, 3).map(v => Math.round(v * 255).toString(16).padStart(2, "0")).join("");
  picker.setAttribute("aria-label", `${name}拾色器`); picker.disabled = busy();
  const strip = node("div", null, row, "color-strip"); strip.style.backgroundColor = picker.value;
  picker.addEventListener("change", () => attempt(() => bridge.transaction([0, 1, 2].map(i => build(i, parseInt(picker.value.slice(1 + i * 2, 3 + i * 2), 16) / 255)))));
  const channels = node("div", null, parent, "vector rgba");
  for (let i = 0; i < 4; i++) {
    const axis = node("label", null, channels, "axis"); node("span", ["R", "G", "B", "A"][i], axis);
    inputNumber(value[i], 0, 1, `${name} ${["R", "G", "B", "A"][i]}`, axis,
      v => attempt(() => bridge.edit(build(i, v))));
  }
}
function renderChrome() {
  const state = bridge.state;
  $("connection").textContent = state ? (state.locked ? "图层已锁定" : "已连接") : "未连接";
  $("connection").classList.toggle("ready", Boolean(state && !state.locked));
  $("refresh").disabled = !state;
  $("reset").disabled = busy() || Boolean(drag);
  $("frame").textContent = state ? `帧 ${state.frame}` : "—";
  $("cancel-gesture").hidden = !drag;
}
function render() {
  renderChrome(); const state = bridge.state; if (!state) return;
  const isLens = bridge.definition.renderer === "lens_flare", tabs = isLens ? lensTabs : particleTabs;
  if (!tabs.some(item => item[0] === tab)) tab = tabs[0][0];
  $("title").textContent = bridge.definition.name;
  $("effect-subtitle").textContent = bridge.definition.english_name;
  $("inspector-title").textContent = isLens ? "镜头设计" : "粒子设计";
  $("context-hint").textContent = isLens ? "光源随合成摄影机投影；遮挡读取图层的源 Alpha。" : "粒子在发射器局部空间运动，移动图层也会带动已出生粒子。";
  const navigation = $("tabs"); navigation.replaceChildren(); navigation.setAttribute("role", "tablist");
  for (const [id, name] of tabs) {
    const item = button(name, navigation, () => { if (!drag) { tab = id; render(); } }, "tab");
    item.disabled = Boolean(drag); item.setAttribute("aria-selected", String(id === tab));
    item.setAttribute("role", "tab"); item.setAttribute("aria-controls", "controls"); item.tabIndex = id === tab ? 0 : -1;
    item.addEventListener("keydown", event => {
      const position = tabs.findIndex(entry => entry[0] === id);
      const next = event.key === "ArrowRight" ? (position + 1) % tabs.length : event.key === "ArrowLeft" ? (position + tabs.length - 1) % tabs.length : event.key === "Home" ? 0 : event.key === "End" ? tabs.length - 1 : -1;
      if (next >= 0 && !drag) { event.preventDefault(); tab = tabs[next][0]; render(); $("tabs").querySelector('[aria-selected="true"]').focus(); }
    });
  }
  const focus = document.activeElement?.dataset.focus;
  const controls = $("controls"); controls.replaceChildren();
  if (state.locked) notice("当前图层已锁定。解锁后可继续编辑。", controls);
  if (isLens) renderLens(controls); else renderParticles(controls);
  if (focus) [...controls.querySelectorAll("input")].find(input => input.dataset.focus === focus)?.focus({preventScroll: true});
  renderPresets();
}
function renderParticles(parent) {
  if (tab === "emitter") {
    const budget = node("div", null, parent, "budget"); node("span", "存活容量", budget);
    node("strong", `${Math.ceil(bridge.state.values.rate[0] * bridge.state.values.lifetime[0]).toLocaleString()} / 20,000`, budget);
    const box = group("发射器", parent); ["shape", "extent", "rate", "lifetime", "prewarm"].forEach(id => parameter(id, box));
    const seed = group("随机性", parent);
    inputNumber(bridge.state.seed, 0, 4294967295, "随机种子", seed,
      value => attempt(() => bridge.edit(() => ({op: "seed", seed: value}))), true);
    node("p", "相同种子可重复生成相同粒子。", seed, "hint");
  } else if (tab === "motion") {
    const box = group("运动", parent); ["speed", "spread", "gravity"].forEach(id => parameter(id, box));
    node("p", "纵向速度为正时向上，重力 Y 为正时向下。出生与运动参数暂不支持关键帧。", box, "hint");
  } else if (tab === "appearance") {
    const size = group("尺寸与淡入淡出", parent); ["size", "end_size", "fade"].forEach(id => parameter(id, size));
    const colors = group("寿命颜色", parent); ["color", "end_color"].forEach(id => parameter(id, colors));
  } else {
    const values = bridge.state.transform_values;
    if (!values) { notice("当前宿主尚未提供变换采样值，请在图层面板编辑变换。", parent); return; }
    for (const [id, name, unit, low, high] of [["position", "发射器位置", "px", -1e7, 1e7], ["rotation", "旋转", "°", -1e6, 1e6], ["scale", "缩放", "%", -1e5, 1e5]]) {
      const box = group(`${name} · ${unit}`, parent), row = node("div", null, box, "vector");
      for (let i = 0; i < 3; i++) {
        const axis = node("label", null, row, "axis"); node("span", ["X", "Y", "Z"][i], axis);
        inputNumber(values[id][i], low, high, `${name} ${["X", "Y", "Z"][i]}`, axis, v => attempt(() => bridge.edit(state => {
          const next = state.transform_values[id].slice(); next[i] = v; return {op: "transform", property: id, value: next};
        })));
      }
    }
  }
}
function sceneBuild(change) {
  return state => { const settings = clone(state.scene); change(settings); return {op: "scene", settings}; };
}
function elementBuild(id, key, value) {
  return sceneBuild(settings => {
    const item = settings.elements.find(item => item.id === id);
    if (!item) throw new Error("元件已被删除，请重新选择");
    item[key] = typeof value === "function" ? value(item[key]) : value;
  });
}
function newId(elements) {
  let id = 1; const used = new Set(elements.map(item => item.id)); while (used.has(id)) id++;
  if (id > 4294967295) throw new Error("无法分配元件 ID"); return id;
}
function renderLens(parent) {
  const settings = bridge.state.scene;
  if (tab === "source") {
    const box = group("光源绑定", parent), select = node("select", null, box, "select");
    const manual = node("option", "手动位置", select); manual.value = "";
    for (const layer of bridge.state.layers) { const option = node("option", layer.name, select); option.value = String(layer.id); }
    if (settings.source_layer != null && !bridge.state.layers.some(layer => layer.id === settings.source_layer)) {
      const missing = node("option", `已缺失的图层 #${settings.source_layer}`, select); missing.value = String(settings.source_layer);
      notice("光源引用的图层已缺失，请重新绑定或选择手动位置。", parent, true);
    }
    select.value = settings.source_layer == null ? "" : String(settings.source_layer); select.disabled = busy(); select.setAttribute("aria-label", "跟随图层");
    select.addEventListener("change", () => attempt(() => bridge.edit(sceneBuild(s => { s.source_layer = select.value ? Number(select.value) : null; }))));
    if (settings.source_layer == null) parameter("position", box); else node("p", "跟随所选图层或 Null 的世界枢轴。位置请在该图层上调整。", box, "hint");
    const occlusion = group("遮挡", parent), label = node("label", null, occlusion, "check"), check = node("input", null, label);
    check.type = "checkbox"; check.checked = settings.occlusion; check.disabled = busy(); node("span", "启用图层 Alpha 遮挡", label);
    check.addEventListener("change", () => attempt(() => bridge.edit(sceneBuild(s => { s.occlusion = check.checked; }))));
    parameter("occlusion_radius", occlusion); node("p", "使用效果链之前的图层 Alpha；半透明图层会部分衰减光效。", occlusion, "hint");
  } else if (tab === "appearance") {
    const box = group("整体光效", parent); ["intensity", "scale", "attenuation", "reference_distance"].forEach(id => parameter(id, box));
  } else renderElements(parent, settings.elements);
}
function renderElements(parent, elements) {
  if (!elements.some(item => item.id === selectedElement)) selectedElement = elements[0]?.id ?? null;
  const list = node("div", null, parent, "element-list");
  for (const item of elements) {
    const row = node("div", null, list, "element-row"), select = button("", row, () => { selectedElement = item.id; render(); }, "element-select");
    select.disabled = Boolean(drag); select.setAttribute("aria-pressed", String(selectedElement === item.id));
    select.setAttribute("aria-label", `选择${shapeNames[item.shape]} #${item.id}`);
    node("span", shapeIcons[item.shape], select, "shape-icon"); node("span", shapeNames[item.shape] + (item.enabled ? "" : " · 已停用"), select, "element-name"); node("small", `#${item.id}`, select);
  }
  const add = button("＋ 添加镜头元件", parent, () => attempt(() => bridge.edit(sceneBuild(s => {
    if (s.elements.length >= 64) throw new Error("最多支持 64 个镜头元件");
    const id = newId(s.elements); selectedElement = id;
    s.elements.push({id, shape: "glow", enabled: true, offset: 0, size: [150, 150], color: [1, 1, 1, 1], intensity: 1, rays: 8, chromatic: 0});
  }))), "add-element"); add.disabled ||= elements.length >= 64;
  if (!elements.length) { node("p", "添加光晕、鬼影或光条，开始设计镜头光效。", parent, "empty"); return; }
  const item = elements.find(item => item.id === selectedElement), id = item.id, index = elements.indexOf(item);
  const actions = node("div", null, parent, "element-actions");
  for (const [text, amount] of [["上移", -1], ["下移", 1]]) {
    const move = button(text, actions, () => attempt(() => bridge.edit(sceneBuild(s => {
      const at = s.elements.findIndex(e => e.id === id), to = at + amount;
      if (at < 0 || to < 0 || to >= s.elements.length) throw new Error("元件顺序已改变");
      [s.elements[at], s.elements[to]] = [s.elements[to], s.elements[at]];
    })))); move.disabled ||= index + amount < 0 || index + amount >= elements.length;
  }
  const duplicate = button("复制", actions, () => attempt(() => bridge.edit(sceneBuild(s => {
    if (s.elements.length >= 64) throw new Error("最多支持 64 个镜头元件");
    const at = s.elements.findIndex(e => e.id === id); if (at < 0) throw new Error("元件已被删除");
    const copy = clone(s.elements[at]); copy.id = newId(s.elements); selectedElement = copy.id; s.elements.splice(at + 1, 0, copy);
  })))); duplicate.disabled ||= elements.length >= 64;
  button("删除", actions, () => attempt(() => bridge.edit(sceneBuild(s => { s.elements = s.elements.filter(e => e.id !== id); }))), "danger");
  const box = group(`${shapeNames[item.shape]} #${id}`, parent), enabled = node("label", null, box, "check"), check = node("input", null, enabled);
  check.type = "checkbox"; check.checked = item.enabled; check.disabled = busy(); node("span", "启用元件", enabled);
  check.addEventListener("change", () => attempt(() => bridge.edit(elementBuild(id, "enabled", check.checked))));
  const type = node("select", null, box, "select"); type.setAttribute("aria-label", "元件类型"); type.disabled = busy();
  for (const [key, name] of Object.entries(shapeNames)) { const option = node("option", name, type); option.value = key; }
  type.value = item.shape; type.addEventListener("change", () => attempt(() => bridge.edit(elementBuild(id, "shape", type.value))));
  for (const [key, name, min, max] of [["offset", "轴线偏移", -8, 8], ["intensity", "元件强度", 0, 32], ["chromatic", "色差", 0, .5]]) {
    const control = node("div", null, box, "control"); heading(name, "", control);
    slider(name, item[key], min, max, control, v => elementBuild(id, key, v));
  }
  const size = node("div", null, box, "vector dimensions");
  item.size.forEach((v, i) => {
    const axis = node("label", null, size, "axis"); node("span", i === 0 ? "宽 / px" : "高 / px", axis);
    inputNumber(v, Number.MIN_VALUE, 8192, i === 0 ? "元件宽度" : "元件高度", axis,
      next => attempt(() => bridge.edit(elementBuild(id, "size", values => values.map((v, c) => c === i ? next : v)))));
  });
  if (item.shape === "star") inputNumber(item.rays, 2, 32, "星芒数", box,
    v => attempt(() => bridge.edit(elementBuild(id, "rays", v))), true);
  heading("元件颜色", "", box); color(item.color, "元件颜色", box, (c, v) => elementBuild(id, "color", values => values.map((x, i) => i === c ? v : x)));
  node("p", "偏移 0 位于光源，1 位于画面中心，2 位于光源关于中心的对侧。", box, "hint");
}
function renderPresets() {
  $("preset-panel").hidden = bridge.definition.renderer !== "particles";
  const parent = $("presets"); parent.replaceChildren();
  for (const [name, rateScale, sizeScale] of [["轻盈", .5, .75], ["均衡", 1, 1], ["密集", 1.75, 1]])
    button(name, parent, () => attempt(() => applyPreset(rateScale, sizeScale)), "preset");
}
async function applyPreset(rateScale, sizeScale) {
  batch = true; render();
  try {
    const changes = ["rate", "size", "end_size"].map(id => state => {
      const p = state.params[id], next = p.default.slice(); next[0] *= id === "rate" ? rateScale : sizeScale;
      return parameterBuild(id, 0, next[0])(state);
    });
    await bridge.transaction(changes);
  } finally { batch = false; render(); }
}
async function reset() {
  batch = true; render();
  try {
    // Preserve animation tracks: restoring a value edits only the current key.
    const defaults = clone(bridge.state.params), ids = Object.keys(defaults);
    const builds = ids.map(id => () => ({op: "set", param: id, value: defaults[id].default.slice()}));
    if (bridge.definition.renderer === "lens_flare") builds.push(() => ({op: "scene", settings: clone(bridge.definition.scene)}));
    await bridge.transaction(builds);
  } finally { batch = false; render(); }
}
function schedulePreview(force = false) {
  if (!bridge.state || (!force && !$("auto-preview").checked)) return;
  previewDirty = true; clearTimeout(previewTimer);
  previewTimer = setTimeout(() => preview(), force ? 0 : 150);
}
async function preview() {
  if (previewBusy || !bridge.state) return;
  previewBusy = true; previewDirty = false;
  const epoch = bridge.epoch, imageEpoch = previewEpoch, revision = bridge.state.revision, frame = bridge.state.frame;
  const [width, height] = bridge.state.dimensions, scale = Math.min(1, 512 / Math.max(width, height));
  $("preview-state").hidden = false; $("preview-state").textContent = "更新中…";
  try {
    const result = await bridge.request({op: "preview", width: Math.max(1, Math.round(width * scale)), height: Math.max(1, Math.round(height * scale))});
    if (epoch !== bridge.epoch || imageEpoch !== previewEpoch) return;
    if (revision !== bridge.state.revision || frame !== bridge.state.frame || result.revision !== revision || result.frame !== frame) {
      previewDirty = true; return;
    }
    const image = new Image();
    await new Promise((resolve, reject) => { image.onload = resolve; image.onerror = () => reject(new Error("预览图像无法解码")); image.src = "data:image/png;base64," + result.png; });
    if (epoch !== bridge.epoch || imageEpoch !== previewEpoch || revision !== bridge.state.revision || frame !== bridge.state.frame) { previewDirty = true; return; }
    const canvas = $("preview"); canvas.width = result.width; canvas.height = result.height;
    canvas.getContext("2d").drawImage(image, 0, 0); $("preview-empty").hidden = true;
    for (const id of ["alive", "visible", "culled"]) $(id).textContent = result.instances[id].toLocaleString();
    $("preview-state").hidden = true;
  } catch (error) {
    if (epoch === bridge.epoch) { $("preview-state").textContent = "预览未更新"; status(error.message, true); }
  } finally {
    previewBusy = false;
    if (previewDirty && bridge.state) schedulePreview(true);
  }
}
window.motionStudioReply = reply => bridge.reply(reply);
window.motionStudioConnect = connection => {
  previewEpoch++; tab = connection?.definition?.renderer === "lens_flare" ? "source" : "emitter";
  drag = null; selectedElement = null; batch = false;
  try { bridge.connect(connection); $("preview-empty").hidden = false; status("已连接 · 修改即时应用，拖动可撤销"); schedulePreview(true); }
  catch (error) { status(error.message, true); }
};
window.motionStudioUpdate = state => {
  try { bridge.accept(state); schedulePreview(); } catch (error) { status(error.message, true); }
};
window.motionStudioDisconnect = () => {
  previewEpoch++; clearTimeout(previewTimer); clearTimeout(drag?.timer);
  bridge.disconnect(); drag = null; previewDirty = false; batch = false; renderChrome();
  $("controls").replaceChildren(); $("tabs").replaceChildren(); $("presets").replaceChildren();
  $("preview-empty").hidden = false; $("preview-empty").textContent = "编辑器连接已关闭";
  $("preview").getContext("2d").clearRect(0, 0, $("preview").width, $("preview").height);
  for (const id of ["alive", "visible", "culled"]) $(id).textContent = "—";
  status("连接已关闭");
};
$("refresh").addEventListener("click", () => schedulePreview(true));
$("auto-preview").addEventListener("change", () => { if ($("auto-preview").checked) schedulePreview(true); });
$("reset").addEventListener("click", () => attempt(reset));
$("cancel-gesture").addEventListener("click", () => finishDrag(true));
window.addEventListener("keydown", event => { if (event.key === "Escape" && drag) { event.preventDefault(); finishDrag(true); } });
window.addEventListener("pagehide", () => { if (bridge.state?.gesture) bridge.request({op: "cancel", revision: bridge.state.revision}).catch(() => {}); });
