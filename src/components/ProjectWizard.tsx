import { assistantAvailable } from "../lib/clis";
import { closedCliIds } from "../lib/cliConfig";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { Link, useNavigate } from "react-router";
import { api } from "../api";
import { MAX_AGENTS, validateBasics, validateRules, validateTeam } from "../lib/wizard";
import type { AgentInput, CliInfo, DraftInput, DraftMember, PersonaTemplate, RuleDoc } from "../types";
import { AgentForm } from "./AgentForm";
import { AssistantDraftDialog } from "./AssistantDraftDialog";
import { RulesEditor } from "./RulesEditor";
import { Button, Field, inputClass } from "./ui";
import { AppShell } from "./AppShell";
import { Icon } from "./Icon";

const STEPS = ["基本信息", "规章制度", "组建团队"];

/** 新建项目三步向导。全部内容只在内存里，点"完成创建"才一次性提交给后端。 */
export default function ProjectWizard() {
  const navigate = useNavigate();
  const [step, setStep] = useState(0);
  const [name, setName] = useState("");
  const [folderPath, setFolderPath] = useState("");
  const [description, setDescription] = useState("");
  const [rules, setRules] = useState<RuleDoc[]>([]);
  const [agents, setAgents] = useState<AgentInput[]>([]);
  const [adding, setAdding] = useState(false);
  const [ruleTemplates, setRuleTemplates] = useState<RuleDoc[]>([]);
  const [personaTemplates, setPersonaTemplates] = useState<PersonaTemplate[]>([]);
  const [clis, setClis] = useState<CliInfo[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [rulesDraftOpen, setRulesDraftOpen] = useState(false);
  const [closedClis, setClosedClis] = useState<ReadonlySet<string>>(new Set());
  const assistantReady = assistantAvailable(clis, closedClis);
  const assistantReason = assistantReady ? undefined : "没有已安装且已接入的 CLI";
  const draftContext = (extra: Pick<DraftInput, "memberName" | "role" | "targetFilename">): Omit<DraftInput, "draftId" | "currentText" | "feedback"> => ({
    ...extra, folderPath, projectName: name.trim(), description: description.trim(), rules,
    members: agents.map((agent): DraftMember => ({ name: agent.name, role: agent.role, persona: agent.persona })),
  });

  useEffect(() => {
    api.listRuleTemplates().then((templates) => {
      setRuleTemplates(templates);
      // 协作规范每个项目都需要，默认带上；用户可以改，也可以换成别的
      setRules((current) =>
        current.length > 0 ? current : templates.filter((t) => t.filename === "协作规范.md"),
      );
    });
    api.listPersonaTemplates().then(setPersonaTemplates);
    api.listClis().then(setClis);
    // 应用管理里关闭的 CLI 不能当助理；读不到时当作没有关闭的
    api.listCliConfigs().then((configs) => setClosedClis(closedCliIds(configs))).catch(() => setClosedClis(new Set()));
  }, []);

  async function pickFolder() {
    const picked = await open({ directory: true, multiple: false, title: "选择项目文件夹" });
    if (typeof picked === "string") setFolderPath(picked);
  }

  function goNext() {
    const problem = step === 0 ? validateBasics(name, folderPath) : validateRules(rules);
    if (problem) {
      setError(problem);
      return;
    }
    setError(null);
    setStep(step + 1);
  }

  function goBack() {
    setError(null);
    setAdding(false);
    setStep(step - 1);
  }

  function addAgent(input: AgentInput) {
    // 新成员被设为组长时，原组长卸任
    const others = input.isLeader ? agents.map((a) => ({ ...a, isLeader: false })) : agents;
    setAgents([...others, input]);
    setAdding(false);
    setError(null);
  }

  function removeAgent(index: number) {
    const rest = agents.filter((_, i) => i !== index);
    // 删掉的是组长时，让剩下的第一个人接任
    if (agents[index].isLeader && rest.length > 0) rest[0] = { ...rest[0], isLeader: true };
    setAgents(rest);
  }

  async function finish() {
    const problem = validateTeam(agents);
    if (problem) {
      setError(problem);
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const project = await api.createProject({
        name: name.trim(),
        description: description.trim(),
        folderPath,
        rules,
        agents,
      });
      navigate(`/project/${project.id}`);
    } catch (e) {
      setError(String(e));
      setSaving(false);
    }
  }

  return (
    <AppShell><main className="studio-page flex h-full max-w-5xl flex-col">
      <header className="mb-8 flex items-center justify-between">
        <div><p className="studio-kicker"><Icon name="plus" />工作空间 / 新建项目</p><h1 className="studio-page-title">组建一支新团队</h1></div>
        <Link to="/" className="text-sm text-slate-400 hover:text-slate-100">
          取消
        </Link>
      </header>

      <ol className="mb-6 grid grid-cols-3 gap-3 text-sm">
        {STEPS.map((label, i) => (
          <li
            key={label}
            aria-current={i === step ? "step" : undefined}
            className={`flex items-center gap-3 rounded-lg border px-4 py-3 ${
              i === step ? "border-sky-600 bg-white text-sky-300" : "border-slate-800 text-slate-500"
            }`}
          >
            {i + 1}. {label}
          </li>
        ))}
      </ol>

      <section className="min-h-0 flex-1 overflow-y-auto rounded-xl border border-slate-800 bg-white p-6">
        {step === 0 && (
          <div className="space-y-4">
            <Field label="项目名称">
              <input
                className={inputClass}
                value={name}
                onChange={(e) => setName(e.target.value)}
              />
            </Field>
            <div className="space-y-1">
              <span className="text-xs text-slate-400">项目文件夹</span>
              <div className="flex gap-2">
                <input
                  className={inputClass}
                  aria-label="项目文件夹"
                  readOnly
                  value={folderPath}
                  placeholder="Agent 将在这个文件夹里工作"
                />
                <Button onClick={pickFolder}>选择…</Button>
              </div>
            </div>
            <Field label="项目描述（要做什么、给谁用、有什么约束）">
              <textarea
                className={inputClass}
                rows={5}
                value={description}
                onChange={(e) => setDescription(e.target.value)}
              />
            </Field>
          </div>
        )}

        {step === 1 && (
          <div className="space-y-3">
            <p className="text-sm text-slate-400">
              规章是全体成员都要遵守的规范，会保存到项目文件夹的 .imates/rules/ 里。至少保留一份。
            </p>
            <div className="flex items-center gap-2">
              <Button disabled={!assistantReady || !folderPath.trim() || !name.trim()} title={!folderPath.trim() ? "先选择项目文件夹" : !name.trim() ? "先填写项目名称" : assistantReason}
                onClick={() => setRulesDraftOpen(true)}>让助理起草规章</Button>
              {!assistantReady && <span className="text-xs text-slate-500">{assistantReason}</span>}
            </div>
            <RulesEditor rules={rules} templates={ruleTemplates} onChange={setRules} />
          </div>
        )}

        {step === 2 && (
          <div className="space-y-3">
            <ul className="space-y-2">
              {agents.map((agent, i) => (
                <li
                  key={agent.name}
                  className="flex items-center justify-between rounded border border-slate-700 px-3 py-2 text-sm"
                >
                  <span>
                    <span className="font-medium">{agent.name}</span>
                    <span className="ml-2 text-slate-400">
                      {agent.role} · {clis.find((c) => c.id === agent.cli)?.label ?? agent.cli}
                    </span>
                    {agent.isLeader && (
                      <span className="ml-2 rounded bg-amber-900/60 px-1.5 text-xs text-amber-200">
                        组长
                      </span>
                    )}
                  </span>
                  <Button variant="danger" onClick={() => removeAgent(i)}>
                    移除
                  </Button>
                </li>
              ))}
            </ul>

            {adding ? (
              <div className="rounded border border-slate-700 p-4">
                <AgentForm
                  clis={clis}
                  templates={personaTemplates}
                  takenNames={agents.map((a) => a.name)}
                  forceLeader={agents.length === 0}
                  submitLabel="添加"
                  onSubmit={addAgent}
                  onCancel={() => setAdding(false)}
                  draftContext={folderPath.trim() && name.trim() && assistantReady ? draftContext({ memberName: null, role: null, targetFilename: null }) : undefined}
                  assistantUnavailableReason={assistantReason}
                />
              </div>
            ) : (
              <Button disabled={agents.length >= MAX_AGENTS} onClick={() => setAdding(true)}>
                ＋ 添加成员（{agents.length}/{MAX_AGENTS}）
              </Button>
            )}
          </div>
        )}
      </section>

      {error && <p className="mt-4 text-sm text-red-400">{error}</p>}

      {rulesDraftOpen && <AssistantDraftDialog kind="rules" context={draftContext({ memberName: null, role: null, targetFilename: null })}
        initialDraft={rules} onGenerate={api.draftRules}
        onRewrite={(feedback, draftId, currentText) => api.draftRules({ ...draftContext({ memberName: null, role: null, targetFilename: null }), draftId, currentText, feedback })}
        onAccept={(draft) => { if (Array.isArray(draft)) setRules(draft); }} onCancel={() => setRulesDraftOpen(false)} />}

      <footer className="mt-6 flex justify-between">
        <Button disabled={step === 0} onClick={goBack}>
          上一步
        </Button>
        {step < 2 ? (
          <Button variant="primary" onClick={goNext}>
            下一步
          </Button>
        ) : (
          <Button variant="primary" disabled={saving || adding} onClick={finish}>
            完成创建
          </Button>
        )}
      </footer>
    </main></AppShell>
  );
}
