import { assistantAvailable } from "../lib/clis";
import { confirm, open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useState } from "react";
import { Link, useParams } from "react-router";
import { useStore } from "zustand";
import { ChatPanel } from "../components/ChatPanel";
import { projectStore } from "../stores/project";
import { api } from "../api";
import { AgentForm } from "../components/AgentForm";
import { MemberList } from "../components/MemberList";
import { ProxySettingsButton } from "../components/ProxySettings";
import { DshSettingsButton } from "../components/DshSettings";
import { RulesPanel } from "../components/RulesPanel";
import { FileTree } from "../components/FileTree";
import { OfficeCanvas } from "../office/OfficeCanvas";
import { Button, Modal } from "../components/ui";
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

type Editing = { kind: "new" } | { kind: "edit"; agent: Agent } | null;

export default function Project() {
  const { id = "" } = useParams();
  const [detail, setDetail] = useState<ProjectDetail | null>(null);
  const [clis, setClis] = useState<CliInfo[]>([]);
  const [templates, setTemplates] = useState<PersonaTemplate[]>([]);
  const [projectRules, setProjectRules] = useState<RuleDoc[]>([]);
  const [editing, setEditing] = useState<Editing>(null);
  const [tab, setTab] = useState<"files" | "rules">("rules");
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

  const refreshAfterRun = useCallback(() => { reload().catch((e) => setError(String(e))); }, [reload]);

  useEffect(() => {
    reload().catch((e) => setError(String(e)));
    api.listClis().then(setClis);
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
  const assistantReady = assistantAvailable(clis);
  async function openAgentForm(target: Editing) {
    try { setProjectRules(await api.listRules(id)); setEditing(target); }
    catch (failure) { setError(String(failure)); }
  }

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-4 border-b border-slate-800 px-4 py-3">
        <Link to="/" className="text-sm text-slate-400 hover:text-slate-100">
          ← 返回
        </Link>
        <h1 className="text-base font-semibold">{project.name}</h1>
        <span className="rounded bg-slate-800 px-2 py-0.5 text-xs">
          {activeRunId ? "执行中" : STATUS_LABELS[project.status]}
        </span>
        <span className="flex-1 truncate text-xs text-slate-500" title={project.folderPath}>
          {project.folderPath}
        </span>
        <ProxySettingsButton />
        <DshSettingsButton />
      </header>

      {!detail.folderExists && (
        <div className="flex items-center justify-between bg-red-950 px-4 py-2 text-sm text-red-200">
          <span>项目文件夹不存在：{project.folderPath}</span>
          <Button onClick={relocate}>重新选择文件夹</Button>
        </div>
      )}
      {error && <p className="px-4 py-2 text-sm text-red-400">{error}</p>}

      <div className="grid min-h-0 flex-1 grid-cols-[240px_minmax(0,1fr)_minmax(360px,32%)]">
        <MemberList
          agents={agents}
          clis={clis}
          statuses={agentStatuses}
          onMention={(agent) => setMention(agent.name)}
          onAdd={() => void openAgentForm({ kind: "new" })}
          onEdit={(agent) => void openAgentForm({ kind: "edit", agent })}
        />

        {/* 中栏：办公室占 65%，下方是文件与规章 */}
        <section className="grid min-h-0 grid-rows-[minmax(0,65fr)_minmax(0,35fr)] gap-3 p-3">
          <OfficeCanvas agents={agents} statuses={agentStatuses} />
          <div className="flex min-h-0 flex-col overflow-hidden rounded border border-slate-800 bg-slate-950">
            <div className="flex items-center gap-1 border-b border-slate-800 px-2 py-1.5 text-sm" role="tablist">
              {(["files", "rules"] as const).map((key) => (
                <button
                  key={key}
                  type="button"
                  role="tab"
                  aria-selected={tab === key}
                  onClick={() => setTab(key)}
                  className={`rounded px-3 py-1 ${
                    tab === key ? "bg-sky-900/60 text-sky-100" : "text-slate-400 hover:bg-slate-800 hover:text-slate-200"
                  }`}
                >
                  {key === "files" ? "文件" : "规章"}
                  {key === "files" && changedFiles.length > 0 && (
                    <span className="ml-1.5 rounded-full bg-amber-500/20 px-1.5 text-xs text-amber-300">{changedFiles.length}</span>
                  )}
                </button>
              ))}
              <span className="ml-auto truncate pl-2 text-xs text-slate-500">
                {tab === "files" ? "只读 · 点击文件用系统默认程序打开" : "保存在项目文件夹的 .imates/rules/"}
              </span>
            </div>
            <div className="min-h-0 flex-1">
              {tab === "rules" ? (
                // 文件夹重新选定后路径会变，用 key 让规章面板重新读取，不必来回切页签
                <RulesPanel key={`${detail.folderExists}:${project.folderPath}`} projectId={id} project={project} agents={agents} clis={clis} refreshTick={rulesTick} />
              ) : detail.folderExists ? (
                <FileTree key={project.folderPath} projectId={id} changed={changedFiles} tick={filesTick} />
              ) : (
                <p className="p-3 text-sm text-slate-500">项目文件夹不存在，无法列出文件。</p>
              )}
            </div>
          </div>
        </section>

        {/* 右栏：群聊占满整列高度 */}
        <aside className="flex min-h-0 flex-col border-l border-slate-800 p-3">
          <ChatPanel projectId={id} agents={agents} folderExists={detail.folderExists} mention={mention} onMentionUsed={consumeMention} onFinished={refreshAfterRun} />
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
    </div>
  );
}
