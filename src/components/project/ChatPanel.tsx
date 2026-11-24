import { confirm } from "@tauri-apps/plugin-dialog";
import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { useStore } from "zustand";
import { api } from "../../api";
import { errorHint } from "../../lib/errorHint";
import { mentionCandidates, parseMention, shouldSendOnEnter } from "../../lib/mentions";
import { modelNote } from "../../lib/modelNote";
import { describeCleanup, describeRunFinished } from "../../lib/runErrors";
import { shirtColor } from "../../office/layout";
import { attachProjectEvents, projectStore } from "../../stores/project";
import type { Agent, Instruction, ProjectStatus, TaskItem } from "../../types";
import { Icon } from "../ui/Icon";
import { Switch } from "../ui/Switch";
import { Button } from "../ui/ui";
import { InstructionBar } from "./InstructionBar";

const EMPTY_TASKS: TaskItem[] = [];

/** 悬停或键盘聚焦时在上方弹出的说明气泡。 */
function Tips({ label, children }: { label: string; children: ReactNode }) {
  const id = useId();
  return (
    <span className="group relative inline-flex">
      <button
        type="button"
        aria-label={label}
        aria-describedby={id}
        className="inline-flex h-4 w-4 items-center justify-center rounded-full text-ink-muted hover:text-ink focus-visible:text-ink"
      >
        <Icon name="info" width="14" height="14" />
      </button>
      <span
        id={id}
        role="tooltip"
        className="pointer-events-none invisible absolute bottom-full left-1/2 z-20 mb-2 w-72 -translate-x-1/2 rounded-lg border border-line bg-surface px-3 py-2 text-xs leading-5 text-ink-secondary opacity-0 shadow-[0_8px_24px_rgba(24,25,23,0.1)] transition-opacity group-hover:visible group-hover:opacity-100 group-focus-within:visible group-focus-within:opacity-100"
      >
        {children}
      </span>
    </span>
  );
}

function kindBadge(kind: string, round: number | null): { text: string; className: string } | null {
  if (kind === "instruction") return { text: "指令", className: "bg-surface text-ink-secondary border border-line" };
  if (kind === "decision") return { text: "决议", className: "bg-accent-soft text-accent border border-accent-border" };
  if (kind === "report") return { text: "汇报", className: "bg-surface-subtle text-ink-secondary border border-line" };
  if (kind === "discussion") {
    return {
      text: round ? `讨论 · 第 ${round} 轮` : "讨论",
      className: "bg-surface-subtle text-ink-muted border border-line",
    };
  }
  return null;
}

interface Props {
  projectId: string;
  agents: Agent[];
  folderExists: boolean;
  mention?: string;
  onMentionUsed?: () => void;
  onFinished?: () => void;
}

export function ChatPanel({ projectId, agents, folderExists, mention, onMentionUsed, onFinished }: Props) {
  const state = useStore(projectStore);
  const [input, setInput] = useState("");
  const [loadingHistory, setLoadingHistory] = useState(false);
  const [hasMore, setHasMore] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [ready, setReady] = useState(false);
  const [projectStatus, setProjectStatus] = useState<ProjectStatus>("idle");
  const [instruction, setInstruction] = useState<Instruction | null>(null);
  const [supplement, setSupplement] = useState("");
  const [acting, setActing] = useState(false);
  const [allowElevated, setAllowElevated] = useState(false);
  const sendPending = useRef(false);
  const textarea = useRef<HTMLTextAreaElement>(null);
  const messagesBox = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let disposed = false;
    setReady(false);
    projectStore.getState().beginHydrate(projectId);
    const subscription = attachProjectEvents(api.listenProjectEvent, (name, payload) => {
      projectStore.getState().applyEvent(name, payload);
      if (name === "run:finished" && payload.projectId === projectId && !("replacement" in payload && payload.replacement)) {
        onFinished?.();
        Promise.all([api.getProjectRuntime(projectId), api.getProject(projectId), api.listInstructions(projectId)])
          .then(async ([runtime, detail, found]) => {
            if (disposed || projectStore.getState().projectId !== projectId) return;
            projectStore.setState({ cleanupRequired: runtime.cleanupRequired });
            const current = found[0] ?? null;
            const taskPage = current ? await api.listTasks(current.id) : [];
            if (disposed || projectStore.getState().projectId !== projectId) return;
            setProjectStatus(detail.project.status);
            setInstruction(current);
            projectStore.getState().setTasks(projectId, taskPage);
          })
          .catch((error) => {
            if (!disposed) projectStore.getState().setError(String(error));
          });
      }
    });
    subscription.ready
      .then(async () => {
        const [history, runtime, detail, found, elevation] = await Promise.all([
          api.listMessages(projectId),
          api.getProjectRuntime(projectId),
          api.getProject(projectId),
          api.listInstructions(projectId),
          api.getProjectElevation(projectId).catch(() => false),
        ]);
        if (disposed) return;
        const current = found[0] ?? null;
        const taskPage = current ? await api.listTasks(current.id) : [];
        if (disposed) return;
        projectStore.getState().hydrate(projectId, history, runtime);
        setHasMore(history.length === 50);
        setProjectStatus(detail.project.status);
        setInstruction(current);
        projectStore.getState().setTasks(projectId, taskPage);
        setAllowElevated(elevation);
        setReady(true);
      })
      .catch((error) => {
        if (!disposed) {
          projectStore.getState().setError(String(error));
          setReady(false);
        }
      });
    return () => {
      disposed = true;
      subscription.dispose();
    };
  }, [projectId, onFinished]);

  useEffect(() => {
    if (mention) {
      setInput(`@${mention} `);
      textarea.current?.focus();
      onMentionUsed?.();
    }
  }, [mention, onMentionUsed]);

  useEffect(() => {
    const box = messagesBox.current;
    if (box && box.scrollHeight > box.clientHeight) {
      box.scrollTop = box.scrollHeight;
    }
  }, [state.messages, state.streams, projectId]);

  const projectMatches = state.projectId === projectId;
  const tasks = useStore(projectStore, (current) => (current.projectId === projectId ? current.tasks : EMPTY_TASKS));
  const visibleMessages = projectMatches ? state.messages : [];
  const visibleStreams = projectMatches ? Object.values(state.streams) : [];
  const visibleCleanup = projectMatches ? state.cleanupRequired : [];
  const lastRun = projectMatches ? state.lastRun : null;
  const runNotice = lastRun ? describeRunFinished(lastRun.status, lastRun.error) : null;
  const hint = errorHint(lastRun?.error) ?? errorHint(state.error);
  const busy = projectMatches && (Boolean(state.activeRunId) || state.sending);
  const canQueue = instruction?.projectId === projectId && instruction.kind === "team" && instruction.status === "running";

  function canSend(current: typeof state) {
    return (
      ready &&
      current.projectId === projectId &&
      !sendPending.current &&
      !current.sending &&
      !stopping &&
      !acting &&
      folderExists &&
      current.cleanupRequired.length === 0 &&
      Boolean(input.trim()) &&
      (!current.activeRunId || canQueue)
    );
  }

  const candidates = mentionCandidates(input, agents, projectId);

  async function send() {
    if (!canSend(projectStore.getState())) return;
    const recipient = parseMention(input, agents);
    if ("error" in recipient) {
      state.setError(recipient.error);
      return;
    }
    // 同步锁独立于回合事件，防止重渲染前双击，或旧回合结束时再次提交同一条消息。
    sendPending.current = true;
    const content = input.trim();
    state.setSending(true);
    state.setError(null);
    try {
      // 先落盘开关，保证本次指令创建时快照到的是用户刚选的档位。
      await api.saveProjectElevation(projectId, allowElevated);
      const result = await api.sendMessage(projectId, content);
      if (projectStore.getState().projectId !== projectId) return;
      projectStore.getState().applyEvent("message:new", { projectId, runId: result.runId, message: result.userMessage });
      if (result.agentMessageId > 0) {
        projectStore.getState().mergeHistory(projectId, [
          {
            id: result.agentMessageId,
            projectId,
            senderType: "assistant",
            senderAgentId: recipient.agent.id,
            kind: "chat",
            content: "",
            round: null,
            createdAt: Date.now(),
            modelInfo: null,
          },
        ]);
        projectStore.getState().registerRun(projectId, result.runId, recipient.agent.id, result.agentMessageId);
      }
      setInput("");
      await refreshTeam();
    } catch (error) {
      if (projectStore.getState().projectId === projectId) state.setError(String(error));
    } finally {
      sendPending.current = false;
      if (projectStore.getState().projectId === projectId) state.setSending(false);
    }
  }

  async function stop() {
    setStopping(true);
    try {
      await api.stopProject(projectId);
      await refreshTeam();
      onFinished?.();
    } catch (error) {
      state.setError(String(error));
    } finally {
      setStopping(false);
    }
  }

  async function older() {
    setLoadingHistory(true);
    try {
      const page = await api.listMessages(projectId, visibleMessages[0]?.id ?? null);
      if (projectStore.getState().projectId !== projectId) return;
      state.mergeHistory(projectId, page);
      setHasMore(page.length === 50);
    } catch (error) {
      state.setError(String(error));
    } finally {
      setLoadingHistory(false);
    }
  }

  async function refreshTeam() {
    const [detail, found] = await Promise.all([api.getProject(projectId), api.listInstructions(projectId)]);
    if (projectStore.getState().projectId !== projectId) return;
    const current = found[0] ?? null;
    const taskPage = current ? await api.listTasks(current.id) : [];
    if (projectStore.getState().projectId !== projectId) return;
    setProjectStatus(detail.project.status);
    setInstruction(current);
    projectStore.getState().setTasks(projectId, taskPage);
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
    } catch (error) {
      if (projectStore.getState().projectId === projectId) state.setError(String(error));
    } finally {
      if (projectStore.getState().projectId === projectId) setActing(false);
    }
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
    } catch (error) {
      if (projectStore.getState().projectId === projectId) state.setError(String(error));
    } finally {
      if (projectStore.getState().projectId === projectId) setActing(false);
    }
  }

  async function toggleElevation(next: boolean) {
    setAllowElevated(next);
    try {
      await api.saveProjectElevation(projectId, next);
    } catch (error) {
      setAllowElevated(!next);
      if (projectStore.getState().projectId === projectId) state.setError(String(error));
    }
  }

  async function cleanup(runId: string) {
    const accepted = await confirm(
      "请先在终端或活动监视器中确认该目录相关的残留 CLI 及子进程已停止，并检查文件修改。确认后解除此运行的目录阻塞。",
      { title: "确认已人工处理残留进程", kind: "warning" },
    );
    if (!accepted) return;
    try {
      await api.confirmRuntimeCleanup(runId);
      const runtime = await api.getProjectRuntime(projectId);
      const current = projectStore.getState();
      if (current.projectId === projectId) current.hydrate(projectId, current.messages, runtime);
    } catch (error) {
      state.setError(String(error));
    }
  }

  const sender = (id: string | null, type: string) =>
    type === "user" ? "你" : type === "system" ? "系统" : agents.find((a) => a.id === id)?.name ?? "成员";

  return (
    <section className="studio-chat" aria-label="项目群聊">
      <div ref={messagesBox} className="studio-messages" aria-live="polite">
        {hasMore && (
          <div className="mb-3 flex justify-center">
            <Button disabled={loadingHistory} onClick={older}>
              {loadingHistory ? "加载中…" : "加载更早消息"}
            </Button>
          </div>
        )}

        {!visibleMessages.length && !busy && (
          <div className="studio-empty-chat">
            <span className="grid size-12 place-items-center rounded-xl border border-accent-border bg-accent-soft text-accent">
              <Icon name="chat" width="22" height="22" />
            </span>
            <h3>团队已就位</h3>
            <p>向组长下发目标，团队会在此讨论方案、拆解任务并汇报交付进展。</p>
            <p className="text-xs text-ink-muted">也可在右侧成员页点击 ＠ 直接向指定成员派活</p>
          </div>
        )}

        {visibleMessages.map((message) => {
          const stream = visibleStreams.find((s) => s.messageId === message.id);
          const content = stream && !stream.final ? stream.text : message.content;

          if (message.senderType === "system") {
            return (
              <div key={message.id} className="studio-system-event">
                <span>{content}</span>
              </div>
            );
          }

          const note = message.senderType === "user" ? null : modelNote(message.modelInfo);
          const badge = kindBadge(message.kind, message.round);
          const isUser = message.senderType === "user";
          const isDecision = message.kind === "decision";

          return (
            <article
              key={message.id}
              className={`studio-message ${isUser ? "studio-message-user" : ""} ${isDecision ? "studio-message-decision" : ""}`}
            >
              <span
                className="studio-message-avatar"
                aria-hidden="true"
                style={{ borderBottomColor: message.senderAgentId ? shirtColor(message.senderAgentId) : undefined }}
              >
                {sender(message.senderAgentId, message.senderType).slice(0, 1)}
              </span>
              <div className="min-w-0">
                <p className="mb-1.5 flex flex-wrap items-center gap-2 text-xs">
                  <span className="font-medium text-ink">{sender(message.senderAgentId, message.senderType)}</span>
                  {badge && (
                    <span className={`rounded px-1.5 py-0.2 text-[11px] font-medium leading-4 ${badge.className}`}>
                      {badge.text}
                    </span>
                  )}
                  {note && (
                    <span className="max-w-full break-words font-mono text-[11px] text-ink-muted" title={`使用模型：${note}`}>
                      {note}
                    </span>
                  )}
                </p>
                <div className="studio-message-content">
                  {content || (stream?.finished ? "（本回合没有正文）" : "等待成员回复…")}
                </div>
              </div>
            </article>
          );
        })}

        {visibleStreams
          .filter((stream) => stream.messageId === null && !stream.final && Boolean(stream.text))
          .map((stream) => (
            <article key={stream.runId} className="studio-message">
              <span
                className="studio-message-avatar"
                aria-hidden="true"
                style={{ borderBottomColor: shirtColor(stream.agentId) }}
              >
                {sender(stream.agentId, "assistant").slice(0, 1)}
              </span>
              <div className="min-w-0">
                <p className="mb-1.5 flex items-center gap-2 text-xs">
                  <span className="font-medium text-ink">{sender(stream.agentId, "assistant")}</span>
                  <span className="rounded bg-accent-soft px-1.5 py-0.2 text-[11px] font-medium text-accent">输出中</span>
                </p>
                <div className="studio-message-content">{stream.text}</div>
              </div>
            </article>
          ))}
      </div>

      {visibleCleanup.map((notice) => {
        const view = describeCleanup(notice.folderPath);
        return (
          <div key={notice.runId} role="alert" className="border-t border-warn-border bg-warn-soft px-4 py-3 text-xs text-warn">
            <p>
              <strong>{view.title}</strong> · {view.detail}
            </p>
            <p className="mt-1 opacity-90">{view.next}</p>
            <Button className="mt-2" onClick={() => cleanup(notice.runId)}>
              我已处理残留进程，解除阻塞
            </Button>
          </div>
        );
      })}

      {runNotice && (
        <div role="alert" className="mx-4 mb-2 rounded-lg border border-danger-border bg-danger-soft px-3.5 py-2.5 text-xs leading-5 text-danger">
          <p>
            <strong>{runNotice.title}</strong>
            {runNotice.detail && <span className="ml-2 break-words">{runNotice.detail}</span>}
          </p>
          <p className="mt-1 opacity-90">{runNotice.next}</p>
        </div>
      )}

      {projectMatches && state.error && (
        <p role="alert" className="mx-4 mb-2 rounded-lg border border-danger-border bg-danger-soft px-3.5 py-2 text-xs text-danger">
          {state.error}
        </p>
      )}

      {projectMatches && hint && (
        <p className="mx-4 mb-2 rounded-lg border border-warn-border bg-warn-soft px-3.5 py-2 text-xs leading-5 text-warn">
          提示：{hint}
        </p>
      )}

      {projectMatches && (
        <InstructionBar
          projectStatus={projectStatus}
          instruction={instruction}
          tasks={tasks}
          supplement={supplement}
          busy={acting || busy}
          cleanupRequired={visibleCleanup.length > 0}
          onSupplementChange={setSupplement}
          onRetry={() => void resume("retry")}
          onSupplement={() => void resume("supplement", supplement)}
          onAbandon={() => void resume("abandon")}
          onAuthorizeElevated={(taskId) => void authorizeElevated(taskId)}
        />
      )}

      <div className="studio-composer">
        {candidates.length > 0 && (
          <div className="mb-2 flex flex-wrap gap-1.5">
            {candidates.map((agent) => (
              <Button
                key={agent.id}
                className="min-h-7 px-2.5 py-1 text-xs"
                onClick={() => {
                  setInput(`@${agent.name} `);
                  textarea.current?.focus();
                }}
              >
                @{agent.name}
              </Button>
            ))}
          </div>
        )}

        <div className="studio-composer-box">
          <textarea
            ref={textarea}
            className="w-full min-w-0 resize-none bg-transparent px-3.5 py-2.5 text-sm leading-5 text-ink placeholder:text-ink-muted focus:outline-none disabled:cursor-not-allowed disabled:text-ink-muted"
            rows={2}
            aria-label="消息内容"
            placeholder="给组长下发任务，或输入 @成员 直接点名"
            value={input}
            disabled={!folderExists}
            onChange={(event) => setInput(event.target.value)}
            onKeyDown={(event) => {
              if (
                shouldSendOnEnter({
                  key: event.key,
                  shiftKey: event.shiftKey,
                  isComposing: event.nativeEvent.isComposing,
                  keyCode: event.nativeEvent.keyCode,
                })
              ) {
                event.preventDefault();
                void send();
              }
            }}
          />

          <div className="flex flex-wrap items-center justify-between gap-2 border-t border-line/70 px-3 py-2">
            <div className="flex flex-wrap items-center gap-3 text-[11px] text-ink-muted">
              <span>
                <kbd className="rounded border border-line-strong bg-surface-subtle px-1.5 py-0.5 font-mono text-[10px] text-ink-secondary">
                  Enter
                </kbd>{" "}
                发送<span className="hidden xl:inline"> · Shift+Enter 换行</span>
              </span>
              <span className="inline-flex items-center gap-1.5">
                <Switch
                  checked={allowElevated}
                  disabled={!folderExists}
                  label="受阻时自动提权"
                  onChange={(next) => void toggleElevation(next)}
                />
                <span>受阻时自动提权</span>
                <Tips label="关于受阻时自动提权">
                  <p>执行模式自动运行工具，写入范围取决于 CLI 配置，可能超出项目目录。</p>
                  <p className="mt-1">开启自动提权后，受阻任务会用最高权限重跑一次并恢复原档位；仅对之后新建的指令生效。</p>
                </Tips>
              </span>
            </div>

            <div className="flex shrink-0 items-center gap-2">
              <Button
                variant="danger"
                className="min-h-8 px-3 py-1 text-xs"
                disabled={!projectMatches || !state.activeRunId || stopping}
                onClick={stop}
              >
                {stopping ? "正在停止…" : "停止"}
              </Button>
              <Button variant="primary" className="min-h-8 px-3.5 py-1 text-xs" disabled={!canSend(state)} onClick={send}>
                发送指令<Icon name="arrow" width="14" height="14" />
              </Button>
            </div>
          </div>
        </div>

        {canQueue && (
          <p className="mt-1.5 px-1 text-[11px] text-ink-muted">
            团队运行中发送的补充或点名会进入队列，在当前回合或讨论轮结束后处理
          </p>
        )}
      </div>
    </section>
  );
}
