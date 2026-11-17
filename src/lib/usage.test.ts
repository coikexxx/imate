import { describe, expect, it } from "vitest";
import { describeCoverage, formatTokens, formatUsd, initialUsage, USAGE_FIELDS, USAGE_NOTE, usageCell, usageReducer } from "./usage";
import type { ProjectUsage, UsageSummary, UsageTotal } from "../types";

const total = (value: number, reported: number): UsageTotal => ({ total: value, reported });
const summary = (runs: number, okRuns: number, usageRuns: number, patch: Partial<UsageSummary> = {}): UsageSummary => ({
  runs, okRuns, usageRuns,
  inputTokens: total(0, 0), outputTokens: total(0, 0), cacheReadInputTokens: total(0, 0), cacheCreationInputTokens: total(0, 0), costUsd: total(0, 0),
  ...patch,
});
const usage = (projectId: string): ProjectUsage => ({ projectId, members: [], total: summary(0, 0, 0) });

describe("项目用量展示", () => {
  it("字段分列展示，千分位与美元格式固定", () => {
    expect(USAGE_FIELDS.map((field) => field.key)).toEqual(["inputTokens", "outputTokens", "cacheReadInputTokens", "cacheCreationInputTokens", "costUsd"]);
    expect(USAGE_FIELDS.map((field) => field.label)).toEqual(["输入", "输出", "缓存读取", "缓存写入", "费用"]);
    expect(formatTokens(0)).toBe("0");
    expect(formatTokens(1234567)).toBe("1,234,567");
    expect(formatUsd(0)).toBe("$0.00");
    expect(formatUsd(1.25)).toBe("$1.25");
    expect(formatUsd(0.001234)).toBe("$0.0012");
    expect(formatUsd(12.3456789)).toBe("$12.35");
    // 口径说明：不设费用上限，也不按 token 估算未报告的费用。
    expect(USAGE_NOTE).toMatch(/不设费用.*上限/);
    expect(USAGE_NOTE).toMatch(/不按 token 估算/);
  });

  it("没有证据显示未报告，部分回合报告标注已报告的合计，全部报告不加标注", () => {
    const none = usageCell(total(999, 0), summary(3, 3, 0), "tokens");
    expect(none).toEqual({ text: "未报告", coverage: "none", note: null });
    const partial = usageCell(total(110, 2), summary(3, 2, 2), "tokens");
    expect(partial).toMatchObject({ text: "110", coverage: "partial" });
    expect(partial.note).toMatch(/已报告的合计/);
    expect(partial.note).toMatch(/2\/3/);
    expect(usageCell(total(110, 3), summary(3, 3, 3), "tokens")).toEqual({ text: "110", coverage: "full", note: null });
    // 费用只有有证据时才显示金额；报告为零也是证据。
    expect(usageCell(total(0, 0), summary(2, 2, 2), "usd")).toMatchObject({ text: "未报告", coverage: "none" });
    expect(usageCell(total(0, 2), summary(2, 2, 2), "usd")).toEqual({ text: "$0.00", coverage: "full", note: null });
    expect(usageCell(total(0.5, 1), summary(2, 2, 2), "usd")).toMatchObject({ text: "$0.50", coverage: "partial" });
  });

  it("覆盖范围写明回合、成功与有用量的数量，失败或中断让覆盖不足", () => {
    expect(describeCoverage(summary(0, 0, 0))).toEqual({ text: "还没有已结束的回合", insufficient: false });
    expect(describeCoverage(summary(3, 3, 3))).toEqual({ text: "3 回合 · 3 成功 · 3 有用量", insufficient: false });
    const partial = describeCoverage(summary(7, 5, 3));
    expect(partial.insufficient).toBe(true);
    expect(partial.text).toMatch(/^7 回合 · 5 成功 · 3 有用量/);
    expect(partial.text).toMatch(/覆盖不足/);
  });

  it("切换项目时丢弃旧响应；同项目刷新保留上次数值直到新结果到达", () => {
    expect(initialUsage).toEqual({ projectId: null, seq: 0, usage: null, error: null, loading: false });
    let state = usageReducer(initialUsage, { type: "request", projectId: "甲", seq: 1 });
    expect(state).toMatchObject({ projectId: "甲", seq: 1, usage: null, loading: true });
    state = usageReducer(state, { type: "loaded", projectId: "甲", seq: 1, usage: usage("甲") });
    expect(state).toMatchObject({ usage: usage("甲"), loading: false, error: null });
    // 同项目回合结束后刷新：请求期间仍显示旧数值。
    state = usageReducer(state, { type: "request", projectId: "甲", seq: 2 });
    expect(state).toMatchObject({ seq: 2, usage: usage("甲"), loading: true });
    // 切到乙：旧数值清空；甲的迟到响应（无论序号）被丢弃，乙自己的旧序号也被丢弃。
    state = usageReducer(state, { type: "request", projectId: "乙", seq: 3 });
    expect(state).toMatchObject({ projectId: "乙", seq: 3, usage: null, loading: true });
    expect(usageReducer(state, { type: "loaded", projectId: "甲", seq: 2, usage: usage("甲") })).toBe(state);
    expect(usageReducer(state, { type: "failed", projectId: "甲", seq: 2, error: "x" })).toBe(state);
    expect(usageReducer(state, { type: "loaded", projectId: "乙", seq: 2, usage: usage("乙") })).toBe(state);
    state = usageReducer(state, { type: "failed", projectId: "乙", seq: 3, error: "读取失败" });
    expect(state).toMatchObject({ projectId: "乙", usage: null, error: "读取失败", loading: false });
    state = usageReducer(state, { type: "request", projectId: "乙", seq: 4 });
    expect(state.error).toBeNull();
    state = usageReducer(state, { type: "loaded", projectId: "乙", seq: 4, usage: usage("乙") });
    expect(state).toMatchObject({ usage: usage("乙"), loading: false });
  });
});
