/* Packaged page + real Rust editor commands + real wgpu PNG preview. */
import {chromium} from "playwright";
import {createServer} from "node:http";
import {readFile, mkdir, writeFile} from "node:fs/promises";
import {spawn} from "node:child_process";
import {createInterface} from "node:readline";
import {fileURLToPath} from "node:url";
import path from "node:path";
import assert from "node:assert/strict";

const root = fileURLToPath(new URL("../../", import.meta.url));
const ui = path.join(root, "crates/aem-effects/scene-library/ui");
const output = path.join(root, "artifacts/plugin-editor-browser");
await mkdir(output, {recursive: true});
const executable = process.env.EDITOR_PROBE_BIN || path.join(root, "target/debug/plugin_editor_probe" + (process.platform === "win32" ? ".exe" : ""));
const probe = spawn(executable, [], {stdio: ["pipe", "pipe", "pipe"]});
const callbacks = []; let stderr = "";
probe.stderr.on("data", chunk => { stderr += chunk; });
probe.on("error", error => { for (const task of callbacks.splice(0)) task.reject(error); });
probe.on("exit", code => { for (const task of callbacks.splice(0)) task.reject(new Error(`Native probe exited ${code}: ${stderr}`)); });
createInterface({input: probe.stdout}).on("line", line => {
  const task = callbacks.shift(); if (!task) return;
  try { const reply = JSON.parse(line); if (reply.ok) task.resolve(reply.result); else task.reject(new Error(reply.error)); }
  catch (error) { task.reject(error); }
});
function call(input) { return new Promise((resolve, reject) => { callbacks.push({resolve, reject}); probe.stdin.write(JSON.stringify(input) + "\n"); }); }
const types = {"html": "text/html", "css": "text/css", "js": "text/javascript"};
const allowed = new Set(["editor.html", "editor.css", "editor.js", "bridge.js"]);
const server = createServer(async (req, res) => {
  const name = new URL(req.url, "http://localhost").pathname.slice(1);
  if (!allowed.has(name)) { res.writeHead(404); res.end(); return; }
  try { res.writeHead(200, {"Content-Type": `${types[name.split(".").at(-1)]}; charset=utf-8`}); res.end(await readFile(path.join(ui, name))); }
  catch { res.writeHead(500); res.end(); }
});
await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
const address = `http://127.0.0.1:${server.address().port}/editor.html`;
const errors = [], violations = [], traces = [], results = [];
let transportDelay = 0;
let failNextSet = false;
let browser;
try {
  browser = await chromium.launch({headless: true, ...(process.env.PW_BROWSER_PATH ? {executablePath: process.env.PW_BROWSER_PATH} : {})});
  const page = await browser.newPage({viewport: {width: 1024, height: 900}});
  page.on("pageerror", error => errors.push(error.message));
  await page.exposeBinding("editorTransport", async (_source, raw) => {
    const request = JSON.parse(raw); traces.push(request.message);
    try {
      if (failNextSet && request.message.op === "set") { failNextSet = false; throw new Error("测试：宿主拒绝修改"); }
      if (transportDelay && request.message.op === "set") await new Promise(resolve => setTimeout(resolve, transportDelay));
      const result = await call({op: "message", message: request.message});
      if (result.png_bytes) { result.png = Buffer.from(result.png_bytes).toString("base64"); delete result.png_bytes; }
      return {token: request.token, id: request.id, ok: true, result};
    } catch (error) { return {token: request.token, id: request.id, ok: false, error: error.message}; }
  });
  await page.exposeBinding("cspViolation", (_source, directive) => violations.push(directive));
  await page.addInitScript(() => {
    window.__editorTestPending = 0;
    window.MotionStudioHost = {postMessage(raw) {
      window.__editorTestPending++;
      window.editorTransport(raw).then(reply => window.motionStudioReply(reply)).finally(() => window.__editorTestPending--);
    }};
    document.addEventListener("securitypolicyviolation", event => window.cspViolation(event.violatedDirective));
  });
  await page.goto(address); await page.waitForFunction(() => typeof window.motionStudioConnect === "function");
  async function open(effect) {
    const connection = await call({op: "open", effect});
    await page.evaluate(connection => window.motionStudioConnect(connection), connection);
    await page.waitForFunction(() => document.getElementById("preview-empty").hidden);
    return connection;
  }
  async function state() { return call({op: "message", message: {op: "state"}}); }
  async function waitState(predicate) {
    for (let n = 0; n < 100; n++) { const value = await state(); if (predicate(value)) return value; await new Promise(resolve => setTimeout(resolve, 30)); }
    throw new Error("Expected backend state was not reached");
  }
  async function change(label, value) {
    const input = page.getByRole("spinbutton", {name: label, exact: true});
    await input.fill(String(value)); await input.press("Tab");
  }
  async function tab(name) { await page.getByRole("tab", {name, exact: true}).click(); }
  async function checkLayout(label) {
    for (const width of [320, 375, 414, 768, 1024, 1440]) {
      await page.setViewportSize({width, height: 900});
      const dims = await page.evaluate(() => ({scroll: document.documentElement.scrollWidth, viewport: innerWidth,
        short: [...document.querySelectorAll("button,input[type=number],select")].filter(e => e.offsetWidth && e.offsetHeight && e.getBoundingClientRect().height < 47).map(e => e.getAttribute("aria-label") || e.textContent)}));
      assert.ok(dims.scroll <= dims.viewport, `${label} overflows at ${width}: ${JSON.stringify(dims)}`);
      assert.deepEqual(dims.short, [], `${label} touch targets at ${width}`);
      if ([375, 1024].includes(width)) await page.screenshot({path: path.join(output, `${label}-${width}.png`), fullPage: true});
      if (width === 1024) await page.screenshot({path: path.join(output, `${label}-window.png`)});
    }
    results.push(`${label}: six viewport widths, 48px touch targets`);
  }
  await open("lens_flare"); await tab("镜头元件"); await checkLayout("lens-elements");
  await page.setViewportSize({width: 1024, height: 900});
  await page.getByRole("button", {name: "复制", exact: true}).click();
  let s = await waitState(s => s.scene.elements.length === 8);
  assert.equal(new Set(s.scene.elements.map(e => e.id)).size, 8);
  const duplicate = s.scene.elements[1].id;
  await change("元件宽度", .5); await waitState(s => s.scene.elements.find(e => e.id === duplicate)?.size[0] === .5);
  await page.getByRole("button", {name: "下移", exact: true}).click();
  await waitState(s => s.scene.elements[2].id === duplicate);
  await page.getByRole("button", {name: "删除", exact: true}).click();
  await waitState(s => s.scene.elements.length === 7 && !s.scene.elements.some(e => e.id === duplicate));
  results.push("lens stable IDs: duplicate, fractional size, reorder, delete");
  await tab("总控");
  await change("亮度", 2); await change("尺寸", 150);
  await waitState(s => s.values.intensity[0] === 2 && s.values.scale[0] === 150);
  const badStart = traces.filter(m => m.op === "set").length;
  await change("亮度", 999); await page.waitForFunction(() => document.querySelector('[aria-label="亮度"]').getAttribute("aria-invalid") === "true");
  assert.equal(traces.filter(m => m.op === "set").length, badStart);
  assert.equal((await state()).values.intensity[0], 2);
  await change("亮度", 2);
  await page.getByRole("button", {name: "亮度关键帧", exact: true}).click();
  await waitState(s => s.params.intensity.track.keys.length > 0);
  const beforeDrag = (await state()).values.intensity[0], traceStart = traces.length;
  const slider = page.getByRole("slider", {name: "亮度滑块", exact: true});
  const bounds = await slider.boundingBox();
  await page.mouse.move(bounds.x + bounds.width*.35, bounds.y + bounds.height/2); await page.mouse.down();
  await page.mouse.move(bounds.x + bounds.width*.6, bounds.y + bounds.height/2, {steps: 8}); await page.mouse.up();
  await waitState(s => !s.gesture && s.values.intensity[0] !== beforeDrag);
  assert.equal(traces.slice(traceStart).filter(m => m.op === "begin").length, 1);
  assert.equal(traces.slice(traceStart).filter(m => m.op === "commit").length, 1);
  const undone = await call({op: "host", action: "undo"}); assert.equal(undone.values.intensity[0], beforeDrag);
  await page.evaluate(s => window.motionStudioUpdate(s), undone);
  results.push("serial edits, numeric bounds, animate, drag transaction and real Rust undo");
  const cancelBefore = (await state()).values.intensity[0], cancelStart = traces.length;
  const cancelBounds = await page.getByRole("slider", {name: "亮度滑块", exact: true}).boundingBox();
  await page.mouse.move(cancelBounds.x + cancelBounds.width*.3, cancelBounds.y + cancelBounds.height/2); await page.mouse.down();
  await page.mouse.move(cancelBounds.x + cancelBounds.width*.8, cancelBounds.y + cancelBounds.height/2, {steps: 5});
  await waitState(s => s.gesture && s.values.intensity[0] !== cancelBefore);
  await page.keyboard.press("Escape"); await page.mouse.up();
  await waitState(s => !s.gesture && s.values.intensity[0] === cancelBefore);
  assert.equal(traces.slice(cancelStart).filter(m => m.op === "cancel").length, 1);
  assert.equal(traces.slice(cancelStart).filter(m => m.op === "commit").length, 0);
  results.push("Escape cancels an active drag and restores the backend value");
  const staleBefore = await state();
  await call({op: "host", action: "lock", locked: true});
  await call({op: "host", action: "lock", locked: false}); // Host changes revision without notifying the page.
  await change("亮度", 3);
  await page.waitForFunction(() => document.getElementById("status").textContent.includes("stale editor revision"));
  assert.equal((await state()).values.intensity[0], staleBefore.values.intensity[0]);
  await change("亮度", 3); await waitState(s => s.values.intensity[0] === 3);
  results.push("stale revision refreshes state and requires explicit user retry");
  transportDelay = 190;
  const slowStart = traces.length;
  const slowBounds = await page.getByRole("slider", {name: "亮度滑块", exact: true}).boundingBox();
  await page.mouse.move(slowBounds.x + slowBounds.width*.2, slowBounds.y + slowBounds.height/2); await page.mouse.down();
  for (let n = 0; n < 20; n++) {
    await page.mouse.move(slowBounds.x + slowBounds.width*(.25+n*.025), slowBounds.y + slowBounds.height/2);
    await new Promise(resolve => setTimeout(resolve, 20));
  }
  const lastSlider = await page.getByRole("slider", {name: "亮度滑块", exact: true}).inputValue();
  await page.mouse.up(); await waitState(s => !s.gesture && Math.abs(s.values.intensity[0]-Number(lastSlider))<1e-5);
  const slowSets = traces.slice(slowStart).filter(m => m.op === "set").length;
  assert.ok(slowSets <= 4, `slow host accumulated ${slowSets} slider updates`);
  transportDelay = 0;
  results.push("slow host drag coalesces intermediate updates and commits the final value");
  failNextSet = true;
  const failureBefore = (await state()).values.intensity[0];
  const failBounds = await page.getByRole("slider", {name: "亮度滑块", exact: true}).boundingBox();
  await page.mouse.move(failBounds.x + failBounds.width*.3, failBounds.y + failBounds.height/2); await page.mouse.down();
  await page.mouse.move(failBounds.x + failBounds.width*.8, failBounds.y + failBounds.height/2); await page.mouse.up();
  await page.waitForFunction(() => document.getElementById("status").textContent.includes("宿主拒绝修改"));
  await waitState(s => !s.gesture && s.values.intensity[0] === failureBefore);
  results.push("failed final slider update cancels the backend transaction");

  await open("energy"); await checkLayout("particles-emitter"); await tab("外观"); await checkLayout("particles-appearance");
  await page.setViewportSize({width: 375, height: 900});
  await change("出生颜色 R", .45); await change("出生颜色 G", .65);
  await waitState(s => Math.abs(s.values.color[0]-.45)<1e-5 && Math.abs(s.values.color[1]-.65)<1e-5);
  await tab("发射"); const beforePreset = await state();
  await page.getByRole("button", {name: "密集", exact: true}).click();
  await waitState(s => !s.gesture && s.values.rate[0] === beforePreset.params.rate.default[0]*1.75);
  const restored = await call({op: "host", action: "undo"});
  assert.deepEqual(restored.values, beforePreset.values); await page.evaluate(s => window.motionStudioUpdate(s), restored);
  await change("出生速率", 10000);
  await page.waitForFunction(() => document.getElementById("status").textContent.includes("20,000"));
  assert.equal((await state()).values.rate[0], restored.values.rate[0]);
  results.push("RGBA edits preserve channels; preset is one undo; particle capacity is explicit");
  await tab("变换"); await change("发射器位置 X", 350); await change("发射器位置 Y", 190);
  await waitState(s => s.transform_values.position[0] === 350 && s.transform_values.position[1] === 190);
  const locked = await call({op: "host", action: "lock", locked: true});
  await page.evaluate(s => window.motionStudioUpdate(s), locked);
  assert.equal(await page.getByRole("spinbutton", {name: "发射器位置 X", exact: true}).isDisabled(), true);
  await page.evaluate(() => window.motionStudioDisconnect());
  assert.equal(await page.getByRole("button", {name: "刷新", exact: true}).isDisabled(), true);
  await open("snow");
  const oldFrame = await state(); const seek = await call({op: "host", action: "seek", frame: 90});
  await page.evaluate(s => window.motionStudioUpdate(s), seek);
  await page.waitForFunction(() => document.getElementById("frame").textContent === "帧 90");
  assert.notEqual(seek.frame, oldFrame.frame);
  results.push("sampled transform edits; locked state; disconnect/reconnect; host seek");
  await page.waitForFunction(() => window.__editorTestPending === 0);
  assert.deepEqual(errors, [], "browser errors"); assert.deepEqual(violations, [], "CSP violations");
  await writeFile(path.join(output, "report.json"), JSON.stringify({results, pageErrors: errors, cspViolations: violations,
    previewRequests: traces.filter(m => m.op === "preview").length, sceneVersion: "1.1.0", sceneHash: (await state()).plugin.hash,
    browser: await browser.version(), environment: "Desktop Chromium + actual Rust commands/wgpu; Android WebView container not exercised"}, null, 2));
  console.log(JSON.stringify({passed: results.length, results, output}, null, 2));
} finally {
  await browser?.close(); server.close(); probe.stdin.end();
  await writeFile(path.join(output, "probe-stderr.log"), stderr);
  await writeFile(path.join(output, "messages.json"), JSON.stringify(traces, null, 2));
}
