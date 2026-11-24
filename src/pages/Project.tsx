import { confirm, open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useState } from "react";
import { Link, useParams } from "react-router";
import { useStore } from "zustand";
import { api } from "../api";
import { AppShell } from "../components/layout/AppShell";
import { AgentForm } from "../components/project/AgentForm";
import { ChatPanel } from "../components/project/ChatPanel";
import { FileTree } from "../components/project/FileTree";
import { MemberList } from "../components/project/MemberList";
import { ProjectUsagePanel } from "../components/project/ProjectUsagePanel";
import { RulesPanel } from "../components/project/RulesPanel";
import { TrajectoryPanel } from "../components/project/TrajectoryPanel";
import { Icon } from "../components/ui/Icon";
import { Button, Modal } from "../components/ui/ui";
import { closedCliIds } from "../lib/cliConfig";
import { assistantAvailable } from "../lib/clis";
import { OfficeCanvas } from "../office/OfficeCanvas";
import { projectStore } from "../stores/project";
import {
  STATUS_LABELS,
  type Agent,
  type AgentInput,
  type CliInfo,
  type PersonaTemplate,
  type ProjectDetail,
  type ProjectStatus,
  type RuleDoc,
  type TaskItem,
} from "../types";

const EMPTY_AGENT_STATUSES: Record<string, import("../types").AgentStatus> = {};
const EMPTY_FILES: string[] = [];
const EMPTY_TASKS: TaskItem[] = [];
const TAB_LABELS = { chat: "团队讨论", files: "项目文件", rules: "规章制度", usage: "用量" } as const;

const STATUS_BADGE: Record<ProjectStatus, string> = {
  idle: "bg-surface-subtle text-ink-secondary border-line",
  discussing: "bg-accent-soft text-accent border-accent-border",
  executing: "bg-accent-soft text-accent border-accent-border",
  waiting_user: "bg-warn-soft text-warn border-warn-border",
};

type Editing = { kind: "new" } | { kind: "edit"; agent: Agent } | null;

export default function Project() {
  const { id = "" } = useParams();
  const [detail, setDetail] = useState<ProjectDetail | null>(null);
  const [clis, setClis] = useState<CliInfo[]>([]);
  const [closedClis, setClosedClis] = useState<ReadonlySet<string>>(new Set());
  const [templates, setTemplates] = useState<PersonaTemplate[]>([]);
  const [projectRules, setProjectRules] = useState<RuleDoc[]>([]);
  const [editing, setEditing] = useState<Editing>(null);
  const [tab, setTab] = useState<"chat" | "files" | "rules" | "usage">("chat");
  const [teamTab, setTeamTab] = useState<"trajectory" | "members">("trajectory");
  const [usageTick, setUsageTick] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [mention, setMention] = useState<string | undefined>();

  const activeRunId = useStore(projectStore, (state) => (state.projectId === id ? state.activeRunId : null));
  const agentStatuses = useStore(projectStore, (state) => (state.projectId === id ? state.agentStatuses : EMPTY_AGENT_STATUSES));
  const changedFiles = useStore(projectStore, (state) => (state.projectId === id ? state.changedFiles : EMPTY_FILES));
  const filesTick = useStore(projectStore, (state) => (state.projectId === id ? state.filesTick : 0));
  const rulesTick = useStore(projectStore, (state) => (state.projectId === id ? state.rulesTick : 0));
  const tasks = useStore(projectStore, (state) => (state.projectId === id ? state.tasks : EMPTY_TASKS));
  const consumeMention = useCallback(() => setMention(undefined), []);

  const reload = useCallback(async () => {
    setDetail(await api.getProject(id));
  }, [id]);

  const refreshAfterRun = useCallback(() => {
    setUsageTick((n) => n + 1);
    reload().catch((e) => setError(String(e)));
  }, [reload]);

  useEffect(() => {
    reload().catch((e) => setError(String(e)));
    api.listClis().then(setClis);
    api.listCliConfigs().then((configs) => setClosedClis(closedCliIds(configs))).catch(() => setClosedClis(new Set()));
    api.listPersonaTemplates().then(setTemplates);
  }, [reload]);

  useEffect(() => {
    let disposed = false;
    api
      .watchProject(id)
      .then((changed) => {
        if (!disposed) projectStore.getState().setChangedFiles(id, changed);
      })
      .catch(() => {});
    return () => {
      disposed = true;
    };
  }, [id, detail?.project.folderPath]);

  useEffect(() => {
    function refreshProjectPage(event: KeyboardEvent) {
      if (
        !(event.metaKey || event.ctrlKey) ||
        event.altKey ||
        event.shiftKey ||
        event.isComposing ||
        event.key.toLowerCase() !== "r"
      )
        return;
      event.preventDefault();
      if (!event.repeat) window.location.reload();
    }
    window.addEventListener("keydown", refreshProjectPage);
    return () => window.removeEventListener("keydown", refreshProjectPage);
  }, []);

  if (error && !detail) {
    return (
      <AppShell>
        <main className="studio-page">
          <Link to="/" className="inline-flex items-center gap-1.5 text-sm text-ink-secondary hover:text-ink">
            <Icon name="back" width="14" height="14" />返回项目总览
          </Link>
          <p role="alert" className="mt-4 rounded-lg border border-danger-border bg-danger-soft px-3.5 py-2.5 text-sm text-danger">
            {error}
          </p>
        </main>
      </AppShell>
    );
  }
  if (!detail) {
    return (
      <AppShell>
        <main className="studio-page text-sm text-ink-secondary">正在载入项目工作台…</main>
      </AppShell>
    );
  }

  const { project, agents } = detail;
  const effectiveStatus: ProjectStatus = activeRunId ? "executing" : project.status;

  async function saveAgent(input: AgentInput) {
    if (editing?.kind === "edit") await api.updateAgent(editing.agent.id, input);
    else await api.createAgent(id, input);
    setEditing(null);
    await reload();
  }

  async function removeAgent(agent: Agent) {
    const confirmed = await confirm(`移除成员「${agent.name}」？`, {
      title: "移除成员",
      kind: "warning",
    });
    if (!confirmed) return;
    try {
      await api.deleteAgent(agent.id);
      setEditing(null);
      await reload();
    } catch (e) {
      setError(String(e));
    }
  }

  async function relocate() {
    const picked = await open({ directory: true, multiple: false, title: "重新选择项目文件夹" });
    if (typeof picked !== "string") return;
    try {
      await api.updateProject(id, project.name, project.description, picked);
      await reload();
    } catch (e) {
      setError(String(e));
    }
  }

  const editingAgent = editing?.kind === "edit" ? editing.agent : null;
  const assistantReady = assistantAvailable(clis, closedClis);

  async function openAgentForm(target: Editing) {
    try {
      setProjectRules(await api.listRules(id));
      setEditing(target);
    } catch (failure) {
      setError(String(failure));
    }
  }

  return (
    <AppShell projectName={project.name}>
      <div className="studio-project-layout">
        <header className="studio-project-topbar">
          <div className="studio-project-headrow">
            <div className="flex min-w-0 items-center gap-2.5">
              <Link to="/" className="shrink-0 text-xs text-ink-muted hover:text-ink">项目总览</Link>
              <span className="text-xs text-line-strong" aria-hidden="true">/</span>
              <h1 className="truncate text-[16px] font-medium text-ink">{project.name}</h1>
              <span className={`inline-flex shrink-0 items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-xs font-medium ${STATUS_BADGE[effectiveStatus]}`}>
                <span className="size-1.5 rounded-full bg-current" aria-hidden="true" />
                {STATUS_LABELS[effectiveStatus]}
              </span>
            </div>
            <p
              className="hidden max-w-80 truncate rounded-md bg-surface-subtle px-2.5 py-1 font-mono text-[11px] text-ink-secondary md:block"
              title={project.folderPath}
            >
              {project.folderPath}
            </p>
          </div>

          <div className="studio-work-tabs" role="tablist" aria-label="项目工作区">
            {(["chat", "files", "rules", "usage"] as const).map((key) => (
              <button
                key={key}
                type="button"
                role="tab"
                id={`project-tab-${key}`}
                aria-controls={`project-panel-${key}`}
                aria-selected={tab === key}
                onClick={() => setTab(key)}
              >
                <Icon name={key} width="15" height="15" />
                {TAB_LABELS[key]}
                {key === "files" && changedFiles.length > 0 && (
                  <span className="rounded bg-warn-soft px-1.5 py-0.2 font-mono text-[11px] font-medium tabular-nums text-warn">
                    {changedFiles.length}
                  </span>
                )}
              </button>
            ))}
          </div>
        </header>

        {!detail.folderExists && (
          <div className="flex items-center justify-between border-b border-danger-border bg-danger-soft px-6 py-2 text-xs text-danger">
            <span>项目文件夹不存在：{project.folderPath}</span>
            <Button onClick={relocate}>重新选择文件夹</Button>
          </div>
        )}
        {error && (
          <p role="alert" className="border-b border-danger-border bg-danger-soft px-6 py-2 text-xs text-danger">
            {error}
          </p>
        )}

        <div className="studio-project-body">
          <section className="studio-workspace">
            {/* 群聊始终挂载，切换视图时继续接收运行事件并保留输入。 */}
            <div
              id="project-panel-chat"
              role="tabpanel"
              aria-labelledby="project-tab-chat"
              hidden={tab !== "chat"}
              className={tab === "chat" ? "studio-work-panel" : "hidden"}
            >
              <ChatPanel
                projectId={id}
                agents={agents}
                folderExists={detail.folderExists}
                mention={mention}
                onMentionUsed={consumeMention}
                onFinished={refreshAfterRun}
              />
            </div>

            <div
              id="project-panel-rules"
              role="tabpanel"
              aria-labelledby="project-tab-rules"
              hidden={tab !== "rules"}
              className={tab === "rules" ? "studio-work-panel" : "hidden"}
            >
              <div className="border-b border-line bg-paper/60 px-4 py-2.5 text-xs text-ink-secondary">
                团队共同遵守的工作约定，修改保存后从下一回合自动注入通知
              </div>
              <div className="min-h-0 flex-1">
                <RulesPanel
                  key={`${detail.folderExists}:${project.folderPath}`}
                  projectId={id}
                  project={project}
                  agents={agents}
                  clis={clis}
                  closedCliIds={closedClis}
                  refreshTick={rulesTick}
                />
              </div>
            </div>

            {tab === "files" && (
              <div id="project-panel-files" role="tabpanel" aria-labelledby="project-tab-files" className="studio-work-panel">
                <div className="border-b border-line bg-paper/60 px-4 py-2.5 text-xs text-ink-secondary">
                  点击文件在系统默认程序中打开 · 本轮变化只记录文件改动，不区分修改者
                </div>
                <div className="min-h-0 flex-1">
                  {detail.folderExists ? (
                    <FileTree key={project.folderPath} projectId={id} changed={changedFiles} tick={filesTick} />
                  ) : (
                    <p className="p-5 text-sm text-ink-muted">项目文件夹不存在，无法列出文件。</p>
                  )}
                </div>
              </div>
            )}

            {tab === "usage" && (
              <div id="project-panel-usage" role="tabpanel" aria-labelledby="project-tab-usage" className="studio-work-panel">
                <div className="border-b border-line bg-paper/60 px-4 py-2.5 text-xs text-ink-secondary">
                  按成员与项目汇总已结束回合的真实回报用量 · 每次回合结束后自动刷新
                </div>
                <div className="min-h-0 flex-1 overflow-y-auto">
                  <ProjectUsagePanel projectId={id} clis={clis} refreshTick={usageTick} />
                </div>
              </div>
            )}
          </section>

          <aside className="studio-team-panel">
            <div className="studio-team-title">
              <span className="flex items-center gap-1.5">
                <Icon name="team" width="14" height="14" />团队现场
              </span>
              <span className="rounded bg-surface px-1.5 py-0.5 font-mono text-[11px] tabular-nums text-ink-muted">
                {agents.length} / 8
              </span>
            </div>

            <div className="studio-office">
              <OfficeCanvas agents={agents} statuses={agentStatuses} />
            </div>

            <div className="studio-team-tabs" role="tablist" aria-label="团队现场视图">
              {(["trajectory", "members"] as const).map((key) => (
                <button
                  key={key}
                  type="button"
                  role="tab"
                  id={`team-tab-${key}`}
                  aria-selected={teamTab === key}
                  aria-controls="team-panel"
                  onClick={() => setTeamTab(key)}
                >
                  <Icon name={key === "trajectory" ? "trajectory" : "team"} width="14" height="14" />
                  {key === "trajectory" ? "轨迹" : "团队成员"}
                  {key === "trajectory" && tasks.length > 0 && (
                    <span className="rounded bg-surface px-1.5 py-0.2 font-mono text-[11px] tabular-nums text-ink-muted">
                      {tasks.length}
                    </span>
                  )}
                </button>
              ))}
            </div>

            <div id="team-panel" role="tabpanel" aria-labelledby={`team-tab-${teamTab}`} className="studio-team-body">
              {teamTab === "trajectory" ? (
                <TrajectoryPanel tasks={tasks} agents={agents} />
              ) : (
                <MemberList
                  agents={agents}
                  clis={clis}
                  statuses={agentStatuses}
                  onMention={(agent) => {
                    setTab("chat");
                    setMention(agent.name);
                  }}
                  onAdd={() => void openAgentForm({ kind: "new" })}
                  onEdit={(agent) => void openAgentForm({ kind: "edit", agent })}
                />
              )}
            </div>
          </aside>
        </div>

        {editing && (
          <Modal title={editingAgent ? `编辑成员：${editingAgent.name}` : "添加成员"} onClose={() => setEditing(null)}>
            <AgentForm
              key={editingAgent?.id ?? "new"}
              initial={editingAgent ?? undefined}
              permissionLocked={Boolean(editingAgent && (activeRunId || project.status !== "idle"))}
              clis={clis}
              templates={templates}
              takenNames={agents.filter((a) => a.id !== editingAgent?.id).map((a) => a.name)}
              forceLeader={editingAgent ? editingAgent.isLeader : agents.length === 0}
              submitLabel={editingAgent ? "保存" : "添加"}
              onSubmit={saveAgent}
              onCancel={() => setEditing(null)}
              draftContext={
                detail.folderExists && assistantReady
                  ? {
                      folderPath: project.folderPath,
                      projectName: project.name,
                      description: project.description,
                      rules: projectRules,
                      members: agents.map((agent) => ({
                        name: agent.name,
                        role: agent.role,
                        persona: agent.persona,
                      })),
                    }
                  : undefined
              }
              assistantUnavailableReason={
                !detail.folderExists
                  ? "项目文件夹不存在，无法起草"
                  : !assistantReady
                    ? "没有已安装且已接入的 CLI"
                    : undefined
              }
            />
            {editingAgent && !editingAgent.isLeader && (
              <div className="mt-4 border-t border-line pt-4">
                <Button variant="danger" onClick={() => removeAgent(editingAgent)}>
                  移除该成员
                </Button>
              </div>
            )}
          </Modal>
        )}
      </div>
    </AppShell>
  );
}
