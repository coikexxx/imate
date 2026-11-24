import { confirm } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../../api";
import { assistantAvailable } from "../../lib/clis";
import { isValidRuleFilename } from "../../lib/wizard";
import type { Agent, CliInfo, DraftInput, DraftMember, Project, RuleDoc } from "../../types";
import { Icon } from "../ui/Icon";
import { Select } from "../ui/Select";
import { Button, inputClass } from "../ui/ui";
import { AssistantDraftDialog } from "./AssistantDraftDialog";

/** 项目页的规章面板。直接读写项目文件夹里的 .imates/rules/。 */
export function RulesPanel({
  projectId,
  project,
  agents,
  clis,
  closedCliIds,
  refreshTick = 0,
}: {
  projectId: string;
  project: Project;
  agents: Agent[];
  clis: CliInfo[];
  closedCliIds?: ReadonlySet<string>;
  refreshTick?: number;
}) {
  const [rules, setRules] = useState<RuleDoc[]>([]);
  const [templates, setTemplates] = useState<RuleDoc[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [newName, setNewName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [draftOpen, setDraftOpen] = useState(false);
  const [assistantDraftPending, setAssistantDraftPending] = useState(false);

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
    if (refreshTick === 0 || ownChange.current) return;
    if (dirty) {
      setStale(true);
      return;
    }
    reload(selected).catch((e) => setError(String(e)));
  }, [refreshTick]);

  const assistantReady = assistantAvailable(clis, closedCliIds);
  const context = (
    extra: Pick<DraftInput, "memberName" | "role" | "targetFilename">,
  ): Omit<DraftInput, "draftId" | "currentText" | "feedback"> => ({
    ...extra,
    folderPath: project.folderPath,
    projectName: project.name,
    description: project.description,
    rules: rules.map((rule) => (rule.filename === selected ? { ...rule, content: draft } : rule)),
    members: agents.map((agent): DraftMember => ({ name: agent.name, role: agent.role, persona: agent.persona })),
  });
  const unusedTemplates = templates.filter((t) => !rules.some((r) => r.filename === t.filename));

  const ownChange = useRef(false);

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

  const compactButton = "min-h-8 shrink-0 px-2.5 py-1 text-xs";

  return (
    <div className="grid h-full min-h-0 grid-cols-[minmax(188px,26%)_minmax(0,1fr)]">
      <div className="flex min-h-0 flex-col border-r border-line bg-paper/40">
        <ul className="min-h-0 flex-1 space-y-1 overflow-y-auto p-3">
          {rules.map((rule) => (
            <li key={rule.filename}>
              <button
                type="button"
                onClick={() => choose(rule)}
                title={rule.filename}
                className={`flex min-h-9 w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[13px] ${
                  rule.filename === selected
                    ? "bg-accent-soft font-medium text-accent"
                    : "text-ink-secondary hover:bg-surface-subtle hover:text-ink"
                }`}
              >
                <Icon name="rules" width="14" height="14" className="shrink-0 opacity-75" />
                <span className="truncate">{rule.filename}</span>
              </button>
            </li>
          ))}
          {rules.length === 0 && (
            <li className="rounded-lg border border-warn-border bg-warn-soft px-2.5 py-2 text-xs text-warn">
              本项目暂无规章，请补充至少一份
            </li>
          )}
        </ul>

        <div className="space-y-2 border-t border-line p-3">
          <Select
            size="compact"
            aria-label="从模板添加规章"
            value=""
            placeholder="从模板添加…"
            options={unusedTemplates.map((template) => ({ value: template.filename, label: template.filename }))}
            onChange={(value) => {
              const template = templates.find((t) => t.filename === value);
              if (template) create(template.filename, template.content);
            }}
          />
          <div className="flex gap-1.5">
            <input
              className={`${inputClass} min-h-8 px-2.5 py-1 text-xs`}
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

      <div className="flex min-h-0 flex-col">
        {selected !== null ? (
          <>
            <div className="flex items-center gap-2 border-b border-line px-4 py-2.5">
              <span className="truncate font-mono text-xs font-medium text-ink" title={selected}>
                .imates/rules/{selected}
              </span>
              {dirty && (
                <span className="shrink-0 rounded bg-warn-soft px-1.5 py-0.5 text-[11px] font-medium text-warn">
                  未保存
                </span>
              )}
              <span className="flex-1" />
              <Button
                className={compactButton}
                disabled={!assistantReady || !saved}
                title={!assistantReady ? "没有已安装且已接入的 CLI" : "只重写当前选中规章"}
                onClick={() => setDraftOpen(true)}
              >
                AI 重写
              </Button>
              <Button className={compactButton} variant="danger" onClick={remove}>
                删除
              </Button>
              <Button
                className={compactButton}
                variant="primary"
                disabled={!dirty}
                onClick={() => {
                  save();
                  setAssistantDraftPending(false);
                }}
              >
                {assistantDraftPending ? "确认并保存草稿" : "保存"}
              </Button>
            </div>

            <div className="flex min-h-0 flex-1 flex-col gap-2 p-4">
              <textarea
                className={`${inputClass} min-h-0 flex-1 resize-none font-mono text-[13px] leading-relaxed`}
                aria-label="规章正文"
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
              />
              {error && <p className="text-xs text-danger">{error}</p>}
              {stale && (
                <div className="flex items-center justify-between gap-2 rounded-lg border border-warn-border bg-warn-soft px-3 py-2 text-xs text-warn">
                  <span>规章文件已在外部变化，当前编辑内容尚未保存。</span>
                  <Button
                    className={compactButton}
                    onClick={() => {
                      setStale(false);
                      reload(selected).catch((e) => setError(String(e)));
                    }}
                  >
                    放弃编辑并重新载入
                  </Button>
                </div>
              )}
            </div>
          </>
        ) : (
          <p className="m-auto text-sm text-ink-muted">从左侧选择一份规章，或新增一份</p>
        )}
      </div>

      {draftOpen && saved && selected && (
        <AssistantDraftDialog
          kind="rules"
          context={context({ memberName: null, role: null, targetFilename: saved.filename })}
          initialDraft={[{ filename: saved.filename, content: draft }]}
          acceptLabel="确认并应用到编辑器"
          onGenerate={api.draftRules}
          onRewrite={(feedback, draftId, currentText) =>
            api.draftRules({
              ...context({ memberName: null, role: null, targetFilename: saved.filename }),
              draftId,
              currentText,
              feedback,
            })
          }
          onAccept={(value) => {
            if (!Array.isArray(value) || value.length !== 1 || value[0].filename !== saved.filename) {
              throw new Error("只允许应用当前选中的同名规章，请保留一份文件并使用原文件名");
            }
            setSelected(saved.filename);
            setDraft(value[0].content);
            setAssistantDraftPending(true);
          }}
          onCancel={() => setDraftOpen(false)}
        />
      )}
    </div>
  );
}
