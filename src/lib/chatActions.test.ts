import { Children, isValidElement, type ReactElement, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "../api";
import { ChatPanel } from "../components/ChatPanel";
import { Button } from "../components/ui";
import { projectStore } from "../stores/project";
import type { Agent, Instruction, ProjectRuntime, SendMessageResponse } from "../types";

const hooks = vi.hoisted(() => ({ index: 0, values: new Map<number, unknown>() }));

// 直接检查实际组件的按钮与处理器；不依赖浏览器布局，也不启动 Tauri。
vi.mock("react", async (importOriginal) => ({
  ...await importOriginal<typeof import("react")>(),
  useEffect: () => undefined,
  useId: () => "chat-test",
  useRef: (initial: unknown) => ({ current: initial }),
  useState: (initial: unknown) => {
    const index = hooks.index++;
    return [hooks.values.has(index) ? hooks.values.get(index) : initial, vi.fn()];
  },
}));
vi.mock("zustand", () => ({ useStore: (store: { getState: () => unknown }) => store.getState() }));
vi.mock("../api", () => ({ api: {
  saveProjectElevation: vi.fn(), sendMessage: vi.fn(), getProject: vi.fn(),
  listInstructions: vi.fn(), listTasks: vi.fn(), listMessages: vi.fn(), stopProject: vi.fn(),
} }));

const agent: Agent = {
  id: "leader", projectId: "project", name: "组长", role: "组长", persona: "", personaDirty: false,
  cli: "codex", model: null, reasoningEffort: null, isLeader: true, sessionId: null,
  lastSeenMessageId: null, seatIndex: 0, createdAt: 1,
};
const runtime: ProjectRuntime = { active: {
  projectId: "project", runId: "running", agentId: agent.id, status: "thinking", seq: 1, partialText: "",
}, cleanupRequired: [] };
const instruction: Instruction = {
  id: "team", projectId: "project", sourceMessageId: 1, kind: "team", targetAgentId: null,
  phase: "discuss", status: "running", discussRounds: 1, turnCount: 2, currentBatch: null,
  resumePhase: null, contextSnapshotJson: null, waitReason: null, createdAt: 1, updatedAt: 1,
  finishedAt: null, elevatedAllowed: false,
};
const response: SendMessageResponse = {
  runId: "running", agentMessageId: 0,
  userMessage: { id: 1, projectId: "project", senderType: "user", senderAgentId: null,
    kind: "chat", content: "补充要求", round: null, createdAt: 1, modelInfo: null },
};

type Element = ReactElement<Record<string, unknown>>;
function find(root: ReactNode, predicate: (element: Element) => boolean): Element {
  for (const child of Children.toArray(root)) {
    if (!isValidElement<Record<string, unknown>>(child)) continue;
    if (predicate(child)) return child;
    try { return find(child.props.children as ReactNode, predicate); } catch { /* 接着查找兄弟节点。 */ }
  }
  throw new Error("没有找到目标控件");
}
function render(options: { content?: string; ready?: boolean; folderExists?: boolean; stopping?: boolean; acting?: boolean; kind?: string } = {}) {
  hooks.index = 0;
  hooks.values = new Map<number, unknown>([[0, options.content ?? "补充要求"], [4, options.ready ?? true],
    [3, options.stopping ?? false], [9, options.acting ?? false], [6, { ...instruction, kind: options.kind ?? "team" }]]);
  const tree = ChatPanel({ projectId: "project", agents: [agent], folderExists: options.folderExists ?? true });
  return {
    send: find(tree, (element) => element.type === Button && element.props.variant === "primary"),
    stop: find(tree, (element) => element.type === Button && element.props.variant === "danger"),
    input: find(tree, (element) => element.props["aria-label"] === "消息内容"),
  };
}
const click = (element: Element) => (element.props.onClick as () => Promise<void>)();

beforeEach(() => {
  vi.clearAllMocks();
  projectStore.getState().beginHydrate("project");
  projectStore.getState().hydrate("project", [], runtime);
  vi.mocked(api.saveProjectElevation).mockResolvedValue(false);
  vi.mocked(api.sendMessage).mockResolvedValue(response);
  vi.mocked(api.getProject).mockResolvedValue({ project: { id: "project", name: "测试", description: "", folderPath: "/tmp/project",
    status: "discussing", createdAt: 1, updatedAt: 1 }, agents: [agent], folderExists: true });
  vi.mocked(api.listInstructions).mockResolvedValue([]);
  vi.mocked(api.listMessages).mockResolvedValue([]);
  vi.mocked(api.stopProject).mockResolvedValue(undefined);
});

describe("实际群聊组件的运行中干预", () => {
  it.each(["补充要求", "@组长 核对已经写入的文件"])("活动回合允许发送并入队：%s", async (content) => {
    const controls = render({ content });
    expect(controls.send.props.disabled).toBe(false);
    expect(controls.stop.props.disabled).toBe(false);
    expect(controls.input.props.disabled).toBe(false);
    await click(controls.send);
    expect(api.sendMessage).toHaveBeenCalledExactlyOnceWith("project", content);
    expect(projectStore.getState().activeRunId).toBe("running");
    expect(projectStore.getState().messages.map((message) => message.id)).toEqual([1]);
  });

  it("独立点名回合继续保持串行", async () => {
    const controls = render({ kind: "direct" });
    expect(controls.send.props.disabled).toBe(true);
    expect(controls.stop.props.disabled).toBe(false);
    await click(controls.send);
    expect(api.sendMessage).not.toHaveBeenCalled();
  });

  it.each(["未就绪", "目录不存在", "清理待确认", "项目不匹配", "正在提交", "正在停止", "正在恢复", "空消息"])("继续拦截不能提交的情况：%s", async (reason) => {
    if (reason === "清理待确认") projectStore.setState({ cleanupRequired: [{ runId: "old", projectId: "project", folderPath: "/tmp/project" }] });
    if (reason === "项目不匹配") projectStore.setState({ projectId: "other" });
    if (reason === "正在提交") projectStore.getState().setSending(true);
    const controls = render({ ready: reason !== "未就绪", folderExists: reason !== "目录不存在",
      stopping: reason === "正在停止", acting: reason === "正在恢复", content: reason === "空消息" ? " " : "补充要求" });
    expect(controls.send.props.disabled).toBe(true);
    await click(controls.send);
    expect(api.sendMessage).not.toHaveBeenCalled();
    expect(api.saveProjectElevation).not.toHaveBeenCalled();
  });

  it("同一次提交在重渲染前或旧回合结束时都不会被重复发送", async () => {
    let complete!: (value: SendMessageResponse) => void;
    vi.mocked(api.sendMessage).mockImplementation(() => new Promise((resolve) => { complete = resolve; }));
    const controls = render();
    const first = click(controls.send);
    const repeated = click(controls.send);
    await Promise.resolve();
    await Promise.resolve();
    expect(api.sendMessage).toHaveBeenCalledTimes(1);
    projectStore.getState().applyEvent("run:finished", { projectId: "project", runId: "running", agentId: agent.id,
      seq: 2, status: "ok", error: null });
    const afterFinish = click(controls.send);
    await Promise.resolve();
    expect(api.sendMessage).toHaveBeenCalledTimes(1);
    complete(response);
    await Promise.all([first, repeated, afterFinish]);
  });

  it("运行中仍可停止项目", async () => {
    const controls = render();
    expect(controls.stop.props.disabled).toBe(false);
    await click(controls.stop);
    expect(api.stopProject).toHaveBeenCalledExactlyOnceWith("project");
  });
});
