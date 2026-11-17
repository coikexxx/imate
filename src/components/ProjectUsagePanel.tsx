import { useEffect, useReducer, useRef, useState } from "react";
import { api } from "../api";
import { describeCoverage, initialUsage, USAGE_FIELDS, USAGE_NOTE, usageCell, usageReducer } from "../lib/usage";
import type { CliInfo, UsageSummary } from "../types";
import { Button } from "./ui";

interface Props {
  projectId: string;
  clis: CliInfo[];
  /** 每次回合结束后加一，触发重新读取。 */
  refreshTick: number;
}

/** 项目页「用量」区域：按成员与项目合计已结束回合的用量，并写明覆盖范围。 */
export function ProjectUsagePanel({ projectId, clis, refreshTick }: Props) {
  const [state, dispatch] = useReducer(usageReducer, initialUsage);
  const [retryTick, setRetryTick] = useState(0);
  const seqRef = useRef(0);

  useEffect(() => {
    // 切换项目、回合结束或点击重试都重新读取；旧响应由 reducer 按项目与序号丢弃。
    const seq = ++seqRef.current;
    dispatch({ type: "request", projectId, seq });
    api.getProjectUsage(projectId)
      .then((usage) => dispatch({ type: "loaded", projectId, seq, usage }))
      .catch((failure) => dispatch({ type: "failed", projectId, seq, error: String(failure) }));
  }, [projectId, refreshTick, retryTick]);

  const labelOf = (cliId: string) => clis.find((cli) => cli.id === cliId)?.label ?? cliId;
  const { usage, error, loading } = state;

  return <div className="space-y-4 p-4">
    <p className="text-xs leading-5 text-slate-500">{USAGE_NOTE}</p>
    {error && <div className="space-y-2">
      <p role="alert" className="text-sm text-red-300">{error}</p>
      <Button onClick={() => setRetryTick((n) => n + 1)}>重试</Button>
    </div>}
    {!usage && !error && <p className="text-sm text-slate-400">正在读取用量…</p>}
    {usage && <div className="overflow-x-auto">
      <table className="w-full text-sm">
        <thead>
          <tr className="text-left text-xs text-slate-500">
            <th scope="col" className="py-2 pr-3 font-normal">成员</th>
            <th scope="col" className="py-2 pr-3 font-normal">回合覆盖</th>
            {USAGE_FIELDS.map((field) => <th key={field.key} scope="col" className="py-2 pr-3 text-right font-normal">{field.label}</th>)}
          </tr>
        </thead>
        <tbody>
          <UsageRow name="项目合计" detail={loading ? "正在刷新…" : `${usage.members.length} 名成员`} summary={usage.total} emphasize />
          {usage.members.map((member) => <UsageRow key={member.agentId} name={member.name}
            detail={member.isLeader ? `组长 · ${labelOf(member.cli)}` : labelOf(member.cli)} summary={member.summary} />)}
        </tbody>
      </table>
    </div>}
  </div>;
}

function UsageRow({ name, detail, summary, emphasize = false }: { name: string; detail: string; summary: UsageSummary; emphasize?: boolean }) {
  const coverage = describeCoverage(summary);
  return <tr className={`border-t border-slate-800/60 align-top ${emphasize ? "font-medium" : ""}`}>
    <th scope="row" className="py-2 pr-3 text-left">
      <span className="block">{name}</span>
      <span className="block text-xs font-normal text-slate-500">{detail}</span>
    </th>
    <td className={`py-2 pr-3 text-xs ${coverage.insufficient ? "text-amber-300" : "text-slate-500"}`}>{coverage.text}</td>
    {USAGE_FIELDS.map((field) => {
      const cell = usageCell(summary[field.key], summary, field.kind);
      return <td key={field.key} className={`py-2 pr-3 text-right font-mono ${cell.coverage === "none" ? "font-normal text-slate-500" : ""}`}>
        <span className="block">{cell.text}</span>
        {cell.note && <span className="block text-[11px] font-normal text-amber-300">{cell.note}</span>}
      </td>;
    })}
  </tr>;
}
