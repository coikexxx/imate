import { describe, expect, it } from "vitest";
import { applyDelta, createProjectStore } from "./project";

const message = (id: number, content: string, senderType = "assistant") => ({
  id, projectId: "甲", senderType, senderAgentId: null, kind: "chat", content, round: null, createdAt: id,
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
    expect(removed).toBe(4);
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
});
