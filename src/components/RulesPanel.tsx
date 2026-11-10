import { confirm } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import { isValidRuleFilename } from "../lib/wizard";
import type { RuleDoc } from "../types";
import { Button, inputClass } from "./ui";

/** 项目页的规章面板。直接读写项目文件夹里的 .imates/rules/。 */
export function RulesPanel({ projectId }: { projectId: string }) {
  const [rules, setRules] = useState<RuleDoc[]>([]);
  const [templates, setTemplates] = useState<RuleDoc[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [newName, setNewName] = useState("");
  const [error, setError] = useState<string | null>(null);

  /** 重新读取规章列表，并选中指定的那份（找不到就选第一份）。 */
  const reload = useCallback(
    async (prefer: string | null) => {
      const list = await api.listRules(projectId);
      const target = list.find((rule) => rule.filename === prefer) ?? list[0] ?? null;
      setRules(list);
      setSelected(target?.filename ?? null);
      setDraft(target?.content ?? "");
    },
    [projectId],
  );

  useEffect(() => {
    reload(null).catch((e) => setError(String(e)));
    api.listRuleTemplates().then(setTemplates);
  }, [reload]);

  const saved = rules.find((rule) => rule.filename === selected);
  const dirty = saved !== undefined && saved.content !== draft;
  const unusedTemplates = templates.filter((t) => !rules.some((r) => r.filename === t.filename));

  /** 执行一个会改动文件的操作，失败时把原因显示出来。 */
  async function run(action: () => Promise<void>) {
    setError(null);
    try {
      await action();
    } catch (e) {
      setError(String(e));
    }
  }

  function choose(rule: RuleDoc) {
    setSelected(rule.filename);
    setDraft(rule.content);
  }

  function save() {
    if (selected === null) return;
    const filename = selected;
    run(async () => {
      await api.saveRule(projectId, filename, draft);
      await reload(filename);
    });
  }

  async function remove() {
    if (selected === null) return;
    const filename = selected;
    const confirmed = await confirm(`删除规章「${filename}」？文件会从项目文件夹里移除。`, {
      title: "删除规章",
      kind: "warning",
    });
    if (!confirmed) return;
    run(async () => {
      await api.deleteRule(projectId, filename);
      await reload(null);
    });
  }

  function create(filename: string, content: string) {
    if (!isValidRuleFilename(filename)) {
      setError(`规章文件名不合法：${filename}（需以 .md 结尾，且不含 \\ / : * ? " < > |）`);
      return;
    }
    if (rules.some((rule) => rule.filename === filename)) {
      setError(`规章已存在：${filename}`);
      return;
    }
    run(async () => {
      await api.saveRule(projectId, filename, content);
      setNewName("");
      await reload(filename);
    });
  }

  return (
    <div className="flex h-full flex-col gap-3 p-3">
      <ul className="space-y-1">
        {rules.map((rule) => (
          <li key={rule.filename}>
            <button
              type="button"
              onClick={() => choose(rule)}
              className={`w-full truncate rounded px-2 py-1.5 text-left text-sm ${
                rule.filename === selected ? "bg-sky-900/60 text-sky-100" : "hover:bg-slate-800"
              }`}
            >
              {rule.filename}
            </button>
          </li>
        ))}
      </ul>

      {rules.length === 0 && (
        <p className="text-sm text-amber-300">本项目暂无规章，请补充至少一份。</p>
      )}

      {selected !== null && (
        <>
          <textarea
            className={`${inputClass} min-h-40 flex-1 font-mono`}
            aria-label="规章正文"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
          />
          <div className="flex justify-end gap-2">
            <Button variant="danger" onClick={remove}>
              删除
            </Button>
            <Button variant="primary" disabled={!dirty} onClick={save}>
              保存
            </Button>
          </div>
        </>
      )}

      {error && <p className="text-sm text-red-400">{error}</p>}

      <div className="space-y-2 border-t border-slate-800 pt-3">
        <select
          className={inputClass}
          value=""
          onChange={(e) => {
            const template = templates.find((t) => t.filename === e.target.value);
            if (template) create(template.filename, template.content);
          }}
        >
          <option value="">从模板添加…</option>
          {unusedTemplates.map((t) => (
            <option key={t.filename} value={t.filename}>
              {t.filename}
            </option>
          ))}
        </select>
        <div className="flex gap-2">
          <input
            className={inputClass}
            aria-label="新规章文件名"
            placeholder="新规章文件名，如 测试规范.md"
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
          />
          <Button
            disabled={newName.trim() === ""}
            onClick={() => create(newName.trim(), `# ${newName.trim().replace(/\.md$/, "")}\n\n`)}
          >
            新增
          </Button>
        </div>
      </div>
    </div>
  );
}
