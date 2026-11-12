import type { CliConfig, CliConfigInput, CliModel } from "../types";

/** 列表页分组：按是否检测到安装，各组保持后端给出的顺序 */
export function groupClis(configs: CliConfig[]) {
  return { installed: configs.filter((c) => c.installed), missing: configs.filter((c) => !c.installed) };
}

/** 开关显示：未支持、未安装一律关闭且不可切换 */
export function toggleState(config: CliConfig): { checked: boolean; disabled: boolean; hint: string | null } {
  if (!config.supported) return { checked: false, disabled: true, hint: "暂未支持" };
  if (!config.installed) return { checked: false, disabled: true, hint: "未检测到，可在详情里指定路径" };
  return { checked: config.enabled, disabled: false, hint: null };
}

/** 成员表单里能选的 CLI。编辑已有成员时，当前 CLI 即使已关闭也保留 */
export function selectableClis(configs: CliConfig[], keep?: string): CliConfig[] {
  return configs.filter((c) => (c.enabled && c.installed && c.supported) || c.id === keep);
}

export function toInput(config: CliConfig, patch: Partial<CliConfigInput> = {}): CliConfigInput {
  return { enabled: config.enabled, customPath: config.customPath, models: config.models, defaultModel: config.defaultModel, ...patch };
}

export interface ModelRow extends CliModel { checked: boolean }

/** 候选列表与已勾选列表合并：候选默认不勾选，已勾选但不在候选里的（手填或候选已变）追加在后面 */
export function mergeModels(selected: CliModel[], candidates: CliModel[]): ModelRow[] {
  const rows: ModelRow[] = candidates.map((c) => ({ id: c.id, label: c.label, custom: false, checked: selected.some((s) => s.id === c.id) }));
  for (const s of selected) {
    if (!rows.some((r) => r.id === s.id)) rows.push({ ...s, checked: true });
  }
  return rows;
}

export function selectedModels(rows: ModelRow[]): CliModel[] {
  return rows.filter((r) => r.checked).map(({ id, label, custom }) => ({ id, label, custom }));
}

export function addCustomModel(rows: ModelRow[], raw: string): ModelRow[] {
  const id = raw.trim();
  if (!id) return rows;
  if (rows.some((r) => r.id === id)) return rows.map((r) => (r.id === id ? { ...r, checked: true } : r));
  return [...rows, { id, label: id, custom: true, checked: true }];
}

/** 默认模型必须是已勾选的；取消勾选后自动清空 */
export function reconcileDefault(rows: ModelRow[], current: string | null): string | null {
  return current && rows.some((r) => r.checked && r.id === current) ? current : null;
}

export interface MemberModelOptions { restricted: boolean; options: { value: string; label: string }[] }

/** 成员表单的模型选项。白名单为空不限制，沿用自由输入 */
export function memberModelOptions(config: CliConfig | undefined, saved: string): MemberModelOptions {
  if (!config || config.models.length === 0) return { restricted: false, options: [] };
  const options = config.models.map((m) => ({ value: m.id, label: m.label }));
  if (saved && !options.some((o) => o.value === saved)) options.unshift({ value: saved, label: `${saved}（不在白名单）` });
  return { restricted: true, options };
}

export function defaultModelHint(config: CliConfig | undefined): string {
  const model = config?.models.find((m) => m.id === config.defaultModel);
  return model ? `使用默认（${model.label}）` : "使用 CLI 自带默认";
}

/** Dsh 候选按白名单过滤；白名单为空不过滤；已保存的值总是保留 */
export function filterDshOptions<T extends { value: string }>(options: T[], config: CliConfig | undefined, saved: string): T[] {
  if (!config || config.models.length === 0) return options;
  const allowed = new Set(config.models.map((m) => m.id));
  return options.filter((o) => allowed.has(o.value) || o.value === saved);
}
