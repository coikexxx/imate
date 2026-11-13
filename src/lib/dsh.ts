import type { DshCatalog, DshProfile } from "../types";

export interface DshSelectionValue { profile: DshProfile; provider: string; model: string; reasoningEffort?: string }

/** 成员的 Dsh 模型值是 JSON。reasoningEffort 为空时不写出该字段，与旧数据保持一致。 */
export function encodeDshModel(profile: DshProfile, provider: string, model: string, reasoningEffort?: string | null): string {
  return JSON.stringify(reasoningEffort ? { profile, provider, model, reasoningEffort } : { profile, provider, model });
}

export function parseDshModel(value: string): DshSelectionValue | null {
  try {
    const parsed: unknown = JSON.parse(value);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return null;
    const { profile, provider, model, reasoningEffort } = parsed as Record<string, unknown>;
    if ((profile !== "web" && profile !== "headless") || typeof provider !== "string" || typeof model !== "string") return null;
    return { profile, provider, model, ...(typeof reasoningEffort === "string" && reasoningEffort ? { reasoningEffort } : {}) };
  } catch { return null; }
}

/** 去掉推理强度后的模型值：模型下拉框的选项和应用管理白名单都按它比较。 */
export function dshBaseValue(value: string): string {
  const parsed = parseDshModel(value);
  return parsed ? encodeDshModel(parsed.profile, parsed.provider, parsed.model) : value;
}

/** 当前值里的推理强度；没有选择返回空字符串。 */
export function dshEffortOf(value: string): string { return parseDshModel(value)?.reasoningEffort ?? ""; }

/** 换掉推理强度，模型保持不变；传空字符串表示清除。无法解析的值原样返回。 */
export function withDshEffort(value: string, effort: string): string {
  const parsed = parseDshModel(value);
  return parsed ? encodeDshModel(parsed.profile, parsed.provider, parsed.model, effort) : value;
}

/** 换模型：新模型也声明了原来的强度就保留，否则清掉，避免带着不支持的强度发出去。 */
export function changeDshModel(catalog: DshCatalog | null, previous: string, next: string): string {
  const effort = dshEffortOf(previous);
  if (!effort || !next) return next;
  return dshEffortChoices(catalog, next).options.some(option => option.value === effort && !option.unavailable) ? withDshEffort(next, effort) : next;
}

export function dshModelOptions(catalog: DshCatalog | null, saved: string) {
  const options = (catalog?.groups ?? []).flatMap(group => group.models.map(model => ({
    value: encodeDshModel(catalog!.profile, group.id, model.id), label: `${group.name} / ${model.name}`, unavailable: false,
  })));
  if (saved && !options.some(option => option.value === saved)) {
    const selected = parseDshModel(saved);
    const label = selected ? `${selected.provider} / ${selected.model}` : "已保存的模型";
    options.unshift({ value: saved, label: `${label}（当前列表暂不可用）`, unavailable: true });
  }
  return options;
}

export interface DshEffortChoices {
  /** 第一项永远是「默认」，值为空字符串 */
  options: { value: string; label: string; unavailable: boolean }[];
  /** 目录是否声明了该模型的强度；没有声明时只能用默认 */
  declared: boolean;
}

/** 某个模型值可选的推理强度。目录里查不到模型或没有声明时，只给「默认」。 */
export function dshEffortChoices(catalog: DshCatalog | null, value: string): DshEffortChoices {
  const selected = parseDshModel(value);
  const model = selected ? catalog?.groups.find(group => group.id === selected.provider)?.models.find(entry => entry.id === selected.model) : undefined;
  const efforts = model?.efforts ?? [];
  const defaultLabel = model?.defaultEffort ? `默认（${model.defaultEffort}）` : "默认（沿用 Dsh 配置）";
  const options = [{ value: "", label: defaultLabel, unavailable: false },
    ...efforts.map(effort => ({ value: effort.id, label: effort.name === effort.id ? effort.id : `${effort.name}（${effort.id}）`, unavailable: false }))];
  const current = selected?.reasoningEffort;
  if (current && !options.some(option => option.value === current)) {
    options.push({ value: current, label: `${current}（当前列表暂不可用）`, unavailable: true });
  }
  return { options, declared: efforts.length > 0 };
}
