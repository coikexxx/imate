import { assistantAvailable } from "../lib/clis";
import { closedCliIds } from "../lib/cliConfig";
import { confirm, open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useState } from "react";
import { Link, useParams } from "react-router";
import { useStore } from "zustand";
import { ChatPanel } from "../components/ChatPanel";
import { projectStore } from "../stores/project";
import { api } from "../api";
import { AgentForm } from "../components/AgentForm";
import { MemberList } from "../components/MemberList";
import { RulesPanel } from "../components/RulesPanel";
import { FileTree } from "../components/FileTree";
import { ProjectUsagePanel } from "../components/ProjectUsagePanel";
import { OfficeCanvas } from "../office/OfficeCanvas";
import { Button, Modal } from "../components/ui";
import { AppShell } from "../components/AppShell";
import { Icon } from "../components/Icon";
import {
  STATUS_LABELS,
  type Agent,
  type AgentInput,
  type CliInfo,
  type PersonaTemplate,
  type ProjectDetail,
  type RuleDoc,
} from "../types";

const EMPTY_AGENT_STATUSES: Record<string, import("../types").AgentStatus> = {};
const EMPTY_FILES: string[] = [];
const TAB_LABELS = { chat: "团队讨论", files: "项目文件", rules: "规章制度", usage: "用量" } as const;

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
  const [usageTick, setUsageTick] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [mention, setMention] = useState<string | undefined>();
  const activeRunId = useStore(projectStore, (state) => state.projectId === id ? state.activeRunId : null);
  const agentStatuses = useStore(projectStore, (state) => state.projectId === id ? state.agentStatuses : EMPTY_AGENT_STATUSES);
  const changedFiles = useStore(projectStore, (state) => state.projectId === id ? state.changedFiles : EMPTY_FILES);
  const filesTick = useStore(projectStore, (state) => state.projectId === id ? state.filesTick : 0);
  const rulesTick = useStore(projectStore, (state) => state.projectId === id ? state.rulesTick : 0);
  const consumeMention = useCallback(() => setMention(undefined), []);

  const reload = useCallback(async () => {
    setDetail(await api.getProject(id));
  }, [id]);

  // 回合结束后刷新成员状态，并让用量区域重读一次。
  const refreshAfterRun = useCallback(() => { setUsageTick((n) => n + 1); reload().catch((e) => setError(String(e))); }, [reload]);

  useEffect(() => {
    reload().catch((e) => setError(String(e)));
    api.listClis().then(setClis);
    // 应用管理里关闭的 CLI 不能当助理；读不到时当作没有关闭的
    api.listCliConfigs().then((configs) => setClosedClis(closedCliIds(configs))).catch(() => setClosedClis(new Set()));
    api.listPersonaTemplates().then(setTemplates);
  }, [reload]);

  useEffect(() => {
    let disposed = false;
    api.watchProject(id)
      .then((changed) => { if (!disposed) projectStore.getState().setChangedFiles(id, changed); })
      .catch(() => { /* 文件夹不存在时由上方的红条提示，这里不重复报错 */ });
    return () => { disposed = true; };
  }, [id, detail?.project.folderPath]);

  useEffect(() => {
    function refreshProjectPage(event: KeyboardEvent) {
      if (!(event.metaKey || event.ctrlKey) || event.altKey || event.shiftKey || event.isComposing
        || event.key.toLowerCase() !== "r") return;
      event.preventDefault();
      if (!event.repeat) window.location.reload();
    }
    window.addEventListener("keydown", refreshProjectPage);
    return () => window.removeEventListener("keydown", refreshProjectPage);
  }, []);

  if (error && !detail) {
    return (
      <main className="p-8">
        <Link to="/" className="text-sm text-slate-400 hover:text-slate-100">
          ← 返回
        </Link>
        <p className="mt-4 text-sm text-red-400">{error}</p>
      </main>
    );
  }
  if (!detail) return <main className="p-8 text-sm text-slate-400">加载中…</main>;

  const { project, agents } = detail;

  // AgentForm 会捕获这里抛出的错误并显示在表单里
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
    try { setProjectRules(await api.listRules(id)); setEditing(target); }
    catch (failure) { setError(String(failure)); }
  }

  return (
    <AppShell projectName={project.name}><div className="studio-project-layout">
      <header className="studio-project-header">
        <div className="min-w-0"><p className="studio-kicker"><Link to="/" className="hover:text-sky-300">项目总览</Link><span>/</span><span>项目工作台</span></p><h1 className="mt-2 truncate text-xl font-medium">{project.name}</h1></div>
        <div className="min-w-0 text-right"><span className="inline-flex items-center gap-2 rounded-full border border-slate-800 bg-slate-900 px-3 py-1 text-xs text-slate-400"><span className={`size-1.5 rounded-full ${activeRunId ? "bg-sky-400" : "bg-slate-500"}`} />{activeRunId ? "执行中" : STATUS_LABELS[project.status]}</span><p className="mt-2 max-w-64 truncate font-mono text-[11px] text-slate-500" title={project.folderPath}>{project.folderPath}</p></div>
      </header>

      {!detail.folderExists && (
        <div className="flex items-center justify-between bg-red-950 px-4 py-2 text-sm text-red-200">
          <span>项目文件夹不存在：{project.folderPath}</span>
          <Button onClick={relocate}>重新选择文件夹</Button>
        </div>
      )}
      {error && <p className="px-4 py-2 text-sm text-red-400">{error}</p>}

      <div className="studio-project-body">
        <section className="studio-workspace">
          <div className="studio-work-tabs" role="tablist" aria-label="项目工作区">
            {(["chat", "files", "rules", "usage"] as const).map((key) => <button key={key} type="button" role="tab" id={`project-tab-${key}`} aria-controls={`project-panel-${key}`} aria-selected={tab === key} onClick={() => setTab(key)}><Icon name={key} />{TAB_LABELS[key]}{key === "files" && changedFiles.length > 0 && <span className="font-mono text-xs text-amber-200">{changedFiles.length}</span>}</button>)}
          </div>
          {/* 群聊始终挂载，切换视图时继续接收运行事件并保留输入。 */}
          <div id="project-panel-chat" role="tabpanel" aria-labelledby="project-tab-chat" hidden={tab !== "chat"} className={tab === "chat" ? "studio-work-panel" : "hidden"}>
            <ChatPanel projectId={id} agents={agents} folderExists={detail.folderExists} mention={mention} onMentionUsed={consumeMention} onFinished={refreshAfterRun} />
          </div>
          <div id="project-panel-rules" role="tabpanel" aria-labelledby="project-tab-rules" hidden={tab !== "rules"} className={tab === "rules" ? "studio-work-panel" : "hidden"}>
            <div className="border-b border-slate-800 px-4 py-3 text-xs text-slate-500">团队共同遵守的工作约定 · 修改后从下一回合生效</div>
            <div className="min-h-0 flex-1"><RulesPanel key={`${detail.folderExists}:${project.folderPath}`} projectId={id} project={project} agents={agents} clis={clis} closedCliIds={closedClis} refreshTick={rulesTick} /></div>
          </div>
          {tab === "files" && <div id="project-panel-files" role="tabpanel" aria-labelledby="project-tab-files" className="studio-work-panel">
            <div className="border-b border-slate-800 px-4 py-3 text-xs text-slate-500">点击文件，在系统默认程序中打开 · 改动不区分修改者</div>
            <div className="min-h-0 flex-1">{detail.folderExists ? <FileTree key={project.folderPath} projectId={id} changed={changedFiles} tick={filesTick} /> : <p className="p-4 text-sm text-slate-500">项目文件夹不存在，无法列出文件。</p>}</div>
          </div>}
          {tab === "usage" && <div id="project-panel-usage" role="tabpanel" aria-labelledby="project-tab-usage" className="studio-work-panel">
            <div className="border-b border-slate-800 px-4 py-3 text-xs text-slate-500">按成员与项目合计已结束回合的用量 · 每次回合结束后刷新</div>
            <div className="min-h-0 flex-1 overflow-y-auto"><ProjectUsagePanel projectId={id} clis={clis} refreshTick={usageTick} /></div>
          </div>}
        </section>
        <aside className="studio-team-panel">
          <div className="studio-team-title"><span className="flex items-center gap-2"><Icon name="team" />团队现场</span><span className="font-mono text-[11px]">{agents.length} / 8</span></div>
          <div className="studio-office"><OfficeCanvas agents={agents} statuses={agentStatuses} /></div>
          <MemberList agents={agents} clis={clis} statuses={agentStatuses} onMention={(agent) => { setTab("chat"); setMention(agent.name); }} onAdd={() => void openAgentForm({ kind: "new" })} onEdit={(agent) => void openAgentForm({ kind: "edit", agent })} />
        </aside>
      </div>

      {editing && (
        <Modal
          title={editingAgent ? `编辑成员：${editingAgent.name}` : "添加成员"}
          onClose={() => setEditing(null)}
        >
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
            draftContext={detail.folderExists && assistantReady ? {
              folderPath: project.folderPath, projectName: project.name, description: project.description,
              rules: projectRules, members: agents.map((agent) => ({ name: agent.name, role: agent.role, persona: agent.persona })),
            } : undefined}
            assistantUnavailableReason={!detail.folderExists ? "项目文件夹不存在，无法起草" : !assistantReady ? "没有已安装且已接入的 CLI" : undefined}
          />
          {editingAgent && !editingAgent.isLeader && (
            <div className="mt-4 border-t border-slate-800 pt-4">
              <Button variant="danger" onClick={() => removeAgent(editingAgent)}>
                移除该成员
              </Button>
            </div>
          )}
        </Modal>
      )}
    </div></AppShell>
  );
}
