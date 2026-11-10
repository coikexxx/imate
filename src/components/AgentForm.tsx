import { useState } from "react";
import { composePersona, emptyPersona, parsePersona } from "../lib/persona";
import { validateAgent } from "../lib/wizard";
import type { AgentInput, CliInfo, PersonaTemplate } from "../types";
import { PersonaEditor } from "./PersonaEditor";
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
}: Props) {
  const [name, setName] = useState(initial?.name ?? "");
  const [role, setRole] = useState(initial?.role ?? "");
  const [cli, setCli] = useState(initial?.cli ?? clis.find((c) => c.installed)?.id ?? "");
  const [model, setModel] = useState(initial?.model ?? "");
  const [isLeader, setIsLeader] = useState(forceLeader || (initial?.isLeader ?? false));
  const [persona, setPersona] = useState(initial?.persona ?? composePersona(emptyPersona()));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

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
    const problem = validateAgent(input, takenNames);
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
              <option key={c.id} value={c.id} disabled={!c.installed}>
                {c.label}
                {c.installed ? (c.version ? `（${c.version}）` : "") : "（未安装）"}
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
          <Button disabled={!template} onClick={() => template && setPersona(template.persona)}>
            套用「{role.trim() || "预设角色"}」模板
          </Button>
        </div>
        <PersonaEditor value={persona} onChange={setPersona} />
        <p className="text-xs text-slate-500">
          边界是写给 Agent 看的软约束，应用不做硬性拦截；各 CLI 的沙箱强度也不相同。
        </p>
      </div>

      {error && <p className="text-sm text-red-400">{error}</p>}

      <div className="flex justify-end gap-2">
        <Button onClick={onCancel}>取消</Button>
        <Button variant="primary" disabled={busy} onClick={submit}>
          {submitLabel}
        </Button>
      </div>
    </div>
  );
}
