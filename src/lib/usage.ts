import type { ProjectUsage, UsageSummary, UsageTotal } from "../types";

/** 用量区域的口径说明，与后端 `UsageSummary` 的注释一致。 */
export const USAGE_NOTE = "只汇总 CLI 回报并保存在回合记录里的用量，不读 CLI 日志；输入、缓存读取、缓存写入分列，不重复计入。不设费用硬上限，也不按 token 估算未报告的费用。";

export interface UsageField { key: keyof Omit<UsageSummary, "runs" | "okRuns" | "usageRuns">; label: string; kind: "tokens" | "usd" }

export const USAGE_FIELDS: UsageField[] = [
  { key: "inputTokens", label: "输入", kind: "tokens" },
  { key: "outputTokens", label: "输出", kind: "tokens" },
  { key: "cacheReadInputTokens", label: "缓存读取", kind: "tokens" },
  { key: "cacheCreationInputTokens", label: "缓存写入", kind: "tokens" },
  { key: "costUsd", label: "费用", kind: "usd" },
];

export function formatTokens(value: number): string {
  return Math.round(value).toLocaleString("en-US");
}

/** 美元金额：至少两位小数，小于一分钱时保留到第二位有效数字，便于看清小额费用。 */
export function formatUsd(value: number): string {
  if (value > 0 && value < 0.01) return `$${value.toPrecision(2)}`;
  return `$${value.toFixed(2)}`;
}

export interface UsageCell {
  text: string;
  /** full：每个已结束回合都报告了这项；partial：只有部分回合报告；none：没有证据。 */
  coverage: "full" | "partial" | "none";
  note: string | null;
}

/** 一项字段的展示。没有证据不显示数值；部分回合报告时标注这是已报告的合计。 */
export function usageCell(total: UsageTotal, summary: UsageSummary, kind: "tokens" | "usd"): UsageCell {
  if (total.reported === 0) return { text: "未报告", coverage: "none", note: null };
  const text = kind === "usd" ? formatUsd(total.total) : formatTokens(total.total);
  if (total.reported < summary.runs) return { text, coverage: "partial", note: `已报告的合计 · ${total.reported}/${summary.runs} 回合` };
  return { text, coverage: "full", note: null };
}

/** 回合覆盖范围。失败、停止、中断或没保存用量的回合都让覆盖不足。 */
export function describeCoverage(summary: UsageSummary): { text: string; insufficient: boolean } {
  if (summary.runs === 0) return { text: "还没有已结束的回合", insufficient: false };
  const insufficient = summary.usageRuns < summary.runs;
  const text = `${summary.runs} 回合 · ${summary.okRuns} 成功 · ${summary.usageRuns} 有用量${insufficient ? " · 覆盖不足" : ""}`;
  return { text, insufficient };
}

export interface UsageState {
  projectId: string | null;
  /** 最近一次请求的序号；只有同项目、同序号的响应才会被采用。 */
  seq: number;
  usage: ProjectUsage | null;
  error: string | null;
  loading: boolean;
}

export type UsageAction =
  | { type: "request"; projectId: string; seq: number }
  | { type: "loaded"; projectId: string; seq: number; usage: ProjectUsage }
  | { type: "failed"; projectId: string; seq: number; error: string };

export const initialUsage: UsageState = { projectId: null, seq: 0, usage: null, error: null, loading: false };

/** 切换项目时清空旧数值并丢弃迟到的响应；同项目刷新时保留旧数值直到新结果到达。 */
export function usageReducer(state: UsageState, action: UsageAction): UsageState {
  if (action.type === "request") {
    const sameProject = action.projectId === state.projectId;
    return { projectId: action.projectId, seq: action.seq, usage: sameProject ? state.usage : null, error: null, loading: true };
  }
  if (action.projectId !== state.projectId || action.seq !== state.seq) return state;
  if (action.type === "loaded") return { ...state, usage: action.usage, error: null, loading: false };
  return { ...state, error: action.error, loading: false };
}
