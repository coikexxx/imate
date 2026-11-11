import { describe, expect, it } from "vitest";
import { encodeDshModel, dshModelOptions } from "./dsh";

describe("Dsh 配置模型", () => {
  const catalog = { profile: "web" as const, groups: [{ id: "gpt", name: "已配置 GPT", models: [{ id: "gpt-6", name: "GPT-6" }] }], failures: [] };
  it("模型值同时保留来源、provider 和模型 ID", () => {
    expect(JSON.parse(encodeDshModel("web", "gpt", "gpt-6"))).toEqual({ profile: "web", provider: "gpt", model: "gpt-6" });
    expect(dshModelOptions(catalog, "").some(option => option.value === encodeDshModel("web", "gpt", "gpt-6"))).toBe(true);
  });
  it("目录变化或加载失败时保留已有选择", () => {
    const saved = encodeDshModel("headless", "gateway", "旧模型");
    for (const directory of [catalog, null]) {
      const selected = dshModelOptions(directory, saved).find(option => option.value === saved);
      expect(selected?.label).toContain("旧模型");
      expect(selected?.unavailable).toBe(true);
    }
  });
});
