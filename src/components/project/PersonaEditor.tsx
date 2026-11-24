import { useEffect, useState } from "react";
import {
  PERSONA_SECTIONS,
  composePersona,
  parsePersona,
  type PersonaParts,
  type PersonaSection,
} from "../../lib/persona";
import { inputClass } from "../ui/ui";

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

  useEffect(() => {
    setParts((current) => (composePersona(current) === value ? current : parsePersona(value)));
  }, [value]);

  function update(section: PersonaSection, text: string) {
    const next = { ...parts, [section]: text };
    setParts(next);
    onChange(composePersona(next));
  }

  return (
    <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
      {PERSONA_SECTIONS.map((section) => (
        <label key={section} className="block space-y-1">
          <span className="flex items-baseline justify-between gap-2 text-xs">
            <span className="font-medium text-ink">{section}</span>
            <span className="truncate text-[11px] text-ink-muted">{HINTS[section]}</span>
          </span>
          <textarea
            className={`${inputClass} text-[13px] leading-relaxed`}
            rows={section === "定位" ? 3 : 4}
            value={parts[section]}
            onChange={(e) => update(section, e.target.value)}
          />
        </label>
      ))}
    </div>
  );
}
