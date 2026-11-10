import { confirm } from "@tauri-apps/plugin-dialog";
import { useEffect, useRef, useState } from "react";
import { useStore } from "zustand";
import { api } from "../api";
import { mentionCandidates, parseMention, shouldSendOnEnter } from "../lib/mentions";
import { attachProjectEvents, projectStore } from "../stores/project";
import type { Agent } from "../types";
import { Button, inputClass } from "./ui";

interface Props { projectId: string; agents: Agent[]; folderExists: boolean; mention?: string; onMentionUsed?: () => void; onFinished?: () => void }
export function ChatPanel({ projectId, agents, folderExists, mention, onMentionUsed, onFinished }: Props) {
  const state = useStore(projectStore);
  const [input, setInput] = useState("");
  const [loadingHistory, setLoadingHistory] = useState(false);
  const [hasMore, setHasMore] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [ready, setReady] = useState(false);
  const textarea = useRef<HTMLTextAreaElement>(null);
  const tail = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let disposed = false;
    setReady(false);
    projectStore.getState().beginHydrate(projectId);
    const subscription = attachProjectEvents(api.listenProjectEvent, (name, payload) => {
      projectStore.getState().applyEvent(name, payload);
      if (name === "run:finished" && payload.projectId === projectId) {
        onFinished?.();
        api.getProjectRuntime(projectId).then((runtime) => {
          if (!disposed && projectStore.getState().projectId === projectId) projectStore.setState({ cleanupRequired: runtime.cleanupRequired });
        }).catch((error) => { if (!disposed) projectStore.getState().setError(String(error)); });
      }
    });
    subscription.ready.then(async () => {
      const [history, runtime] = await Promise.all([api.listMessages(projectId), api.getProjectRuntime(projectId)]);
      if (disposed) return;
      projectStore.getState().hydrate(projectId, history, runtime); setHasMore(history.length === 50); setReady(true);
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
      projectStore.getState().mergeHistory(projectId, [{ id: result.agentMessageId, projectId, senderType: "assistant",
        senderAgentId: recipient.agent.id, kind: "chat", content: "", round: null, createdAt: Date.now() }]);
      projectStore.getState().registerRun(projectId, result.runId, recipient.agent.id, result.agentMessageId);
      setInput("");
    } catch (error) { if (projectStore.getState().projectId === projectId) state.setError(String(error)); }
    finally { if (projectStore.getState().projectId === projectId) state.setSending(false); }
  }
  async function stop() {
    setStopping(true);
    try { await api.stopProject(projectId); }
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
  return <section className="flex min-h-0 flex-col overflow-hidden rounded border border-slate-800 bg-slate-950" aria-label="项目群聊">
    <header className="flex items-center justify-between border-b border-slate-800 px-3 py-2 text-sm">
      <span>项目群聊</span><span className="text-xs text-slate-500">普通消息交给组长 · @ 点名单个成员</span>
    </header>
    <div className="min-h-0 flex-1 overflow-y-auto p-3" aria-live="polite">
      {hasMore && <Button disabled={loadingHistory} onClick={older}>{loadingHistory ? "加载中…" : "加载更早消息"}</Button>}
      {!visibleMessages.length && !busy && <p className="py-6 text-center text-sm text-slate-500">下发第一条任务，或从成员列表点名。</p>}
      {visibleMessages.map((message) => {
        const stream = visibleStreams.find((s) => s.messageId === message.id);
        const content = stream && !stream.final ? stream.text : message.content;
        return <article key={message.id} className={`mb-3 rounded px-3 py-2 ${message.senderType === "user" ? "bg-sky-950/50" : message.senderType === "system" ? "bg-amber-950/30" : "bg-slate-900"}`}>
          <p className="mb-1 text-xs text-slate-400">{sender(message.senderAgentId, message.senderType)}</p>
          <div className="whitespace-pre-wrap break-words text-sm leading-relaxed">{content || (stream?.finished ? "（本回合没有正文）" : "等待成员回复…")}</div>
        </article>;
      })}
      {visibleStreams.filter((stream) => stream.messageId === null && !stream.final && Boolean(stream.text)).map((stream) => <article key={stream.runId} className="mb-3 rounded bg-slate-900 px-3 py-2">
        <p className="mb-1 text-xs text-slate-400">{sender(stream.agentId, "assistant")}</p>
        <div className="whitespace-pre-wrap break-words text-sm">{stream.text}</div>
      </article>)}
      <div ref={tail} />
    </div>
    {visibleCleanup.map((notice) => <div key={notice.runId} className="border-t border-amber-900 bg-amber-950/40 p-3 text-xs text-amber-200">
      <p>上次运行被中断，需确认残留已处理：{notice.folderPath}</p>
      <Button className="mt-2" onClick={() => cleanup(notice.runId)}>我已处理残留进程，解除阻塞</Button>
    </div>)}
    {projectMatches && state.error && <p role="alert" className="px-3 py-2 text-xs text-red-300">{state.error}</p>}
    <div className="border-t border-slate-800 p-3">
      {candidates.length > 0 && <div className="mb-2 flex flex-wrap gap-1">{candidates.map((agent) => <Button key={agent.id} onClick={() => { setInput(`@${agent.name} `); textarea.current?.focus(); }}>@{agent.name}</Button>)}</div>}
      <textarea ref={textarea} className={`${inputClass} resize-none`} rows={3} aria-label="消息内容" placeholder="给组长下发任务，或 @成员 发言；Shift+Enter 换行" value={input}
        disabled={!folderExists} onChange={(event) => setInput(event.target.value)} onKeyDown={(event) => {
          if (shouldSendOnEnter({ key: event.key, shiftKey: event.shiftKey, isComposing: event.nativeEvent.isComposing, keyCode: event.nativeEvent.keyCode })) { event.preventDefault(); void send(); }
        }} />
      <div className="mt-2 flex items-center justify-between gap-3">
        <p className="text-xs text-slate-500">执行模式自动运行工具，可能写出项目目录。CLI 配置会被继承。</p>
        <div className="flex shrink-0 gap-2"><Button variant="danger" disabled={!projectMatches || !state.activeRunId || stopping} onClick={stop}>{stopping ? "正在停止…" : "停止"}</Button>
          <Button variant="primary" disabled={!ready || busy || !input.trim() || !folderExists || state.cleanupRequired.length > 0} onClick={send}>发送</Button></div>
      </div>
    </div>
  </section>;
}
