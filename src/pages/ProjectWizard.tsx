import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { Link, useNavigate } from "react-router";
import { api } from "../api";
import { AppShell } from "../components/layout/AppShell";
import { AgentForm } from "../components/project/AgentForm";
import { AssistantDraftDialog } from "../components/project/AssistantDraftDialog";
import { RulesEditor } from "../components/project/RulesEditor";
import { Icon } from "../components/ui/Icon";
import { Button, Field, buttonClass, inputClass } from "../components/ui/ui";
import { closedCliIds } from "../lib/cliConfig";
import { assistantAvailable } from "../lib/clis";
import { MAX_AGENTS, validateBasics, validateRules, validateTeam } from "../lib/wizard";
import type { AgentInput, CliInfo, DraftInput, DraftMember, PersonaTemplate, RuleDoc } from "../types";

const STEPS = [
  { title: "基本信息", note: "名称、目录与目标" },
  { title: "规章制度", note: "团队共同工作规范" },
  { title: "组建团队", note: "配置组长与分工成员" },
];

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

  const draftContext = (
    extra: Pick<DraftInput, "memberName" | "role" | "targetFilename">,
  ): Omit<DraftInput, "draftId" | "currentText" | "feedback"> => ({
    ...extra,
    folderPath,
    projectName: name.trim(),
    description: description.trim(),
    rules,
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
    <AppShell>
      <main className="studio-page max-w-4xl">
        <header className="studio-page-head">
          <div>
            <p className="studio-kicker"><Icon name="plus" width="14" height="14" />工作空间 / 新建项目</p>
            <h1 className="studio-page-title">组建一支新团队</h1>
            <p className="studio-subtitle mt-1.5">完成三步设置后才会写入本地记录与规章文件，中途取消不留痕迹</p>
          </div>
          <Link to="/" className={buttonClass("ghost")}>取消</Link>
        </header>

        <ol className="mb-6 grid grid-cols-3 gap-3 text-sm">
          {STEPS.map((item, i) => {
            const done = i < step;
            const active = i === step;
            return (
              <li
                key={item.title}
                aria-current={active ? "step" : undefined}
                className={`flex items-center gap-3 rounded-xl border px-4 py-3 ${
                  active
                    ? "border-accent bg-surface text-ink shadow-[0_1px_2px_rgba(24,25,23,0.04)]"
                    : done
                      ? "border-accent-border bg-accent-soft/50 text-ink"
                      : "border-line bg-surface/60 text-ink-muted"
                }`}
              >
                <span
                  className={`grid size-6 shrink-0 place-items-center rounded-full text-xs font-medium tabular-nums ${
                    active
                      ? "bg-accent text-white"
                      : done
                        ? "bg-accent-soft text-accent"
                        : "bg-surface-subtle text-ink-muted"
                  }`}
                >
                  {done ? <Icon name="check" width="13" height="13" /> : i + 1}
                </span>
                <span className="min-w-0">
                  <span className="block truncate text-[13px] font-medium">{item.title}</span>
                  <span className="block truncate text-[11px] text-ink-muted">{item.note}</span>
                </span>
              </li>
            );
          })}
        </ol>

        <section className="rounded-xl border border-line bg-surface p-6 shadow-[0_1px_2px_rgba(24,25,23,0.03)]">
          {step === 0 && (
            <div className="space-y-4">
              <Field label="项目名称">
                <input
                  className={inputClass}
                  placeholder="例如：ESB 接口平台"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                />
              </Field>
              <Field label="项目文件夹" hint="Agent 将在这个本地文件夹里读取和修改工作文件">
                <div className="flex gap-2">
                  <input
                    className={`${inputClass} font-mono text-xs`}
                    aria-label="项目文件夹"
                    readOnly
                    value={folderPath}
                    placeholder="点击右侧按钮选择目录"
                  />
                  <Button onClick={pickFolder}>
                    <Icon name="folder" width="15" height="15" />选择…
                  </Button>
                </div>
              </Field>
              <Field label="项目描述" hint="写清目标、受众与核心约束，会写入开场上下文供全体成员参考">
                <textarea
                  className={inputClass}
                  rows={5}
                  placeholder="描述项目要交付什么、使用什么技术或遵循哪些边界"
                  value={description}
                  onChange={(e) => setDescription(e.target.value)}
                />
              </Field>
            </div>
          )}

          {step === 1 && (
            <div className="space-y-4">
              <div className="flex flex-wrap items-center justify-between gap-3 border-b border-line pb-3.5">
                <p className="text-[13px] text-ink-secondary">
                  规章是全体成员共同遵守的规范，保存至项目目录 <code className="rounded bg-surface-subtle px-1.5 py-0.5 font-mono text-xs text-ink">.imates/rules/</code>，至少保留一份。
                </p>
                <div className="flex items-center gap-2">
                  {!assistantReady && <span className="text-xs text-ink-muted">{assistantReason}</span>}
                  <Button
                    disabled={!assistantReady || !folderPath.trim() || !name.trim()}
                    title={!folderPath.trim() ? "先选择项目文件夹" : !name.trim() ? "先填写项目名称" : assistantReason}
                    onClick={() => setRulesDraftOpen(true)}
                  >
                    让助理起草规章
                  </Button>
                </div>
              </div>
              <RulesEditor rules={rules} templates={ruleTemplates} onChange={setRules} />
            </div>
          )}

          {step === 2 && (
            <div className="space-y-4">
              {agents.length > 0 && (
                <ul className="space-y-2">
                  {agents.map((agent, i) => (
                    <li
                      key={agent.name}
                      className="flex items-center justify-between gap-3 rounded-lg border border-line bg-surface-subtle/50 px-3.5 py-2.5 text-sm"
                    >
                      <span className="min-w-0">
                        <span className="font-medium text-ink">{agent.name}</span>
                        <span className="ml-2 text-xs text-ink-secondary">
                          {agent.role} · {clis.find((c) => c.id === agent.cli)?.label ?? agent.cli}
                        </span>
                        {agent.isLeader && (
                          <span className="ml-2 rounded-md border border-accent-border bg-accent-soft px-1.5 py-0.5 text-[11px] font-medium text-accent">
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
              )}

              {adding ? (
                <div className="rounded-xl border border-line bg-paper p-5">
                  <AgentForm
                    clis={clis}
                    templates={personaTemplates}
                    takenNames={agents.map((a) => a.name)}
                    forceLeader={agents.length === 0}
                    submitLabel="添加"
                    onSubmit={addAgent}
                    onCancel={() => setAdding(false)}
                    draftContext={
                      folderPath.trim() && name.trim() && assistantReady
                        ? draftContext({ memberName: null, role: null, targetFilename: null })
                        : undefined
                    }
                    assistantUnavailableReason={assistantReason}
                  />
                </div>
              ) : (
                <Button disabled={agents.length >= MAX_AGENTS} onClick={() => setAdding(true)}>
                  <Icon name="plus" width="15" height="15" />
                  添加成员（{agents.length}/{MAX_AGENTS}）
                </Button>
              )}
            </div>
          )}
        </section>

        {error && (
          <p role="alert" className="mt-4 rounded-lg border border-danger-border bg-danger-soft px-3.5 py-2.5 text-sm text-danger">
            {error}
          </p>
        )}

        {rulesDraftOpen && (
          <AssistantDraftDialog
            kind="rules"
            context={draftContext({ memberName: null, role: null, targetFilename: null })}
            initialDraft={rules}
            onGenerate={api.draftRules}
            onRewrite={(feedback, draftId, currentText) =>
              api.draftRules({
                ...draftContext({ memberName: null, role: null, targetFilename: null }),
                draftId,
                currentText,
                feedback,
              })
            }
            onAccept={(draft) => {
              if (Array.isArray(draft)) setRules(draft);
            }}
            onCancel={() => setRulesDraftOpen(false)}
          />
        )}

        <footer className="mt-5 flex items-center justify-between">
          <Button disabled={step === 0} onClick={goBack}>
            上一步
          </Button>
          {step < 2 ? (
            <Button variant="primary" onClick={goNext}>
              下一步<Icon name="arrow" width="15" height="15" />
            </Button>
          ) : (
            <Button variant="primary" disabled={saving || adding} onClick={finish}>
              {saving ? "正在创建…" : "完成创建"}
            </Button>
          )}
        </footer>
      </main>
    </AppShell>
  );
}
