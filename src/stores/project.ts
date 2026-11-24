import { createStore } from "zustand/vanilla";
import type { AgentStatus, Message, ProjectEventMap, ProjectRuntime, RunFinished, TaskItem } from "../types";

export interface StreamText { projectId: string; runId: string; seq: number; text: string }
export interface TextDelta { projectId: string; runId: string; seq: number; text: string }
export function applyDelta(current: StreamText, delta: TextDelta): StreamText {
  if (delta.projectId !== current.projectId || delta.runId !== current.runId || delta.seq <= current.seq) return current;
  return { ...current, seq: delta.seq, text: current.text + delta.text };
}
interface Stream extends StreamText { agentId: string; messageId: number | null; finished: boolean; final: boolean; superseded?: boolean }
type Buffered = { name: keyof ProjectEventMap; payload: ProjectEventMap[keyof ProjectEventMap] };
/** 最近一个没有成功结束的回合。成功、自动重开或新回合开始时清空；与接口错误 `error` 分开。 */
export interface LastRun { runId: string; agentId: string; status: RunFinished["status"]; error: string | null }
interface ProjectState {
  projectId: string;
  messages: Message[];
  streams: Record<string, Stream>;
  agentStatuses: Record<string, AgentStatus>;
  activeRunId: string | null;
  /** 正在运行的回合，按开始顺序。并行讨论时不止一个。 */
  liveRuns: string[];
  changedFiles: string[];
  filesTick: number;
  rulesTick: number;
  /** 当前指令的任务清单；轨迹面板与指令栏共用，由群聊在三次拉取后写入。 */
  tasks: TaskItem[];
  setChangedFiles: (projectId: string, paths: string[]) => void;
  setTasks: (projectId: string, tasks: TaskItem[]) => void;
  cleanupRequired: ProjectRuntime["cleanupRequired"];
  lastRun: LastRun | null;
  /** 接口调用失败的说明；回合终态不写这里。 */
  error: string | null;
  sending: boolean;
  hydrating: boolean;
  buffered: Buffered[];
  beginHydrate: (projectId: string) => void;
  hydrate: (projectId: string, history: Message[], runtime: ProjectRuntime) => void;
  registerRun: (projectId: string, runId: string, agentId: string, messageId: number) => void;
  applyEvent: <K extends keyof ProjectEventMap>(name: K, payload: ProjectEventMap[K]) => void;
  mergeHistory: (projectId: string, messages: Message[]) => void;
  setError: (error: string | null) => void;
  setSending: (sending: boolean) => void;
}
function upsert(messages: Message[], incoming: Message[]): Message[] {
  const map = new Map(messages.map((m) => [m.id, m]));
  for (const message of incoming) map.set(message.id, message);
  return [...map.values()].sort((a, b) => a.id - b.id);
}
function streamFor(projectId: string, runId: string, agentId: string): Stream {
  return { projectId, runId, seq: 0, text: "", agentId, messageId: null, finished: false, final: false };
}
function withRun(live: string[], runId: string): string[] {
  return [...live.filter((id) => id !== runId), runId];
}
function latestRun(live: string[]): string | null {
  return live.length > 0 ? live[live.length - 1] : null;
}
export function createProjectStore() {
  return createStore<ProjectState>((set, get) => ({
    projectId: "", messages: [], streams: {}, agentStatuses: {}, activeRunId: null, liveRuns: [],
    cleanupRequired: [], lastRun: null, error: null, sending: false, hydrating: false, buffered: [],
    changedFiles: [], filesTick: 0, rulesTick: 0, tasks: [],
    beginHydrate: (projectId) => set({ projectId, messages: [], streams: {}, agentStatuses: {}, activeRunId: null, liveRuns: [],
      cleanupRequired: [], lastRun: null, error: null, sending: false, hydrating: true, buffered: [],
      changedFiles: [], filesTick: 0, rulesTick: 0, tasks: [] }),
    mergeHistory: (projectId, messages) => {
      if (projectId !== get().projectId) return;
      set((state) => ({ messages: upsert(messages, state.messages) }));
    },
    hydrate: (projectId, history, runtime) => {
      if (projectId !== get().projectId) return;
      const buffered = get().buffered;
      set((state) => {
        const streams = { ...state.streams };
        const agentStatuses = { ...state.agentStatuses };
        if (runtime.active) {
          const active = runtime.active;
          const previous = streams[active.runId];
          const messageId = previous?.messageId ?? history.filter((m) => m.senderAgentId === active.agentId).slice(-1)[0]?.id ?? null;
          streams[active.runId] = { ...streamFor(projectId, active.runId, active.agentId), text: active.partialText,
            seq: active.seq, messageId, finished: previous?.finished ?? false, final: previous?.final ?? false };
          agentStatuses[active.agentId] = active.status;
        }
        return { messages: upsert(state.messages, history), streams, agentStatuses,
          activeRunId: runtime.active?.runId ?? null, liveRuns: runtime.active ? [runtime.active.runId] : [], cleanupRequired: runtime.cleanupRequired, hydrating: false, buffered: [] };
      });
      const deltas = buffered.filter((event) => event.name === "message:delta")
        .sort((a, b) => (a.payload as ProjectEventMap["message:delta"]).seq - (b.payload as ProjectEventMap["message:delta"]).seq);
      for (const event of [...deltas, ...buffered.filter((event) => event.name !== "message:delta")]) get().applyEvent(event.name, event.payload);
    },
    registerRun: (projectId, runId, agentId, messageId) => {
      if (projectId !== get().projectId) return;
      set((state) => {
        const stream = state.streams[runId] ?? streamFor(projectId, runId, agentId);
        if (stream.superseded) return {};
        const liveRuns = stream.finished ? state.liveRuns : withRun(state.liveRuns, runId);
        return { streams: { ...state.streams, [runId]: { ...stream, messageId } },
          liveRuns, activeRunId: stream.finished ? state.activeRunId : runId, hydrating: false };
      });
    },
    applyEvent: (name, payload) => {
      if (payload.projectId !== get().projectId) return;
      if (get().hydrating) {
        set((state) => ({ buffered: [...state.buffered, { name, payload }] }));
        return;
      }
      set((state) => {
        const streams = { ...state.streams };
        const agentStatuses = { ...state.agentStatuses };
        if (name === "message:delta") {
          const delta = payload as ProjectEventMap["message:delta"];
          const previous = streams[delta.runId] ?? streamFor(delta.projectId, delta.runId, delta.agentId);
          if (previous.finished || previous.final) return {};
          streams[delta.runId] = { ...previous, ...applyDelta(previous, delta) };
          agentStatuses[delta.agentId] = "talking";
          return { streams, agentStatuses, liveRuns: withRun(state.liveRuns, delta.runId), activeRunId: delta.runId, lastRun: null };
        }
        if (name === "message:new") {
          const event = payload as ProjectEventMap["message:new"];
          if (event.runId && event.message.senderType === "assistant") {
            const previous = streams[event.runId] ?? streamFor(event.projectId, event.runId, event.message.senderAgentId ?? "");
            streams[event.runId] = { ...previous, messageId: event.message.id, text: event.message.content, final: true };
          }
          return { messages: upsert(state.messages, [event.message]), streams };
        }
        if (name === "agent:status") {
          const event = payload as ProjectEventMap["agent:status"];
          if (event.runId && streams[event.runId]?.finished) return {};
          agentStatuses[event.agentId] = event.status;
          const active = ["thinking", "talking", "working"].includes(event.status);
          if (!active || !event.runId) return { agentStatuses };
          return { agentStatuses, liveRuns: withRun(state.liveRuns, event.runId), activeRunId: event.runId, lastRun: null };
        }
        if (name === "files:changed") {
          const event = payload as ProjectEventMap["files:changed"];
          return { changedFiles: event.changed, filesTick: state.filesTick + 1 };
        }
        if (name === "rules:changed") return { rulesTick: state.rulesTick + 1 };
        const event = payload as ProjectEventMap["run:finished"];
        const previous = streams[event.runId] ?? streamFor(event.projectId, event.runId, event.agentId);
        if (previous.finished) return {};
        const finished = { ...previous, seq: Math.max(previous.seq, event.seq), finished: true };
        const remaining = state.liveRuns.filter((id) => id !== event.runId);
        if (event.replacement) {
          const { runId, messageId } = event.replacement;
          const next = streams[runId] ?? streamFor(event.projectId, runId, event.agentId);
          streams[event.runId] = { ...finished, messageId: null, superseded: true };
          streams[runId] = { ...next, messageId };
          const liveRuns = next.finished ? remaining : withRun(remaining, runId);
          if (!next.finished) agentStatuses[event.agentId] = next.text ? "talking" : "thinking";
          // 自动重开：失败的旧回合不留提醒，由新回合的终态决定。
          return { streams, agentStatuses, liveRuns, activeRunId: latestRun(liveRuns), lastRun: null };
        }
        streams[event.runId] = finished;
        agentStatuses[event.agentId] = event.status === "ok" || event.status === "cancelled" ? "idle" : "error";
        const lastRun: LastRun | null = event.status === "ok" ? null
          : { runId: event.runId, agentId: event.agentId, status: event.status, error: event.error };
        return { streams, agentStatuses, liveRuns: remaining,
          activeRunId: state.activeRunId === event.runId ? latestRun(remaining) : state.activeRunId,
          lastRun, sending: false };
      });
    },
    setError: (error) => set({ error }),
    setSending: (sending) => set({ sending }),
    setChangedFiles: (projectId, paths) => { if (projectId === get().projectId) set({ changedFiles: paths }); },
    setTasks: (projectId, tasks) => { if (projectId === get().projectId) set({ tasks }); },
  }));
}
export const projectStore = createProjectStore();

/** 即使异步订阅完成前卸载，也立即释放随后返回的监听；兼容 StrictMode。 */
export function attachProjectEvents(
  subscribe: (name: keyof ProjectEventMap, receive: (payload: ProjectEventMap[keyof ProjectEventMap]) => void) => Promise<() => void>,
  receive: (name: keyof ProjectEventMap, payload: ProjectEventMap[keyof ProjectEventMap]) => void,
): { ready: Promise<void>; dispose: () => void } {
  let disposed = false; const unlisteners: (() => void)[] = [];
  const names: (keyof ProjectEventMap)[] = ["message:delta", "message:new", "agent:status", "run:finished", "files:changed", "rules:changed"];
  const ready = Promise.all(names.map(async (name) => {
    const unlisten = await subscribe(name, (payload) => { if (!disposed) receive(name, payload); });
    if (disposed) unlisten(); else unlisteners.push(unlisten);
  })).then(() => {});
  return { ready, dispose: () => { disposed = true; for (const unlisten of unlisteners.splice(0)) unlisten(); } };
}
