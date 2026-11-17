import { describe, expect, it } from "vitest";
import { applyDelta, createProjectStore } from "./project";

const message = (id: number, content: string, senderType = "assistant") => ({
  id, projectId: "甲", senderType, senderAgentId: null, kind: "chat", content, round: null, createdAt: id, modelInfo: null,
});

describe("运行输出归并", () => {
  it("去重并隔离项目和运行", () => {
    const initial = { projectId: "甲", runId: "一", seq: 0, text: "" };
    const first = applyDelta(initial, { ...initial, seq: 1, text: "第一段" });
    expect(first.text).toBe("第一段");
    expect(applyDelta(first, { ...initial, seq: 1, text: "不能重复" })).toEqual(first);
    expect(applyDelta(first, { projectId: "乙", runId: "一", seq: 2, text: "不能混入" })).toEqual(first);
    expect(applyDelta(first, { projectId: "甲", runId: "二", seq: 2, text: "不能混入" })).toEqual(first);
    expect(applyDelta(first, { ...initial, seq: 2, text: "第二段" }).text).toBe("第一段第二段");
  });
  it("最终数据库消息替换临时回答，终态后忽略迟到增量", () => {
    const store = createProjectStore(); const state = store.getState();
    state.beginHydrate("甲");
    state.registerRun("甲", "run-1", "agent-1", -1);
    state.applyEvent("message:delta", { projectId: "甲", runId: "run-1", agentId: "agent-1", seq: 1, text: "临时" });
    state.applyEvent("message:new", { projectId: "甲", runId: "run-1", message: message(-1, "最终正文") });
    expect(store.getState().messages).toHaveLength(1);
    expect(store.getState().messages[0].content).toBe("最终正文");
    state.applyEvent("run:finished", { projectId: "甲", runId: "run-1", agentId: "agent-1", seq: 2, status: "ok", error: null });
    state.applyEvent("message:delta", { projectId: "甲", runId: "run-1", agentId: "agent-1", seq: 3, text: "迟到" });
    expect(store.getState().streams["run-1"].text).toBe("最终正文");
  });
  it("首订阅期间先缓存事件，快照后按序续接且历史重叠按 id 去重", () => {
    const store = createProjectStore(); const state = store.getState();
    state.beginHydrate("甲");
    state.applyEvent("message:delta", { projectId: "甲", runId: "r", agentId: "a", seq: 2, text: "二" });
    state.applyEvent("message:delta", { projectId: "甲", runId: "r", agentId: "a", seq: 1, text: "一" });
    state.hydrate("甲", [message(1, "历史"), message(2, "重叠")], {
      active: { projectId: "甲", runId: "r", agentId: "a", status: "talking", seq: 0, partialText: "" }, cleanupRequired: [],
    });
    expect(store.getState().streams.r.text).toBe("一二");
    state.hydrate("甲", [message(2, "重叠"), message(3, "新历史")], { active: null, cleanupRequired: [] });
    expect(store.getState().messages.map((m) => m.id)).toEqual([1, 2, 3]);
  });
  it("监听在卸载后返回时立即清理，避免 StrictMode 残留订阅", async () => {
    const { attachProjectEvents } = await import("./project");
    let removed = 0;
    const subscription = attachProjectEvents(async () => () => { removed += 1; }, () => {});
    subscription.dispose();
    await subscription.ready;
    expect(removed).toBe(6);
  });
  it("加载期间结束的回合不会被较旧的运行快照复活", () => {
    const store = createProjectStore(); const state = store.getState();
    state.beginHydrate("甲");
    state.applyEvent("run:finished", { projectId: "甲", runId: "r", agentId: "a", seq: 2, status: "ok", error: null });
    state.hydrate("甲", [message(1, "")], {
      active: { projectId: "甲", runId: "r", agentId: "a", status: "working", seq: 1, partialText: "已输出" }, cleanupRequired: [],
    });
    expect(store.getState().activeRunId).toBeNull();
    expect(store.getState().agentStatuses.a).toBe("idle");
  });
  it("成员状态取终态并按项目隔离", () => {
    const store = createProjectStore(); const state = store.getState();
    state.beginHydrate("甲");
    state.hydrate("甲", [], { active: null, cleanupRequired: [] });
    state.applyEvent("agent:status", { projectId: "甲", agentId: "a", runId: "r", status: "working" });
    state.applyEvent("agent:status", { projectId: "乙", agentId: "a", runId: "x", status: "error" });
    expect(store.getState().agentStatuses.a).toBe("working");
  });
  it.each([false, true])("旧会话重开后，正文仍绑定同一条消息（发送响应迟到：%s）", (lateResponse) => {
    const store = createProjectStore(); const state = store.getState();
    state.beginHydrate("甲");
    state.hydrate("甲", [message(7, "")], { active: null, cleanupRequired: [] });
    if (!lateResponse) state.registerRun("甲", "旧回合", "agent-1", 7);
    state.applyEvent("run:finished", { projectId: "甲", runId: "旧回合", agentId: "agent-1", seq: 1,
      status: "failed", error: null, replacement: { runId: "新回合", messageId: 7 } });
    if (lateResponse) state.registerRun("甲", "旧回合", "agent-1", 7);
    expect(store.getState().liveRuns).toEqual(["新回合"]);
    expect(store.getState().activeRunId).toBe("新回合");
    expect(store.getState().agentStatuses["agent-1"]).toBe("thinking");
    expect(store.getState().error).toBeNull();
    state.applyEvent("message:delta", { projectId: "甲", runId: "旧回合", agentId: "agent-1", seq: 2, text: "旧正文" });
    state.applyEvent("message:delta", { projectId: "甲", runId: "新回合", agentId: "agent-1", seq: 1, text: "新正文" });
    expect(Object.values(store.getState().streams).filter((stream) => stream.messageId === 7))
      .toEqual([expect.objectContaining({ runId: "新回合", text: "新正文" })]);
    state.applyEvent("message:new", { projectId: "甲", runId: "新回合", message: message(7, "完整回复") });
    state.applyEvent("run:finished", { projectId: "甲", runId: "新回合", agentId: "agent-1", seq: 2, status: "ok", error: null });
    expect(store.getState().messages).toEqual([message(7, "完整回复")]);
    expect(store.getState().streams["新回合"].final).toBe(true);
    expect(store.getState().activeRunId).toBeNull();
  });
  it("并行回合里一个结束时，其余还在运行就不清空运行状态", () => {
    const store = createProjectStore(); const state = store.getState();
    state.beginHydrate("甲");
    state.hydrate("甲", [], { active: null, cleanupRequired: [] });
    state.applyEvent("agent:status", { projectId: "甲", agentId: "a", runId: "r1", status: "thinking" });
    state.applyEvent("agent:status", { projectId: "甲", agentId: "b", runId: "r2", status: "thinking" });
    state.applyEvent("agent:status", { projectId: "甲", agentId: "a", runId: "r1", status: "talking" });
    expect(store.getState().liveRuns).toEqual(["r2", "r1"]);
    state.applyEvent("run:finished", { projectId: "甲", runId: "r1", agentId: "a", seq: 1, status: "ok", error: null });
    expect(store.getState().activeRunId).toBe("r2");
    expect(store.getState().agentStatuses).toMatchObject({ a: "idle", b: "thinking" });
    state.applyEvent("run:finished", { projectId: "甲", runId: "r2", agentId: "b", seq: 1, status: "failed", error: "超时" });
    expect(store.getState().activeRunId).toBeNull();
    expect(store.getState().liveRuns).toEqual([]);
    expect(store.getState().agentStatuses.b).toBe("error");
  });
  it("没有先收到状态事件的回合结束时，不影响别的回合", () => {
    const store = createProjectStore(); const state = store.getState();
    state.beginHydrate("甲");
    state.hydrate("甲", [], { active: { projectId: "甲", runId: "r1", agentId: "a", status: "working", seq: 0, partialText: "" }, cleanupRequired: [] });
    expect(store.getState().liveRuns).toEqual(["r1"]);
    state.applyEvent("run:finished", { projectId: "甲", runId: "别的回合", agentId: "b", seq: 1, status: "failed", error: null });
    expect(store.getState().activeRunId).toBe("r1");
  });
  it("回合终态单独记录，不混入接口错误；自动重开成功后不留失败提醒；新回合开始即清除", () => {
    const store = createProjectStore(); const state = store.getState();
    state.beginHydrate("甲");
    state.hydrate("甲", [], { active: null, cleanupRequired: [] });
    state.applyEvent("run:finished", { projectId: "甲", runId: "r1", agentId: "a", seq: 1, status: "failed", error: "CLI 不可用：未找到已安装的 claude" });
    expect(store.getState().lastRun).toEqual({ runId: "r1", agentId: "a", status: "failed", error: "CLI 不可用：未找到已安装的 claude" });
    expect(store.getState().error).toBeNull();
    state.setError("接口错误");
    expect(store.getState().lastRun?.error).toBe("CLI 不可用：未找到已安装的 claude");
    state.applyEvent("message:delta", { projectId: "甲", runId: "r2", agentId: "a", seq: 1, text: "新回合" });
    expect(store.getState().lastRun).toBeNull();
    state.applyEvent("run:finished", { projectId: "甲", runId: "r2", agentId: "a", seq: 1, status: "failed", error: null,
      replacement: { runId: "r3", messageId: 1 } });
    expect(store.getState().lastRun).toBeNull();
    state.applyEvent("run:finished", { projectId: "甲", runId: "r3", agentId: "a", seq: 1, status: "ok", error: null });
    expect(store.getState().lastRun).toBeNull();
    state.applyEvent("run:finished", { projectId: "甲", runId: "r4", agentId: "b", seq: 1, status: "timeout", error: "回合超过总时长，已停止" });
    expect(store.getState().lastRun).toMatchObject({ status: "timeout" });
    state.beginHydrate("乙");
    expect(store.getState().lastRun).toBeNull();
  });
  it("文件变化以后端给出的全集为准，并推动文件树刷新", () => {
    const store = createProjectStore(); const state = store.getState();
    state.beginHydrate("甲");
    state.hydrate("甲", [], { active: null, cleanupRequired: [] });
    state.applyEvent("files:changed", { projectId: "甲", paths: ["a.txt"], changed: ["a.txt"] });
    state.applyEvent("files:changed", { projectId: "乙", paths: ["x.txt"], changed: ["x.txt"] });
    expect(store.getState().changedFiles).toEqual(["a.txt"]);
    expect(store.getState().filesTick).toBe(1);
    state.applyEvent("files:changed", { projectId: "甲", paths: [], changed: [] });
    expect(store.getState().changedFiles).toEqual([]);
    expect(store.getState().filesTick).toBe(2);
    state.setChangedFiles("乙", ["不该写入"]);
    expect(store.getState().changedFiles).toEqual([]);
  });
  it("规章变化只推动规章面板刷新，不影响运行状态", () => {
    const store = createProjectStore(); const state = store.getState();
    state.beginHydrate("甲");
    state.hydrate("甲", [], { active: null, cleanupRequired: [] });
    state.applyEvent("agent:status", { projectId: "甲", agentId: "a", runId: "r", status: "working" });
    state.applyEvent("rules:changed", { projectId: "甲", version: "abc" });
    expect(store.getState().rulesTick).toBe(1);
    expect(store.getState().activeRunId).toBe("r");
    expect(store.getState().agentStatuses.a).toBe("working");
  });
});
