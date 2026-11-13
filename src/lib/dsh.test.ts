import { describe, expect, it } from "vitest";
import { changeDshModel, dshBaseValue, dshEffortChoices, dshEffortOf, encodeDshModel, dshModelOptions, parseDshModel, withDshEffort } from "./dsh";
import { modelNote } from "./modelNote";

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

describe("Dsh 推理强度", () => {
  const efforts = [{ id: "off", name: "off" }, { id: "low", name: "low" }, { id: "high", name: "high" }];
  const catalog = { profile: "web" as const, failures: [], groups: [
    { id: "claude", name: "claude", models: [
      { id: "opus", name: "opus", efforts, defaultEffort: "high" },
      { id: "haiku", name: "haiku" },
      { id: "short", name: "short", efforts: [{ id: "low", name: "low" }] },
    ] },
  ] };
  const opus = encodeDshModel("web", "claude", "opus");

  it("没选强度时值与旧格式完全一致，选了才多出 reasoningEffort", () => {
    expect(JSON.parse(opus)).toEqual({ profile: "web", provider: "claude", model: "opus" });
    expect(JSON.parse(withDshEffort(opus, "high"))).toEqual({ profile: "web", provider: "claude", model: "opus", reasoningEffort: "high" });
    expect(withDshEffort(withDshEffort(opus, "high"), "")).toBe(opus);
  });

  it("模型选项和白名单按去掉强度的值比较", () => {
    const withEffort = withDshEffort(opus, "high");
    expect(dshBaseValue(withEffort)).toBe(opus);
    expect(dshEffortOf(withEffort)).toBe("high");
    expect(dshEffortOf(opus)).toBe("");
    expect(dshModelOptions(catalog, dshBaseValue(withEffort)).some(option => option.value === opus && !option.unavailable)).toBe(true);
    expect(dshBaseValue("不是 JSON")).toBe("不是 JSON");
    expect(parseDshModel('{"profile":"x","provider":"a","model":"b"}')).toBeNull();
  });

  it("强度选项来自目录：第一项是默认并标出默认值，其余按声明顺序", () => {
    const choices = dshEffortChoices(catalog, opus);
    expect(choices.declared).toBe(true);
    expect(choices.options.map(option => option.value)).toEqual(["", "off", "low", "high"]);
    expect(choices.options[0].label).toBe("默认（high）");
    expect(dshEffortChoices(catalog, encodeDshModel("web", "claude", "short")).options[0].label).toBe("默认（沿用 Dsh 配置）");
  });

  it("模型没有声明强度时只有默认；已保存的强度仍保留并标为暂不可用", () => {
    const haiku = encodeDshModel("web", "claude", "haiku");
    expect(dshEffortChoices(catalog, haiku)).toMatchObject({ declared: false, options: [{ value: "" }] });
    const stale = dshEffortChoices(catalog, withDshEffort(haiku, "high"));
    expect(stale.options[stale.options.length - 1]).toMatchObject({ value: "high", unavailable: true });
    expect(dshEffortChoices(null, opus).options).toHaveLength(1);
  });

  it("换模型时，新模型支持原强度就保留，不支持就清掉", () => {
    const high = withDshEffort(opus, "high");
    expect(dshEffortOf(changeDshModel(catalog, high, encodeDshModel("web", "claude", "short")))).toBe("");
    expect(dshEffortOf(changeDshModel(catalog, withDshEffort(opus, "low"), encodeDshModel("web", "claude", "short")))).toBe("low");
    expect(changeDshModel(catalog, high, "")).toBe("");
    expect(changeDshModel(catalog, opus, encodeDshModel("web", "claude", "haiku"))).toBe(encodeDshModel("web", "claude", "haiku"));
  });

  it("消息旁的模型说明带上强度", () => {
    const note = modelNote({ requestedModel: withDshEffort(opus, "high"), reportedModel: null, runStatus: "ok" });
    expect(note).toBe("claude / opus · high（请求值，CLI 未回报）");
    expect(modelNote({ requestedModel: opus, reportedModel: null, runStatus: "ok" })).toBe("claude / opus（请求值，CLI 未回报）");
  });
});
