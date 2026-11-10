import { useState } from "react";
import { api } from "../api";
import { composePersona, emptyPersona, parsePersona } from "../lib/persona";
import { normalizeName } from "../lib/mentions";
import { validateAgent } from "../lib/wizard";
import type { AgentInput, CliInfo, DraftInput, PersonaTemplate } from "../types";
import { PersonaEditor } from "./PersonaEditor";
import { AssistantDraftDialog } from "./AssistantDraftDialog";
import { Button, Field, inputClass } from "./ui";

interface Props {
  /** 编辑已有成员时传入；新建时不传 */
  initial?: AgentInput;
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
  const [isLeader, setIsLeader] = useState(forceLeader || (initial?.isLeader ?? false));
  const [persona, setPersona] = useState(initial?.persona ?? composePersona(emptyPersona()));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [draftOpen, setDraftOpen] = useState(false);

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
      isLeader,
    };
    const selected = clis.find((c) => c.id === cli);
    const duplicate = takenNames.some((other) => normalizeName(other) === normalizeName(input.name));
    const problem = duplicate ? `成员名「${input.name}」已存在` :
      !selected?.supported ? "该 CLI 尚未接入，M5 阶段实现" : !selected.installed ? "请先安装所选 CLI" : validateAgent(input, takenNames);
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
          <select className={inputClass} value={cli} onChange={(e) => setCli(e.target.value)}>
            <option value="" disabled>
              请选择
            </option>
            {clis.map((c) => (
              <option key={c.id} value={c.id} disabled={!c.installed || !c.supported}>
                {c.label}
                {!c.supported ? "（M5 接入）" : c.installed ? (c.version ? `（${c.version}）` : "") : "（未安装）"}
              </option>
            ))}
          </select>
        </Field>
        <Field label="模型（可选，留空用 CLI 默认）">
          <input className={inputClass} value={model} onChange={(e) => setModel(e.target.value)} />
        </Field>
      </div>

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
          人设边界属于提示词约束。执行模式会自动运行工具，可能写出项目目录；CLI 会继承自身的用户与项目配置。
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
