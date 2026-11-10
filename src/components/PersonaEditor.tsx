import { useEffect, useState } from "react";
import {
  PERSONA_SECTIONS,
  composePersona,
  parsePersona,
  type PersonaParts,
  type PersonaSection,
} from "../lib/persona";
import { inputClass } from "./ui";

const HINTS: Record<PersonaSection, string> = {
  定位: "一句话说明这个成员是谁",
  职责: "能干什么，负责哪些产出",
  边界: "不做什么；遇到职责外的事该交给谁",
  工作风格: "讨论时的立场倾向，例如偏保守、偏用户视角",
};

interface Props {
  /** 完整的人设 Markdown */
  value: string;
  onChange: (markdown: string) => void;
}

export function PersonaEditor({ value, onChange }: Props) {
  const [parts, setParts] = useState<PersonaParts>(() => parsePersona(value));

  // 外部整份替换了人设（例如套用模板）时重新拆分。
  // 自己输入引起的变化不重置，否则每次按键都会被 trim 掉行尾的空格和换行。
  useEffect(() => {
    setParts((current) => (composePersona(current) === value ? current : parsePersona(value)));
  }, [value]);

  function update(section: PersonaSection, text: string) {
    const next = { ...parts, [section]: text };
    setParts(next);
    onChange(composePersona(next));
  }

  return (
    <div className="space-y-3">
      {PERSONA_SECTIONS.map((section) => (
        <label key={section} className="block space-y-1">
          <span className="text-xs text-slate-300">
            {section}
            <span className="ml-2 text-slate-500">{HINTS[section]}</span>
          </span>
          <textarea
            className={inputClass}
            rows={section === "定位" ? 2 : 4}
            value={parts[section]}
            onChange={(e) => update(section, e.target.value)}
          />
        </label>
      ))}
    </div>
  );
}
