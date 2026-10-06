/* Only the documented, scoped plugin protocol is exposed to the page. */
export class EditorBridge {
  constructor(changed) {
    this.changed = changed; this.pending = new Map(); this.sequence = 0;
    this.epoch = 0; this.tail = Promise.resolve(); this.state = null; this.token = null;
  }
  connect(connection) {
    this.disconnect();
    if (!connection || typeof connection.token !== "string" || !connection.definition || !connection.state)
      throw new Error("宿主连接信息不完整");
    this.token = connection.token; this.definition = connection.definition;
    this.state = connection.state; this.changed(this.state);
  }
  disconnect() {
    this.epoch++; this.token = null; this.state = null;
    for (const task of this.pending.values()) { clearTimeout(task.timeout); task.reject(new Error("编辑器连接已关闭")); }
    this.pending.clear(); this.tail = Promise.resolve();
  }
  reply(reply) {
    if (typeof reply === "string") { try { reply = JSON.parse(reply); } catch { return; } }
    if (!reply || reply.token !== this.token) return;
    const task = this.pending.get(String(reply.id)); if (!task) return;
    this.pending.delete(String(reply.id)); clearTimeout(task.timeout);
    if (reply.ok) task.resolve(reply.result); else task.reject(new Error(reply.error || "修改失败"));
  }
  request(message) {
    if (!this.token || !window.MotionStudioHost) return Promise.reject(new Error("宿主未连接"));
    if (this.pending.size >= 4) return Promise.reject(new Error("等待宿主完成当前请求"));
    const id = String(++this.sequence), token = this.token;
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => { this.pending.delete(id); reject(new Error("宿主请求超时，请刷新状态")); }, 10000);
      this.pending.set(id, {resolve, reject, timeout});
      try { window.MotionStudioHost.postMessage(JSON.stringify({protocol: 1, token, id, message})); }
      catch (error) { clearTimeout(timeout); this.pending.delete(id); reject(error); }
    });
  }
  accept(state) {
    if (!this.token) return;
    if (!state || !Number.isSafeInteger(state.revision)) throw new Error("宿主返回了无效状态");
    if (this.state && state.revision < this.state.revision) return;
    this.state = state; this.changed(state);
  }
  enqueue(work) {
    const epoch = this.epoch;
    const result = this.tail.then(() => {
      if (!this.token || epoch !== this.epoch) throw new Error("编辑器连接已关闭");
      return work(epoch);
    });
    this.tail = result.catch(() => {}); return result;
  }
  async send(build, epoch) {
    if (!this.state || epoch !== this.epoch) throw new Error("编辑器连接已关闭");
    const message = build(this.state); if (message === null) return;
    const state = await this.request({...message, revision: this.state.revision});
    if (epoch !== this.epoch) throw new Error("编辑器连接已关闭");
    this.accept(state);
  }
  async recover(epoch) {
    if (epoch !== this.epoch || !this.token) return;
    try { this.accept(await this.request({op: "state"})); } catch { /* Preserve the original error. */ }
  }
  edit(build) {
    return this.enqueue(async epoch => {
      try { await this.send(build, epoch); }
      catch (error) { await this.recover(epoch); throw error; }
    });
  }
  transaction(builds) {
    return this.enqueue(async epoch => {
      try {
        await this.send(() => ({op: "begin"}), epoch);
        for (const build of builds) await this.send(build, epoch);
        await this.send(() => ({op: "commit"}), epoch);
      } catch (error) {
        await this.recover(epoch);
        if (epoch === this.epoch && this.state?.gesture) {
          try { await this.send(() => ({op: "cancel"}), epoch); } catch { /* Host close also cancels. */ }
        }
        throw error;
      }
    });
  }
}
