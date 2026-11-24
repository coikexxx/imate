import { useEffect, useReducer, useRef, useState } from "react";
import { api } from "../../api";
import { describeCoverage, initialUsage, USAGE_FIELDS, USAGE_NOTE, usageCell, usageReducer } from "../../lib/usage";
import type { CliInfo, UsageSummary } from "../../types";
import { Button } from "../ui/ui";

interface Props {
  projectId: string;
  clis: CliInfo[];
  /** 每次回合结束后加一，触发重新读取。 */
  refreshTick: number;
}

/** 项目页「用量」区域：顶部统计概览 + 成员明细表，标明覆盖证据范围。 */
export function ProjectUsagePanel({ projectId, clis, refreshTick }: Props) {
  const [state, dispatch] = useReducer(usageReducer, initialUsage);
  const [retryTick, setRetryTick] = useState(0);
  const seqRef = useRef(0);

  useEffect(() => {
    const seq = ++seqRef.current;
    dispatch({ type: "request", projectId, seq });
    api
      .getProjectUsage(projectId)
      .then((usage) => dispatch({ type: "loaded", projectId, seq, usage }))
      .catch((failure) => dispatch({ type: "failed", projectId, seq, error: String(failure) }));
  }, [projectId, refreshTick, retryTick]);

  const labelOf = (cliId: string) => clis.find((cli) => cli.id === cliId)?.label ?? cliId;
  const { usage, error, loading } = state;

  const inputStat = usage ? usageCell(usage.total.inputTokens, usage.total, "tokens") : null;
  const outputStat = usage ? usageCell(usage.total.outputTokens, usage.total, "tokens") : null;
  const costStat = usage ? usageCell(usage.total.costUsd, usage.total, "usd") : null;

  return (
    <div className="space-y-5 p-5">
      <p className="text-xs leading-5 text-ink-muted">{USAGE_NOTE}</p>

      {error && (
        <div className="space-y-2 rounded-lg border border-danger-border bg-danger-soft p-3.5">
          <p role="alert" className="text-sm text-danger">{error}</p>
          <Button onClick={() => setRetryTick((n) => n + 1)}>重试</Button>
        </div>
      )}

      {!usage && !error && <p className="text-sm text-ink-secondary">正在读取用量…</p>}

      {usage && (
        <>
          <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
            <div className="rounded-xl border border-line bg-paper/60 p-3.5">
              <span className="block text-xs text-ink-muted">已结束回合</span>
              <strong className="mt-1.5 block font-mono text-xl font-medium tabular-nums text-ink">
                {usage.total.okRuns} / {usage.total.runs}
              </strong>
              <span className="mt-1 block text-[11px] text-ink-muted">
                {usage.total.usageRuns} 回合含用量记录
              </span>
            </div>
            <div className="rounded-xl border border-line bg-paper/60 p-3.5">
              <span className="block text-xs text-ink-muted">累计输入 Token</span>
              <strong className="mt-1.5 block font-mono text-xl font-medium tabular-nums text-ink">
                {inputStat?.text}
              </strong>
              <span className="mt-1 block text-[11px] text-ink-muted">{inputStat?.note ?? "全部已报告"}</span>
            </div>
            <div className="rounded-xl border border-line bg-paper/60 p-3.5">
              <span className="block text-xs text-ink-muted">累计输出 Token</span>
              <strong className="mt-1.5 block font-mono text-xl font-medium tabular-nums text-ink">
                {outputStat?.text}
              </strong>
              <span className="mt-1 block text-[11px] text-ink-muted">{outputStat?.note ?? "全部已报告"}</span>
            </div>
            <div className="rounded-xl border border-line bg-paper/60 p-3.5">
              <span className="block text-xs text-ink-muted">已报告费用</span>
              <strong className="mt-1.5 block font-mono text-xl font-medium tabular-nums text-accent">
                {costStat?.text}
              </strong>
              <span className="mt-1 block text-[11px] text-ink-muted">{costStat?.note ?? "不估算未报告项"}</span>
            </div>
          </div>

          <div className="overflow-x-auto rounded-xl border border-line">
            <table className="w-full text-[13px]">
              <thead>
                <tr className="border-b border-line bg-paper/70 text-left text-xs text-ink-secondary">
                  <th scope="col" className="px-3.5 py-2.5 font-medium">成员</th>
                  <th scope="col" className="px-3 py-2.5 font-medium">回合覆盖</th>
                  {USAGE_FIELDS.map((field) => (
                    <th key={field.key} scope="col" className="px-3.5 py-2.5 text-right font-medium">
                      {field.label}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody className="divide-y divide-line">
                <UsageRow
                  name="项目合计"
                  detail={loading ? "正在刷新…" : `${usage.members.length} 名成员`}
                  summary={usage.total}
                  emphasize
                />
                {usage.members.map((member) => (
                  <UsageRow
                    key={member.agentId}
                    name={member.name}
                    detail={member.isLeader ? `组长 · ${labelOf(member.cli)}` : labelOf(member.cli)}
                    summary={member.summary}
                  />
                ))}
              </tbody>
            </table>
          </div>
        </>
      )}
    </div>
  );
}

function UsageRow({
  name,
  detail,
  summary,
  emphasize = false,
}: {
  name: string;
  detail: string;
  summary: UsageSummary;
  emphasize?: boolean;
}) {
  const coverage = describeCoverage(summary);
  return (
    <tr className={`align-top ${emphasize ? "bg-surface-subtle/60 font-medium" : "hover:bg-paper/50"}`}>
      <th scope="row" className="px-3.5 py-2.5 text-left">
        <span className="block text-ink">{name}</span>
        <span className="block text-xs font-normal text-ink-muted">{detail}</span>
      </th>
      <td className={`px-3 py-2.5 text-xs ${coverage.insufficient ? "text-warn" : "text-ink-secondary"}`}>
        {coverage.text}
      </td>
      {USAGE_FIELDS.map((field) => {
        const cell = usageCell(summary[field.key], summary, field.kind);
        return (
          <td
            key={field.key}
            className={`px-3.5 py-2.5 text-right font-mono tabular-nums ${
              cell.coverage === "none" ? "font-normal text-ink-muted" : "text-ink"
            }`}
          >
            <span className="block">{cell.text}</span>
            {cell.note && <span className="mt-0.5 block font-sans text-[11px] font-normal text-warn">{cell.note}</span>}
          </td>
        );
      })}
    </tr>
  );
}
