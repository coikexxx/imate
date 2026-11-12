import { describe, expect, it } from "vitest";
import type { CliConfig } from "../types";
import {
  addCustomModel, closedCliIds, defaultModelHint, filterDshOptions, groupClis, memberModelOptions, mergeModels,
  isDetailDirty, providerHint, reconcileDefault, selectableClis, selectedModels, supportsProviderQuery, toggleState, toInput,
} from "./cliConfig";

function cli(patch: Partial<CliConfig> & { id: string }): CliConfig {
  return {
    label: patch.id, installed: true, supported: true, path: "/bin/x", version: "1.0.0", enabled: true,
    customPath: null, models: [], defaultModel: null, modelsSource: "builtin", ...patch,
  };
}

describe("应用管理 纯函数", () => {
  it("按是否安装分组并保持顺序", () => {
    const groups = groupClis([cli({ id: "a" }), cli({ id: "b", installed: false }), cli({ id: "c", supported: false })]);
    expect(groups.installed.map((c) => c.id)).toEqual(["a", "c"]);
    expect(groups.missing.map((c) => c.id)).toEqual(["b"]);
  });

  it("开关状态：未支持与未安装都置灰且显示关闭", () => {
    expect(toggleState(cli({ id: "a" }))).toEqual({ checked: true, disabled: false, hint: null });
    expect(toggleState(cli({ id: "a", enabled: false })).checked).toBe(false);
    expect(toggleState(cli({ id: "pi", supported: false, enabled: false }))).toMatchObject({ checked: false, disabled: true, hint: "暂未支持" });
    expect(toggleState(cli({ id: "x", installed: false }))).toMatchObject({ checked: false, disabled: true });
  });

  it("开关提示：未安装提示可指定路径；已安装但不受支持显示暂未支持且置灰关闭", () => {
    expect(toggleState(cli({ id: "x", installed: false })).hint).toBe("未检测到，可在详情里指定路径");
    expect(toggleState(cli({ id: "pi", installed: true, supported: false, enabled: true })))
      .toEqual({ checked: false, disabled: true, hint: "暂未支持" });
  });

  it("成员表单只列已启用、已安装、受支持的 CLI，编辑时保留当前值", () => {
    const all = [cli({ id: "a" }), cli({ id: "b", enabled: false }), cli({ id: "c", installed: false }), cli({ id: "pi", supported: false })];
    expect(selectableClis(all).map((c) => c.id)).toEqual(["a"]);
    expect(selectableClis(all, "b").map((c) => c.id)).toEqual(["a", "b"]);
  });

  it("已关闭的 CLI 集合只含 enabled 为 false 的", () => {
    const ids = closedCliIds([cli({ id: "a" }), cli({ id: "b", enabled: false }), cli({ id: "pi", enabled: false, supported: false })]);
    expect([...ids].sort()).toEqual(["b", "pi"]);
    expect(closedCliIds([]).size).toBe(0);
  });

  it("toInput 只带可保存字段并允许覆盖", () => {
    const config = cli({ id: "a", models: [{ id: "m", label: "m", custom: false }], defaultModel: "m", customPath: "/p" });
    expect(toInput(config)).toEqual({ enabled: true, customPath: "/p", models: config.models, defaultModel: "m" });
    expect(toInput(config, { enabled: false }).enabled).toBe(false);
  });

  it("toInput 返回的 models 是拷贝，不与传入配置共享引用", () => {
    const config = cli({ id: "a", models: [{ id: "m", label: "m", custom: false }] });
    const input = toInput(config);
    expect(input.models).toEqual(config.models);
    expect(input.models).not.toBe(config.models);
    expect(input.models[0]).not.toBe(config.models[0]);
    input.models[0].label = "改了";
    expect(config.models[0].label).toBe("m");
  });

  it("合并候选时按 id 去重，先出现的优先", () => {
    const rows = mergeModels([], [
      { id: "a", label: "第一个 a", custom: false },
      { id: "b", label: "b", custom: false },
      { id: "a", label: "重复的 a", custom: false },
    ]);
    expect(rows.map((r) => r.id)).toEqual(["a", "b"]);
    expect(rows[0].label).toBe("第一个 a");
  });

  it("详情页未保存修改判断：逐项与传入配置比较", () => {
    const config = cli({ id: "a", enabled: true, customPath: "/p", models: [{ id: "m1", label: "m1", custom: false }, { id: "m2", label: "m2", custom: false }], defaultModel: "m1" });
    const same = { enabled: true, customPath: "/p", models: [{ id: "m2", label: "m2", custom: false }, { id: "m1", label: "m1", custom: false }], defaultModel: "m1" };
    expect(isDetailDirty(config, same)).toBe(false);
    expect(isDetailDirty(config, { ...same, customPath: " /p " })).toBe(false);
    expect(isDetailDirty(config, { ...same, enabled: false })).toBe(true);
    expect(isDetailDirty(config, { ...same, customPath: "/q" })).toBe(true);
    expect(isDetailDirty(config, { ...same, models: same.models.slice(0, 1) })).toBe(true);
    expect(isDetailDirty(config, { ...same, models: [...same.models, { id: "m3", label: "m3", custom: true }] })).toBe(true);
    expect(isDetailDirty(config, { ...same, defaultModel: null })).toBe(true);
    const bare = cli({ id: "b", customPath: null });
    expect(isDetailDirty(bare, { enabled: true, customPath: "   ", models: [], defaultModel: null })).toBe(false);
    expect(isDetailDirty(bare, { enabled: true, customPath: "/x", models: [], defaultModel: null })).toBe(true);
  });

  it("合并候选与已选：已选自定义模型保留，候选默认不勾选", () => {
    const rows = mergeModels(
      [{ id: "b", label: "b", custom: false }, { id: "mine", label: "mine", custom: true }],
      [{ id: "a", label: "a", custom: false }, { id: "b", label: "B 模型", custom: false }],
    );
    expect(rows.map((r) => [r.id, r.checked, r.custom])).toEqual([["a", false, false], ["b", true, false], ["mine", true, true]]);
    expect(rows[1].label).toBe("B 模型");
    expect(selectedModels(rows).map((m) => m.id)).toEqual(["b", "mine"]);
    expect(selectedModels(rows)[0]).not.toHaveProperty("checked");
  });

  it("手填模型：去空白、去重、已存在则直接勾选", () => {
    const rows = mergeModels([], [{ id: "a", label: "a", custom: false }]);
    expect(addCustomModel(rows, "   ")).toEqual(rows);
    const added = addCustomModel(rows, " x-1 ");
    expect(added[1]).toEqual({ id: "x-1", label: "x-1", custom: true, checked: true });
    const again = addCustomModel(added, "a");
    expect(again.find((r) => r.id === "a")?.checked).toBe(true);
    expect(again).toHaveLength(2);
  });

  it("取消勾选后默认模型自动清空", () => {
    const rows = mergeModels([{ id: "a", label: "a", custom: false }], []);
    expect(reconcileDefault(rows, "a")).toBe("a");
    expect(reconcileDefault(rows.map((r) => ({ ...r, checked: false })), "a")).toBeNull();
    expect(reconcileDefault(rows, null)).toBeNull();
  });

  it("成员模型框：白名单为空不限制；有白名单则只列白名单并保留不在名单内的旧值", () => {
    expect(memberModelOptions(undefined, "")).toEqual({ restricted: false, options: [] });
    expect(memberModelOptions(cli({ id: "a" }), "x")).toEqual({ restricted: false, options: [] });
    const config = cli({ id: "a", models: [{ id: "m1", label: "模型一", custom: false }] });
    expect(memberModelOptions(config, "")).toEqual({ restricted: true, options: [{ value: "m1", label: "模型一" }] });
    const withOld = memberModelOptions(config, "old");
    expect(withOld.options[0]).toEqual({ value: "old", label: "old（不在白名单）" });
  });

  it("默认模型提示", () => {
    expect(defaultModelHint(undefined)).toBe("使用 CLI 自带默认");
    expect(defaultModelHint(cli({ id: "a", models: [{ id: "m1", label: "模型一", custom: false }], defaultModel: "m1" }))).toBe("使用默认（模型一）");
  });

  it("Dsh 候选按白名单过滤，白名单为空时不过滤，已保存值保留", () => {
    const options = [{ value: "p1" }, { value: "p2" }];
    expect(filterDshOptions(options, undefined, "")).toEqual(options);
    expect(filterDshOptions(options, cli({ id: "dsh" }), "")).toEqual(options);
    const config = cli({ id: "dsh", models: [{ id: "p1", label: "p1", custom: false }] });
    expect(filterDshOptions(options, config, "")).toEqual([{ value: "p1" }]);
    expect(filterDshOptions(options, config, "p2")).toEqual(options);
  });

  it("只有 Claude 和 Codex 支持从供应商读取", () => {
    expect(supportsProviderQuery("claude")).toBe(true);
    expect(supportsProviderQuery("codex")).toBe(true);
    for (const id of ["grok", "dsh", "pi", ""]) expect(supportsProviderQuery(id)).toBe(false);
  });

  it("提示写明密钥发给谁：已知主机名时点名，否则泛指", () => {
    expect(providerHint("gw.example.com")).toBe("点击后会把该 CLI 配置里的密钥发给 gw.example.com");
    expect(providerHint(null)).toBe("点击后会把该 CLI 配置里的密钥发给它配置的供应商");
  });
});
