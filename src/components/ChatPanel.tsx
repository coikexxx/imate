import { confirm } from "@tauri-apps/plugin-dialog";
import { useEffect, useRef, useState } from "react";
import { useStore } from "zustand";
import { api } from "../api";
import { mentionCandidates, parseMention, shouldSendOnEnter } from "../lib/mentions";
import { modelNote } from "../lib/modelNote";
import { shirtColor } from "../office/layout";
import { attachProjectEvents, projectStore } from "../stores/project";
import type { Agent, Instruction, ProjectStatus, TaskItem } from "../types";
import { InstructionBar } from "./InstructionBar";
import { Button, inputClass } from "./ui";

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
      const [history, runtime, detail, found] = await Promise.all([
        api.listMessages(projectId), api.getProjectRuntime(projectId), api.getProject(projectId), api.listInstructions(projectId),
      ]);
      if (disposed) return;
      const current = found[0] ?? null;
      const taskPage = current ? await api.listTasks(current.id) : [];
      if (disposed) return;
      projectStore.getState().hydrate(projectId, history, runtime); setHasMore(history.length === 50);
      setProjectStatus(detail.project.status);
      setInstruction(current);
      setTasks(taskPage);
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
  const busy = projectMatches && (Boolean(state.activeRunId) || state.sending);
  const candidates = mentionCandidates(input, agents, projectId);
  async function send() {
    if (!ready || busy || !folderExists || state.cleanupRequired.length > 0) return;
    const recipient = parseMention(input, agents);
    if ("error" in recipient) { state.setError(recipient.error); return; }
    const content = input.trim(); state.setSending(true); state.setError(null);
    try {
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
    finally { if (projectStore.getState().projectId === projectId) state.setSending(false); }
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
    if (!instruction || acting) return;
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
  return <section className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl border border-slate-800 bg-slate-950" aria-label="项目群聊">
    <header className="flex flex-wrap items-center justify-between gap-2 border-b border-slate-800 px-4 py-3 text-sm">
      <span className="font-medium">项目群聊</span><span className="text-xs text-slate-500">消息交给组长 · @ 点名成员</span>
    </header>
    <div className="min-h-0 flex-1 overflow-y-auto px-4 py-2" aria-live="polite">
      {hasMore && <Button disabled={loadingHistory} onClick={older}>{loadingHistory ? "加载中…" : "加载更早消息"}</Button>}
      {!visibleMessages.length && !busy && <p className="py-6 text-center text-sm text-slate-500">下发第一条任务，或从成员列表点名。</p>}
      {visibleMessages.map((message) => {
        const stream = visibleStreams.find((s) => s.messageId === message.id);
        const content = stream && !stream.final ? stream.text : message.content;
        const note = message.senderType === "user" || message.senderType === "system" ? null : modelNote(message.modelInfo);
        return <article key={message.id} className={`border-b border-slate-800/60 px-1 py-4 ${message.senderType === "user" ? "my-2 rounded-lg border-b-0 bg-slate-900 px-3" : message.senderType === "system" ? "text-slate-400" : ""}`}>
          <p className="mb-2 flex flex-wrap items-baseline gap-y-1 text-xs text-slate-400">
            {message.senderAgentId && <span aria-hidden className="mr-2 inline-block h-2 w-1 shrink-0 rounded-sm" style={{ backgroundColor: shirtColor(message.senderAgentId) }} />}
            <span className="font-medium text-slate-300">{sender(message.senderAgentId, message.senderType)}</span>
            {note && <span className="ml-2 max-w-full break-words text-xs text-slate-500" title={`使用模型：${note}`}>{note}</span>}</p>
          <div className="whitespace-pre-wrap break-words text-sm leading-relaxed">{content || (stream?.finished ? "（本回合没有正文）" : "等待成员回复…")}</div>
        </article>;
      })}
      {visibleStreams.filter((stream) => stream.messageId === null && !stream.final && Boolean(stream.text)).map((stream) => <article key={stream.runId} className="border-b border-slate-800/60 px-1 py-4">
        <p className="mb-2 flex items-center gap-2 text-xs font-medium text-slate-300"><span aria-hidden className="h-2 w-1 rounded-sm" style={{ backgroundColor: shirtColor(stream.agentId) }} />{sender(stream.agentId, "assistant")}</p>
        <div className="whitespace-pre-wrap break-words text-sm leading-relaxed">{stream.text}</div>
      </article>)}
      <div ref={tail} />
    </div>
    {visibleCleanup.map((notice) => <div key={notice.runId} className="border-t border-amber-900 bg-amber-950/40 p-3 text-xs text-amber-200">
      <p>上次运行被中断，需确认残留已处理：{notice.folderPath}</p>
      <Button className="mt-2" onClick={() => cleanup(notice.runId)}>我已处理残留进程，解除阻塞</Button>
    </div>)}
    {projectMatches && state.error && <p role="alert" className="px-3 py-2 text-xs text-red-300">{state.error}</p>}
    {projectMatches && <InstructionBar projectStatus={projectStatus} instruction={instruction} tasks={tasks} agents={agents}
      supplement={supplement} busy={acting || busy} onSupplementChange={setSupplement}
      onRetry={() => void resume("retry")} onSupplement={() => void resume("supplement", supplement)} onAbandon={() => void resume("abandon")} />}
    <div className="border-t border-slate-800 p-4">
      {candidates.length > 0 && <div className="mb-2 flex flex-wrap gap-1">{candidates.map((agent) => <Button key={agent.id} onClick={() => { setInput(`@${agent.name} `); textarea.current?.focus(); }}>@{agent.name}</Button>)}</div>}
      <textarea ref={textarea} className={`${inputClass} resize-none`} rows={3} aria-label="消息内容" placeholder="给组长下发任务，或 @成员" value={input}
        disabled={!folderExists} onChange={(event) => setInput(event.target.value)} onKeyDown={(event) => {
          if (shouldSendOnEnter({ key: event.key, shiftKey: event.shiftKey, isComposing: event.nativeEvent.isComposing, keyCode: event.nativeEvent.keyCode })) { event.preventDefault(); void send(); }
        }} />
      <div className="mt-3 flex flex-wrap items-center justify-between gap-3">
        <p className="text-xs text-slate-500">Enter 发送 · Shift+Enter 换行</p>
        <div className="flex shrink-0 gap-2"><Button variant="danger" disabled={!projectMatches || !state.activeRunId || stopping} onClick={stop}>{stopping ? "正在停止…" : "停止"}</Button>
          <Button variant="primary" disabled={!ready || busy || !input.trim() || !folderExists || state.cleanupRequired.length > 0} onClick={send}>发送</Button></div>
      </div>
      <p className="mt-3 text-xs leading-5 text-slate-500">执行自动运行工具，可能写出项目目录；沿用 CLI 配置。</p>
    </div>
  </section>;
}
