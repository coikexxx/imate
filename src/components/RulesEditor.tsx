import { useState } from "react";
import { uniqueRuleFilename } from "../lib/wizard";
import type { RuleDoc } from "../types";
import { Button, inputClass } from "./ui";
import { Select } from "./Select";

interface Props {
  rules: RuleDoc[];
  templates: RuleDoc[];
  onChange: (rules: RuleDoc[]) => void;
}

/** 向导里编辑规章草稿用。只改内存里的列表，不读写文件。 */
export function RulesEditor({ rules, templates, onChange }: Props) {
  const [selected, setSelected] = useState(0);
  const index = Math.min(selected, rules.length - 1);
  const current = index >= 0 ? rules[index] : undefined;
  const unusedTemplates = templates.filter((t) => !rules.some((r) => r.filename === t.filename));

  function add(doc: RuleDoc) {
    onChange([...rules, doc]);
    setSelected(rules.length);
  }

  function patch(changes: Partial<RuleDoc>) {
    onChange(rules.map((rule, i) => (i === index ? { ...rule, ...changes } : rule)));
  }

  function remove() {
    onChange(rules.filter((_, i) => i !== index));
    setSelected(Math.max(0, index - 1));
  }

  return (
    <div className="grid grid-cols-[200px_1fr] gap-4">
      <div className="space-y-2">
        <ul className="space-y-1">
          {rules.map((rule, i) => (
            <li key={i}>
              <button
                type="button"
                onClick={() => setSelected(i)}
                className={`w-full truncate rounded px-2 py-1.5 text-left text-sm ${
                  i === index ? "bg-sky-900/60 text-sky-100" : "hover:bg-slate-800"
                }`}
              >
                {rule.filename}
              </button>
            </li>
          ))}
        </ul>
        <Select aria-label="从模板添加规章" value="" placeholder="从模板添加…"
          options={unusedTemplates.map((template) => ({ value: template.filename, label: template.filename }))}
          onChange={(value) => {
            const template = templates.find((t) => t.filename === value);
            if (template) add(template);
          }} />
        <Button
          className="w-full"
          onClick={() =>
            add({
              filename: uniqueRuleFilename(
                "新规章.md",
                rules.map((r) => r.filename),
              ),
              content: "# 新规章\n\n",
            })
          }
        >
          新增空白规章
        </Button>
      </div>

      {current ? (
        <div className="space-y-2">
          <div className="flex gap-2">
            <input
              className={inputClass}
              aria-label="规章文件名"
              value={current.filename}
              onChange={(e) => patch({ filename: e.target.value })}
            />
            <Button variant="danger" onClick={remove}>
              删除
            </Button>
          </div>
          <textarea
            className={`${inputClass} h-80 font-mono`}
            aria-label="规章正文"
            value={current.content}
            onChange={(e) => patch({ content: e.target.value })}
          />
        </div>
      ) : (
        <p className="text-sm text-slate-400">还没有规章。从左侧选择模板，或新增一份空白规章。</p>
      )}
    </div>
  );
}
