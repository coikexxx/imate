import { confirm } from "@tauri-apps/plugin-dialog";
import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { useStore } from "zustand";
import { api } from "../api";
import { mentionCandidates, parseMention, shouldSendOnEnter } from "../lib/mentions";
import { errorHint } from "../lib/errorHint";
import { describeCleanup, describeRunFinished } from "../lib/runErrors";
import { modelNote } from "../lib/modelNote";
import { shirtColor } from "../office/layout";
import { attachProjectEvents, projectStore } from "../stores/project";
import type { Agent, Instruction, ProjectStatus, TaskItem } from "../types";
import { InstructionBar } from "./InstructionBar";
import { Button, inputClass } from "./ui";
import { Switch } from "./Switch";

/** 悬停或键盘聚焦时在上方弹出的说明气泡。 */
function Tips({ label, children }: { label: string; children: ReactNode }) {
  const id = useId();
  return <span className="group relative inline-flex">
    <button type="button" aria-label={label} aria-describedby={id}
      className="inline-flex h-4 w-4 items-center justify-center rounded-full text-slate-400 hover:text-slate-600 focus-visible:text-slate-600 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-emerald-700">
      <Icon name="info" width="14" height="14" />
    </button>
    <span id={id} role="tooltip"
      className="pointer-events-none invisible absolute bottom-full left-1/2 z-20 mb-2 w-72 -translate-x-1/2 rounded-lg border border-slate-200 bg-white px-3 py-2 text-xs leading-5 text-slate-600 opacity-0 shadow-lg transition-opacity group-hover:visible group-hover:opacity-100 group-focus-within:visible group-focus-within:opacity-100">
      {children}
    </span>
  </span>;
}
import { Icon } from "./Icon";

interface Props { projectId: string; agents: Agent[]; folderExists: boolean; mention?: string; onMentionUsed?: () => void; onFinished?: () => void }
export function ChatPanel({ projectId, agents, folderExists, mention, onMentionUsed, onFinished }: Props) {
  const state = useStore(projectStore);
  const [input, setInput] = useState("");
  const [loadingHistory, setLoadingHistory] = useState(false);
  const [hasMore, setHasMore] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [ready, setReady] = useState(false);
  const [projectStatus, setProjectStatus] = useState<ProjectStatus>("idle");
  const [instruction, setInstruction] = useState<Instruction | null>(null);
  const [tasks, setTasks] = useState<TaskItem[]>([]);
  const [supplement, setSupplement] = useState("");
  const [acting, setActing] = useState(false);
  const [allowElevated, setAllowElevated] = useState(false);
  const sendPending = useRef(false);
  const textarea = useRef<HTMLTextAreaElement>(null);
  const tail = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let disposed = false;
    setReady(false);
    projectStore.getState().beginHydrate(projectId);
    const subscription = attachProjectEvents(api.listenProjectEvent, (name, payload) => {
      projectStore.getState().applyEvent(name, payload);
      if (name === "run:finished" && payload.projectId === projectId && !("replacement" in payload && payload.replacement)) {
        onFinished?.();
        Promise.all([api.getProjectRuntime(projectId), api.getProject(projectId), api.listInstructions(projectId)]).then(async ([runtime, detail, found]) => {
          if (disposed || projectStore.getState().projectId !== projectId) return;
          projectStore.setState({ cleanupRequired: runtime.cleanupRequired });
          const current = found[0] ?? null;
          const taskPage = current ? await api.listTasks(current.id) : [];
          if (disposed || projectStore.getState().projectId !== projectId) return;
          setProjectStatus(detail.project.status);
          setInstruction(current);
          setTasks(taskPage);
        }).catch((error) => { if (!disposed) projectStore.getState().setError(String(error)); });
      }
    });
    subscription.ready.then(async () => {
      const [history, runtime, detail, found, elevation] = await Promise.all([
        api.listMessages(projectId), api.getProjectRuntime(projectId), api.getProject(projectId),
        api.listInstructions(projectId), api.getProjectElevation(projectId).catch(() => false),
      ]);
      if (disposed) return;
      const current = found[0] ?? null;
      const taskPage = current ? await api.listTasks(current.id) : [];
      if (disposed) return;
      projectStore.getState().hydrate(projectId, history, runtime); setHasMore(history.length === 50);
      setProjectStatus(detail.project.status);
      setInstruction(current);
      setTasks(taskPage);
      setAllowElevated(elevation);
      setReady(true);
    }).catch((error) => { if (!disposed) { projectStore.getState().setError(String(error)); setReady(false); } });
    return () => { disposed = true; subscription.dispose(); };
  }, [projectId, onFinished]);
  useEffect(() => {
    if (mention) { setInput(`@${mention} `); textarea.current?.focus(); onMentionUsed?.(); }
  }, [mention, onMentionUsed]);
  useEffect(() => { tail.current?.scrollIntoView({ block: "nearest" }); }, [state.messages, state.streams, projectId]);
  const projectMatches = state.projectId === projectId;
  const visibleMessages = projectMatches ? state.messages : [];
  const visibleStreams = projectMatches ? Object.values(state.streams) : [];
  const visibleCleanup = projectMatches ? state.cleanupRequired : [];
  const lastRun = projectMatches ? state.lastRun : null;
  const runNotice = lastRun ? describeRunFinished(lastRun.status, lastRun.error) : null;
  const hint = errorHint(lastRun?.error) ?? errorHint(state.error);
  const busy = projectMatches && (Boolean(state.activeRunId) || state.sending);
  const canQueue = instruction?.projectId === projectId && instruction.kind === "team" && instruction.status === "running";
  function canSend(current: typeof state) {
    return ready && current.projectId === projectId && !sendPending.current && !current.sending && !stopping && !acting
      && folderExists && current.cleanupRequired.length === 0 && Boolean(input.trim())
      && (!current.activeRunId || canQueue);
  }
  const candidates = mentionCandidates(input, agents, projectId);
  async function send() {
    if (!canSend(projectStore.getState())) return;
    const recipient = parseMention(input, agents);
    if ("error" in recipient) { state.setError(recipient.error); return; }
    // 同步锁独立于回合事件，防止重渲染前双击，或旧回合结束时再次提交同一条消息。
    sendPending.current = true;
    const content = input.trim(); state.setSending(true); state.setError(null);
    try {
      // 先落盘开关，保证本次指令创建时快照到的是用户刚选的档位。
      await api.saveProjectElevation(projectId, allowElevated);
      const result = await api.sendMessage(projectId, content);
      if (projectStore.getState().projectId !== projectId) return;
      projectStore.getState().applyEvent("message:new", { projectId, runId: result.runId, message: result.userMessage });
      if (result.agentMessageId > 0) {
        projectStore.getState().mergeHistory(projectId, [{ id: result.agentMessageId, projectId, senderType: "assistant",
          senderAgentId: recipient.agent.id, kind: "chat", content: "", round: null, createdAt: Date.now(), modelInfo: null }]);
        projectStore.getState().registerRun(projectId, result.runId, recipient.agent.id, result.agentMessageId);
      }
      setInput("");
      await refreshTeam();
    } catch (error) { if (projectStore.getState().projectId === projectId) state.setError(String(error)); }
    finally { sendPending.current = false; if (projectStore.getState().projectId === projectId) state.setSending(false); }
  }
  async function stop() {
    setStopping(true);
    try { await api.stopProject(projectId); await refreshTeam(); onFinished?.(); }
    catch (error) { state.setError(String(error)); }
    finally { setStopping(false); }
  }
  async function older() {
    setLoadingHistory(true);
    try {
      const page = await api.listMessages(projectId, visibleMessages[0]?.id ?? null);
      if (projectStore.getState().projectId !== projectId) return;
      state.mergeHistory(projectId, page); setHasMore(page.length === 50);
    } catch (error) { state.setError(String(error)); }
    finally { setLoadingHistory(false); }
  }
  async function refreshTeam() {
    const [detail, found] = await Promise.all([api.getProject(projectId), api.listInstructions(projectId)]);
    if (projectStore.getState().projectId !== projectId) return;
    const current = found[0] ?? null;
    const taskPage = current ? await api.listTasks(current.id) : [];
    if (projectStore.getState().projectId !== projectId) return;
    setProjectStatus(detail.project.status);
    setInstruction(current);
    setTasks(taskPage);
    const history = await api.listMessages(projectId);
    if (projectStore.getState().projectId === projectId) projectStore.getState().mergeHistory(projectId, history);
  }
  async function resume(action: "retry" | "supplement" | "abandon", note?: string) {
    const current = projectStore.getState();
    if (!instruction || instruction.projectId !== projectId || current.projectId !== projectId || acting) return;
    if (action !== "abandon" && current.cleanupRequired.length > 0) return;
    if (action === "supplement" && !note?.trim()) return;
    setActing(true);
    state.setError(null);
    try {
      await api.resumeInstruction(instruction.id, action, note ?? null);
      if (action === "supplement") setSupplement("");
      await refreshTeam();
      onFinished?.();
    } catch (error) { if (projectStore.getState().projectId === projectId) state.setError(String(error)); }
    finally { if (projectStore.getState().projectId === projectId) setActing(false); }
  }
  async function authorizeElevated(taskId: string) {
    const current = projectStore.getState();
    if (acting || current.projectId !== projectId || current.cleanupRequired.length > 0) return;
    setActing(true);
    state.setError(null);
    try {
      await api.retryBlockedTask(taskId);
      await refreshTeam();
      onFinished?.();
    } catch (error) { if (projectStore.getState().projectId === projectId) state.setError(String(error)); }
    finally { if (projectStore.getState().projectId === projectId) setActing(false); }
  }
  async function toggleElevation(next: boolean) {
    setAllowElevated(next);
    try { await api.saveProjectElevation(projectId, next); }
    catch (error) {
      setAllowElevated(!next);
      if (projectStore.getState().projectId === projectId) state.setError(String(error));
    }
  }
  async function cleanup(runId: string) {
    const accepted = await confirm("请先在终端或活动监视器中确认该目录相关的残留 CLI 及子进程已停止，并检查文件修改。确认后解除此运行的目录阻塞。", {
      title: "确认已人工处理残留进程", kind: "warning",
    });
    if (!accepted) return;
    try {
      await api.confirmRuntimeCleanup(runId);
      const runtime = await api.getProjectRuntime(projectId);
      const current = projectStore.getState();
      if (current.projectId === projectId) current.hydrate(projectId, current.messages, runtime);
    } catch (error) { state.setError(String(error)); }
  }
  const sender = (id: string | null, type: string) => type === "user" ? "你" : type === "system" ? "系统" : agents.find((a) => a.id === id)?.name ?? "成员";
  return <section className="studio-chat" aria-label="项目群聊">
    <header className="studio-chat-head">
      <span className="text-sm font-medium">项目群聊</span><span className="text-[11px] text-slate-500">组长协调 · 成员协作</span>
    </header>
    <div className="studio-messages" aria-live="polite">
      {hasMore && <Button disabled={loadingHistory} onClick={older}>{loadingHistory ? "加载中…" : "加载更早消息"}</Button>}
      {!visibleMessages.length && !busy && <div className="studio-empty-chat"><span className="grid size-14 place-items-center rounded-2xl border border-[#d5e1cd] bg-[#eff4e9] text-sky-300"><Icon name="chat" width="26" height="26" /></span><h3>团队已就位</h3><p>告诉组长你想完成什么。团队会讨论、分工，并在这里汇报进展。</p><p>也可以从右侧成员列表点名，直接交代任务。</p></div>}
      {visibleMessages.map((message) => {
        const stream = visibleStreams.find((s) => s.messageId === message.id);
        const content = stream && !stream.final ? stream.text : message.content;
        const note = message.senderType === "user" || message.senderType === "system" ? null : modelNote(message.modelInfo);
        return <article key={message.id} className={`studio-message ${message.senderType === "user" ? "studio-message-user" : ""}`}>
          <span className="studio-message-avatar" aria-hidden style={{ borderBottomColor: message.senderAgentId ? shirtColor(message.senderAgentId) : undefined }}>{sender(message.senderAgentId, message.senderType).slice(0, 1)}</span>
          <div className="min-w-0"><p className="mb-2 flex flex-wrap items-baseline gap-y-1 text-xs text-slate-400">
            <span className="font-medium text-slate-200">{sender(message.senderAgentId, message.senderType)}</span>
            {note && <span className="ml-2 max-w-full break-words text-xs text-slate-500" title={`使用模型：${note}`}>{note}</span>}</p>
          <div className="studio-message-content">{content || (stream?.finished ? "（本回合没有正文）" : "等待成员回复…")}</div></div>
        </article>;
      })}
      {visibleStreams.filter((stream) => stream.messageId === null && !stream.final && Boolean(stream.text)).map((stream) => <article key={stream.runId} className="studio-message"><span className="studio-message-avatar" aria-hidden style={{ borderBottomColor: shirtColor(stream.agentId) }}>{sender(stream.agentId, "assistant").slice(0, 1)}</span><div className="min-w-0">
        <p className="mb-2 text-xs font-medium text-slate-200">{sender(stream.agentId, "assistant")}</p>
        <div className="studio-message-content">{stream.text}</div></div>
      </article>)}
      <div ref={tail} />
    </div>
    {visibleCleanup.map((notice) => { const view = describeCleanup(notice.folderPath); return <div key={notice.runId} role="alert" className="border-t border-amber-900 bg-amber-950/40 p-3 text-xs text-amber-200">
      <p><strong>{view.title}</strong> · {view.detail}</p>
      <p className="mt-1 text-amber-200/80">{view.next}</p>
      <Button className="mt-2" onClick={() => cleanup(notice.runId)}>我已处理残留进程，解除阻塞</Button>
    </div>; })}
    {runNotice && <div role="alert" className="mx-3 mb-2 rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-xs leading-5 text-red-800">
      <p><strong>{runNotice.title}</strong>{runNotice.detail && <span className="ml-2 break-words text-red-700">{runNotice.detail}</span>}</p>
      <p className="mt-1 text-red-700/80">{runNotice.next}</p>
    </div>}
    {projectMatches && state.error && <p role="alert" className="px-3 py-2 text-xs text-red-300">{state.error}</p>}
    {projectMatches && hint && <p className="mx-3 mb-2 rounded-lg border border-[#d8b878] bg-[#fbf3df] px-3 py-2 text-xs leading-5 text-[#6b4a12]">提示：{hint}</p>}
    {projectMatches && <InstructionBar projectStatus={projectStatus} instruction={instruction} tasks={tasks} agents={agents}
      supplement={supplement} busy={acting || busy} cleanupRequired={visibleCleanup.length > 0} onSupplementChange={setSupplement}
      onRetry={() => void resume("retry")} onSupplement={() => void resume("supplement", supplement)} onAbandon={() => void resume("abandon")}
      onAuthorizeElevated={(taskId) => void authorizeElevated(taskId)} />}
    <div className="studio-composer">
      {candidates.length > 0 && <div className="mb-2 flex flex-wrap gap-1">{candidates.map((agent) => <Button key={agent.id} onClick={() => { setInput(`@${agent.name} `); textarea.current?.focus(); }}>@{agent.name}</Button>)}</div>}
      <textarea ref={textarea} className={`${inputClass} resize-none !rounded-xl !bg-white !px-4 !py-3`} rows={3} aria-label="消息内容" placeholder="给组长下发任务，或 @成员" value={input}
        disabled={!folderExists} onChange={(event) => setInput(event.target.value)} onKeyDown={(event) => {
          if (shouldSendOnEnter({ key: event.key, shiftKey: event.shiftKey, isComposing: event.nativeEvent.isComposing, keyCode: event.nativeEvent.keyCode })) { event.preventDefault(); void send(); }
        }} />
      {canQueue && <p className="mt-2 text-[11px] leading-5 text-slate-500">团队运行中发送的补充或点名会排队，当前回合结束后处理；并行讨论会等本轮结束。</p>}
      <div className="mt-3 flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-wrap items-center gap-3">
          <p className="text-[11px] text-slate-500"><kbd className="rounded border border-slate-700 bg-white px-1">Enter</kbd> 发送 · Shift+Enter 换行</p>
          <div className="flex items-center gap-2 text-[11px] text-slate-500">
            <Switch checked={allowElevated} disabled={!folderExists} label="受阻时自动提权" onChange={(next) => void toggleElevation(next)} />
            <span>受阻时自动提权</span>
            <Tips label="关于受阻时自动提权">
              <p>执行自动运行工具，可能写出项目目录；沿用 CLI 配置。</p>
              <p className="mt-1">开启自动提权后，受阻任务会用最高权限重跑一次，并恢复原档位。只影响之后新建的指令，可写 .git、可删文件。</p>
            </Tips>
          </div>
        </div>
        <div className="flex shrink-0 gap-2"><Button variant="danger" disabled={!projectMatches || !state.activeRunId || stopping} onClick={stop}>{stopping ? "正在停止…" : "停止"}</Button>
          <Button variant="primary" disabled={!canSend(state)} onClick={send}>发送指令<Icon name="arrow" width="16" height="16" /></Button></div>
      </div>
    </div>
  </section>;
}
