import { assistantAvailable } from "../lib/clis";
import { confirm } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../api";
import { isValidRuleFilename } from "../lib/wizard";
import type { Agent, CliInfo, DraftInput, DraftMember, Project, RuleDoc } from "../types";
import { AssistantDraftDialog } from "./AssistantDraftDialog";
import { Button, inputClass } from "./ui";

/** 项目页的规章面板。直接读写项目文件夹里的 .imates/rules/。 */
export function RulesPanel({ projectId, project, agents, clis, closedCliIds, refreshTick = 0 }: { projectId: string; project: Project; agents: Agent[]; clis: CliInfo[]; closedCliIds?: ReadonlySet<string>; refreshTick?: number }) {
  const [rules, setRules] = useState<RuleDoc[]>([]);
  const [templates, setTemplates] = useState<RuleDoc[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [newName, setNewName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [draftOpen, setDraftOpen] = useState(false);
  const [assistantDraftPending, setAssistantDraftPending] = useState(false);

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
  const [stale, setStale] = useState(false);
  useEffect(() => {
    // 只在收到新的规章事件时触发；自己刚做的操作已经重新读取过。
    if (refreshTick === 0 || ownChange.current) return;
    if (dirty) { setStale(true); return; }
    reload(selected).catch((e) => setError(String(e)));
  }, [refreshTick]);
  const assistantReady = assistantAvailable(clis, closedCliIds);
  const context = (extra: Pick<DraftInput, "memberName" | "role" | "targetFilename">): Omit<DraftInput, "draftId" | "currentText" | "feedback"> => ({
    ...extra, folderPath: project.folderPath, projectName: project.name, description: project.description,
    rules: rules.map((rule) => rule.filename === selected ? { ...rule, content: draft } : rule),
    members: agents.map((agent): DraftMember => ({ name: agent.name, role: agent.role, persona: agent.persona })),
  });
  const unusedTemplates = templates.filter((t) => !rules.some((r) => r.filename === t.filename));

  const ownChange = useRef(false);

  /** 执行一个会改动文件的操作，失败时把原因显示出来。 */
  async function run(action: () => Promise<void>) {
    setError(null);
    ownChange.current = true;
    try {
      await action();
    } catch (e) {
      setError(String(e));
    } finally {
      ownChange.current = false;
    }
  }

  function choose(rule: RuleDoc) {
    setSelected(rule.filename);
    setDraft(rule.content);
    setAssistantDraftPending(false);
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

  const compactInput = `${inputClass} !min-h-8 !px-2 !py-1 !text-xs`;
  const compactButton = "!min-h-8 shrink-0 !px-2 !py-1 !text-xs";
  return (
    <div className="grid h-full min-h-0 grid-cols-[minmax(170px,26%)_minmax(0,1fr)]">
      {/* 左：规章列表与新增 */}
      <div className="flex min-h-0 flex-col border-r border-slate-800">
        <ul className="min-h-0 flex-1 space-y-0.5 overflow-y-auto p-2">
          {rules.map((rule) => (
            <li key={rule.filename}>
              <button
                type="button"
                onClick={() => choose(rule)}
                title={rule.filename}
                className={`min-h-8 w-full truncate rounded-lg px-2 py-1 text-left text-sm ${
                  rule.filename === selected ? "bg-slate-800 text-slate-100" : "text-slate-300 hover:bg-slate-900"
                }`}
              >
                {rule.filename}
              </button>
            </li>
          ))}
          {rules.length === 0 && (
            <li className="px-1 py-2 text-xs text-amber-300">本项目暂无规章，请补充至少一份。</li>
          )}
        </ul>
        <div className="space-y-1.5 border-t border-slate-800 p-2">
          <select
            className={compactInput}
            aria-label="从模板添加规章"
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
          <div className="flex gap-1.5">
            <input
              className={compactInput}
              aria-label="新规章文件名"
              placeholder="新文件名.md"
              title="新规章文件名，如 测试规范.md"
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
            />
            <Button
              className={compactButton}
              disabled={newName.trim() === ""}
              onClick={() => create(newName.trim(), `# ${newName.trim().replace(/\.md$/, "")}\n\n`)}
            >
              新增
            </Button>
          </div>
        </div>
      </div>

      {/* 右：工具栏与正文编辑 */}
      <div className="flex min-h-0 flex-col gap-2 p-2">
        {selected !== null ? (
          <>
            <div className="flex items-center gap-2">
              <span className="truncate text-sm text-slate-200" title={selected}>{selected}</span>
              {dirty && <span className="shrink-0 rounded bg-amber-500/20 px-1.5 text-xs text-amber-300">未保存</span>}
              <span className="flex-1" />
              <Button className={compactButton} disabled={!assistantReady || !saved} title={!assistantReady ? "没有已安装且已接入的 CLI" : "只重写当前选中规章"}
                onClick={() => setDraftOpen(true)}>AI 重写</Button>
              <Button className={compactButton} variant="danger" onClick={remove}>
                删除
              </Button>
              <Button className={compactButton} variant="primary" disabled={!dirty} onClick={() => { save(); setAssistantDraftPending(false); }}>
                {assistantDraftPending ? "确认并保存草稿" : "保存"}
              </Button>
            </div>
            <textarea
              className={`${inputClass} min-h-0 flex-1 resize-none font-mono leading-relaxed`}
              aria-label="规章正文"
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
            />
          </>
        ) : (
          <p className="m-auto text-sm text-slate-500">从左侧选择一份规章，或新增一份。</p>
        )}
        {error && <p className="text-xs text-red-400">{error}</p>}
        {stale && (
          <div className="flex items-center justify-between gap-2 rounded border border-amber-900 bg-amber-950/40 px-2 py-1 text-xs text-amber-200">
            <span>规章文件已在别处变化，当前编辑内容尚未保存。</span>
            <Button className={compactButton} onClick={() => { setStale(false); reload(selected).catch((e) => setError(String(e))); }}>放弃编辑并重新载入</Button>
          </div>
        )}
      </div>
      {draftOpen && saved && selected && <AssistantDraftDialog kind="rules" context={context({ memberName: null, role: null, targetFilename: saved.filename })}
        initialDraft={[{ filename: saved.filename, content: draft }]} acceptLabel="确认并应用到编辑器"
        onGenerate={api.draftRules}
        onRewrite={(feedback, draftId, currentText) => api.draftRules({ ...context({ memberName: null, role: null, targetFilename: saved.filename }), draftId, currentText, feedback })}
        onAccept={(value) => {
          if (!Array.isArray(value) || value.length !== 1 || value[0].filename !== saved.filename) {
            throw new Error("只允许应用当前选中的同名规章，请保留一份文件并使用原文件名");
          }
          setSelected(saved.filename); setDraft(value[0].content); setAssistantDraftPending(true);
        }} onCancel={() => setDraftOpen(false)} />}
    </div>
  );
}
