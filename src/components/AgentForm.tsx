import { DshModelSelect } from "./DshModelSelect";
import { useEffect, useState } from "react";
import { api } from "../api";
import { defaultModelHint, memberModelOptions, selectableClis } from "../lib/cliConfig";
import { composePersona, emptyPersona, parsePersona } from "../lib/persona";
import { normalizeName } from "../lib/mentions";
import { keepEffortForCli, reasoningOptions, supportsReasoningEffort } from "../lib/reasoning";
import { validateAgent } from "../lib/wizard";
import type { AgentInput, CliConfig, CliInfo, DraftInput, PersonaTemplate } from "../types";
import { PersonaEditor } from "./PersonaEditor";
import { AssistantDraftDialog } from "./AssistantDraftDialog";
import { Button, Field, inputClass } from "./ui";
import { Select } from "./Select";

interface Props {
  /** 编辑已有成员时传入；新建时不传 */
  initial?: AgentInput;
  /** 编辑时，项目非空闲状态下锁定执行权限。 */
  permissionLocked?: boolean;
  clis: CliInfo[];
  templates: PersonaTemplate[];
  /** 同项目里其他成员的名字，用于查重 */
  takenNames: string[];
  /** 为 true 时"组长"锁定为选中：项目的第一个成员，或正在编辑现任组长 */
  forceLeader: boolean;
  submitLabel: string;
  /** 抛出的错误会显示在表单里 */
  onSubmit: (input: AgentInput) => void | Promise<void>;
  onCancel: () => void;
  draftContext?: Omit<DraftInput, "draftId" | "currentText" | "feedback" | "memberName" | "role" | "targetFilename">;
  assistantUnavailableReason?: string;
}

export function AgentForm({
  initial,
  permissionLocked = false,
  clis,
  templates,
  takenNames,
  forceLeader,
  submitLabel,
  onSubmit,
  onCancel,
  draftContext,
  assistantUnavailableReason,
}: Props) {
  const [name, setName] = useState(initial?.name ?? "");
  const [role, setRole] = useState(initial?.role ?? "");
  const [cli, setCli] = useState(initial?.cli ?? clis.find((c) => c.installed && c.supported)?.id ?? "");
  const [model, setModel] = useState(initial?.model ?? "");
  const [effort, setEffort] = useState(initial?.cli === "dsh" ? "" : initial?.reasoningEffort ?? "");
  const [permission, setPermission] = useState(initial?.executionPermission ?? "");
  const [isLeader, setIsLeader] = useState(forceLeader || (initial?.isLeader ?? false));
  const [persona, setPersona] = useState(initial?.persona ?? composePersona(emptyPersona()));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [draftOpen, setDraftOpen] = useState(false);

  // 应用管理里的开关与模型白名单；读不到时退回只看安装状态
  const [configs, setConfigs] = useState<CliConfig[]>([]);
  useEffect(() => { api.listCliConfigs().then(setConfigs).catch(() => setConfigs([])); }, []);
  useEffect(() => {
    if (configs.length === 0 || initial) return;
    const allowed = selectableClis(configs).map((c) => c.id);
    const next = allowed.includes(cli) ? cli : allowed[0] ?? "";
    if (next === cli) return;
    setCli(next);
    setModel("");
    setEffort((current) => keepEffortForCli(next, current));
    setPermission("");
  }, [configs, initial, cli]);
  const allowedIds = new Set(selectableClis(configs, initial?.cli).map((c) => c.id));
  const visibleClis = configs.length === 0 ? clis : clis.filter((c) => allowedIds.has(c.id));
  const cliConfig = configs.find((c) => c.id === cli);
  const selected = clis.find((c) => c.id === cli);
  const permissions = selected?.executionPermissions;
  const modelChoice = memberModelOptions(cliConfig, model);
  const closed = (id: string) => configs.find((c) => c.id === id)?.enabled === false;

  const template = templates.find((t) => t.role === role.trim());

  function changeRole(next: string) {
    setRole(next);
    const match = templates.find((t) => t.role === next.trim());
    const personaIsBlank = Object.values(parsePersona(persona)).every((text) => text === "");
    // 人设还没写过时，选中预设角色就自动带出模板
    if (match && personaIsBlank) setPersona(match.persona);
  }

  async function submit() {
    const input: AgentInput = {
      name: name.trim(),
      role: role.trim(),
      persona,
      cli,
      model: model.trim() || null,
      reasoningEffort: supportsReasoningEffort(cli) && effort ? effort : null,
      executionPermission: permission || null,
      isLeader,
    };
    const duplicate = takenNames.some((other) => normalizeName(other) === normalizeName(input.name));
    const problem = permissionLocked && (cli !== initial?.cli || permission !== (initial?.executionPermission ?? "")) ? "请在项目空闲时修改成员 CLI 或执行权限" :
      !cli ? "没有可选的 CLI（未启用或未安装），请先在系统设置的应用管理中启用" :
      !initial && closed(cli) ? "该 CLI 已在应用管理中关闭，请先在系统设置里启用" : duplicate ? `成员名「${input.name}」已存在` :
      !selected?.supported ? "该 CLI 尚未接入" : !selected.installed ? "请先安装所选 CLI" : validateAgent(input, takenNames);
    if (problem) {
      setError(problem);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await onSubmit(input);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="space-y-4">
      <div className="grid grid-cols-2 gap-3">
        <Field label="名字">
          <input
            className={inputClass}
            value={name}
            placeholder="例如：老王"
            onChange={(e) => setName(e.target.value)}
          />
        </Field>
        <Field label="角色">
          <input
            className={inputClass}
            list="preset-roles"
            value={role}
            placeholder="选择预设或自行填写"
            onChange={(e) => changeRole(e.target.value)}
          />
          <datalist id="preset-roles">
            {templates.map((t) => (
              <option key={t.role} value={t.role} />
            ))}
          </datalist>
        </Field>
        <Field label="CLI">
          <Select aria-label="CLI" value={cli} disabled={permissionLocked || busy} onChange={(value) => {
            if (value === cli) return;
            setCli(value);
            setModel("");
            setEffort((current) => keepEffortForCli(value, current));
            setPermission(value === initial?.cli ? initial.executionPermission ?? "" : "");
          }}
            options={[{ value: "", label: "请选择", disabled: true }, ...visibleClis.map((c) => ({ value: c.id, disabled: !c.installed || !c.supported,
              label: `${c.label}${!c.supported ? "（尚未接入）" : c.installed ? (c.version ? `（${c.version}）` : "") : "（未安装）"}${closed(c.id) ? "（已关闭）" : ""}` }))]} />
        </Field>
        <Field label={cli === "dsh" ? "模型（读取 Dsh 已配置的模型）" : modelChoice.restricted ? "模型（从应用管理已启用的模型中选）" : "模型（可选，留空用 CLI 默认）"}>
          {cli === "dsh" ? <DshModelSelect value={model} onChange={setModel} config={cliConfig} /> :
            modelChoice.restricted ? (
              <Select aria-label="模型" value={model} onChange={setModel}
                options={[{ value: "", label: defaultModelHint(cliConfig) }, ...modelChoice.options]} />
            ) : <input className={inputClass} value={model} onChange={(e) => setModel(e.target.value)} />}
        </Field>
        {supportsReasoningEffort(cli) && <Field label="推理强度（可选，留空沿用 CLI 配置）">
          <Select aria-label="推理强度" value={effort} onChange={setEffort}
            options={reasoningOptions(cli, effort).map(({ value, label }) => ({ value, label }))} />
        </Field>}
      </div>

      {permissions && <Field label="执行权限">
        <Select aria-label="执行权限" value={permission} onChange={setPermission} disabled={permissionLocked || busy}
          options={[
            { value: "", label: `默认（${permissions.defaultExecute}）` },
            ...permissions.options.map(({ id, label }) => ({ value: id, label: `${label}（${id}）` })),
            ...(permission && !permissions.options.some(({ id }) => id === permission)
              ? [{ value: permission, label: `${permission}（当前不可用）`, disabled: true }] : []),
          ]} />
        <p className="text-xs text-slate-500">
          {permissionLocked ? "项目空闲时可修改 CLI 和执行权限。" : "执行权限用于派工和点名，实际档位变更后会开启新会话。"}
          讨论使用 {permissions.discuss}。项目预授权或单次授权可以为受阻任务提权。
        </p>
      </Field>}

      <label className="flex items-center gap-2 text-sm">
        <input
          type="checkbox"
          checked={isLeader}
          disabled={forceLeader}
          onChange={(e) => setIsLeader(e.target.checked)}
        />
        设为组长
        <span className="text-xs text-slate-500">
          {forceLeader ? "每个项目必须有一个组长" : "勾选后原组长自动卸任"}
        </span>
      </label>

      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <span className="text-sm font-medium">人设</span>
          <div className="flex gap-2">
            <Button disabled={!draftContext} title={assistantUnavailableReason} onClick={() => setDraftOpen(true)}>让助理起草</Button>
            <Button disabled={!template} onClick={() => template && setPersona(template.persona)}>
              套用「{role.trim() || "预设角色"}」模板
            </Button>
          </div>
        </div>
        <PersonaEditor value={persona} onChange={setPersona} />
        <p className="text-xs text-slate-500">
          人设边界通过提示词表达。工具权限由 CLI 档位和原生配置决定，写入范围可能超出项目目录。
        </p>
      </div>

      {error && <p className="text-sm text-red-400">{error}</p>}
      {!draftContext && <p className="text-xs text-slate-500">{assistantUnavailableReason ?? "选择项目文件夹并检测到已接入的 CLI 后可起草"}</p>}

      <div className="flex justify-end gap-2">
        <Button onClick={onCancel}>取消</Button>
        <Button variant="primary" disabled={busy} onClick={submit}>
          {submitLabel}
        </Button>
      </div>
      {draftOpen && draftContext && <AssistantDraftDialog kind="persona" context={{ ...draftContext, memberName: name.trim() || null,
        role: role.trim() || null, targetFilename: null }} initialDraft={persona} onGenerate={api.draftPersona}
        onRewrite={(feedback, draftId, currentText) => api.draftPersona({ ...draftContext, memberName: name.trim() || null,
          role: role.trim() || null, targetFilename: null, draftId, currentText, feedback })}
        onAccept={(draft) => { if (typeof draft === "string") setPersona(draft); }} onCancel={() => setDraftOpen(false)} />}
    </div>
  );
}
