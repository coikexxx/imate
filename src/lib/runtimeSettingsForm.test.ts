import { describe, expect, it } from "vitest";
import {
  assistantNotice, assistantOptions, DEFAULT_LIMITS, fromForm, initialPanel, isDirty, LIMIT_FIELDS, panelReducer, toForm,
} from "./runtimeSettingsForm";
import type { CliConfig, CliInfo, RuntimeSettings } from "../types";

const saved: RuntimeSettings = {
  assistantCli: null,
  limits: { discussRounds: 3, reworksPerTask: 2, turnsPerInstruction: 30, modelIdleSeconds: 300,
    discussToolSeconds: 600, executeToolSeconds: 1800, discussTotalSeconds: 600, executeTotalSeconds: 1800 },
};
const cli = (id: string, label: string, installed: boolean, supported = true): CliInfo => ({ id, label, installed, supported, path: null, version: null });
const config = (id: string, enabled: boolean, installed = true): CliConfig => ({
  ...cli(id, id, installed), enabled, customPath: null, models: [], defaultModel: null, modelsSource: "none",
});
const clis = [cli("claude", "Claude Code", false), cli("codex", "Codex", true), cli("grok", "Grok", true), cli("dsh", "DeepSeek Harness", true), cli("pi", "Pi", false, false)];

describe("运行设置表单", () => {
  it("默认值与手册一致，秒换算成分钟展示，空助理显示为自动选择", () => {
    expect(DEFAULT_LIMITS).toEqual(saved.limits);
    const form = toForm(saved);
    expect(form.assistantCli).toBe("");
    expect(form.limits).toEqual({ discussRounds: "3", reworksPerTask: "2", turnsPerInstruction: "30",
      modelIdleMinutes: "5", discussToolMinutes: "10", executeToolMinutes: "30", discussTotalMinutes: "10", executeTotalMinutes: "30" });
    expect(toForm({ ...saved, assistantCli: "dsh", limits: { ...saved.limits, modelIdleSeconds: 90 } }).limits.modelIdleMinutes).toBe("1.5");
    // 每个字段都带单位、范围与生效时机说明，用户提交前就能看到。
    expect(LIMIT_FIELDS.map((field) => field.key)).toEqual(Object.keys(form.limits));
    for (const field of LIMIT_FIELDS) {
      expect(["回合", "次", "分钟"]).toContain(field.unit);
      expect(field.hint).toMatch(/新指令|下一次/);
    }
  });

  it("提交时分钟换回整秒，整数字段拒绝小数与越界，并指出是哪一项", () => {
    const form = toForm(saved);
    const parsed = fromForm({ ...form, assistantCli: " codex " });
    expect(parsed).toEqual({ ok: true, settings: { ...saved, assistantCli: "codex" } });
    const decimal = fromForm({ ...form, limits: { ...form.limits, modelIdleMinutes: "1.5" } });
    expect(decimal.ok && decimal.settings.limits.modelIdleSeconds).toBe(90);
    const cases: [Partial<typeof form.limits>, RegExp][] = [
      [{ discussRounds: "0" }, /讨论轮数.*1 到 10/],
      [{ discussRounds: "2.5" }, /讨论轮数.*整数/],
      [{ reworksPerTask: "11" }, /返工.*0 到 10/],
      [{ turnsPerInstruction: "" }, /回合总数/],
      [{ turnsPerInstruction: "301" }, /回合总数.*1 到 300/],
      [{ modelIdleMinutes: "0" }, /空闲.*分钟/],
      [{ executeTotalMinutes: "1441" }, /执行回合总时长.*1440/],
      [{ discussToolMinutes: "abc" }, /讨论工具/],
    ];
    for (const [patch, pattern] of cases) {
      const result = fromForm({ ...form, limits: { ...form.limits, ...patch } });
      expect(result.ok, JSON.stringify(patch)).toBe(false);
      expect(!result.ok && result.error).toMatch(pattern);
    }
    expect(fromForm({ ...form, limits: { ...form.limits, reworksPerTask: "0" } }).ok).toBe(true);
  });

  it("保存后重开：从保存值再生成的表单与提交前一致，脏检查只看有效值", () => {
    const form = { ...toForm(saved), assistantCli: "grok", limits: { ...toForm(saved).limits, executeTotalMinutes: "45" } };
    const result = fromForm(form);
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(toForm(result.settings)).toEqual(form);
    expect(isDirty(saved, form)).toBe(true);
    expect(isDirty(saved, toForm(saved))).toBe(false);
    expect(isDirty(saved, { ...toForm(saved), limits: { ...toForm(saved).limits, modelIdleMinutes: "5.0" } })).toBe(false);
  });

  it("助理选项首项是自动选择，其余按接入顺序标注安装与启用状态，未知值保留且不列出未接入的 CLI", () => {
    const configs = [config("claude", true, false), config("codex", false), config("grok", true), config("dsh", true)];
    const options = assistantOptions(clis, configs, "");
    expect(options[0]).toMatchObject({ value: "", label: expect.stringContaining("自动选择") });
    expect(options.slice(1).map((option) => option.value)).toEqual(["claude", "codex", "grok", "dsh"]);
    expect(options[1].label).toMatch(/Claude Code.*未安装/);
    expect(options[2].label).toMatch(/Codex.*已安装.*已关闭/);
    expect(options[3].label).toMatch(/Grok.*已安装.*已启用/);
    expect(assistantOptions(clis, configs, "gemini").slice(-1)[0]).toMatchObject({ value: "gemini", label: expect.stringContaining("未知") });
    // 读不到应用管理配置时，只按安装状态标注。
    expect(assistantOptions(clis, null, "")[2].label).toMatch(/Codex.*已安装$/);
  });

  it("提示说明自动选择会用谁、显式选择为何会失败", () => {
    const configs = [config("claude", true, false), config("codex", false), config("grok", true), config("dsh", true)];
    expect(assistantNotice(clis, configs, "")).toEqual({ tone: "info", text: expect.stringContaining("Grok") });
    expect(assistantNotice(clis, configs, "claude")).toEqual({ tone: "warning", text: expect.stringMatching(/未安装.*应用管理/) });
    expect(assistantNotice(clis, configs, "codex")).toEqual({ tone: "warning", text: expect.stringMatching(/已关闭.*应用管理/) });
    expect(assistantNotice(clis, configs, "dsh")).toEqual({ tone: "info", text: expect.stringContaining("DeepSeek Harness") });
    const none = [config("claude", true, false), config("codex", false), config("grok", false), config("dsh", false)];
    expect(assistantNotice(clis, none, "")).toEqual({ tone: "warning", text: expect.stringMatching(/没有.*可用/) });
  });

  it("面板状态：加载失败可见；保存失败保留草稿和原设置；保存成功后用后端返回值刷新", () => {
    expect(initialPanel).toEqual({ phase: "loading" });
    expect(panelReducer(initialPanel, { type: "loadFailed", error: "设置损坏" })).toEqual({ phase: "load_failed", error: "设置损坏" });
    let state = panelReducer(initialPanel, { type: "loaded", settings: saved });
    expect(state).toMatchObject({ phase: "ready", saved, form: toForm(saved), error: null, saving: false, savedTick: 0 });
    const draft = { ...toForm(saved), limits: { ...toForm(saved).limits, discussRounds: "0" } };
    state = panelReducer(state, { type: "edit", form: draft });
    state = panelReducer(state, { type: "saveStarted" });
    expect(state).toMatchObject({ saving: true, error: null });
    state = panelReducer(state, { type: "saveFailed", error: "讨论轮数上限须在 1 到 10 之间" });
    expect(state).toMatchObject({ phase: "ready", saving: false, error: "讨论轮数上限须在 1 到 10 之间", form: draft, saved });
    const next: RuntimeSettings = { assistantCli: "codex", limits: { ...saved.limits, discussRounds: 4 } };
    state = panelReducer(state, { type: "saved", settings: next });
    expect(state).toMatchObject({ phase: "ready", saving: false, error: null, saved: next, form: toForm(next), savedTick: 1 });
    // 加载失败或仍在加载时，编辑类动作不改变状态。
    expect(panelReducer(initialPanel, { type: "edit", form: draft })).toEqual(initialPanel);
  });
});
