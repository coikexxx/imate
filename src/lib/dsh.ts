import type { DshCatalog, DshProfile } from "../types";

export function encodeDshModel(profile: DshProfile, provider: string, model: string): string {
  return JSON.stringify({ profile, provider, model });
}

export function dshModelOptions(catalog: DshCatalog | null, saved: string) {
  const options = (catalog?.groups ?? []).flatMap(group => group.models.map(model => ({
    value: encodeDshModel(catalog!.profile, group.id, model.id), label: `${group.name} / ${model.name}`, unavailable: false,
  })));
  if (saved && !options.some(option => option.value === saved)) {
    let label = "已保存的模型";
    try { const selected = JSON.parse(saved); label = `${selected.provider} / ${selected.model}`; } catch { /* 保留旧值供用户重新选择。 */ }
    options.unshift({ value: saved, label: `${label}（当前列表暂不可用）`, unavailable: true });
  }
  return options;
}
