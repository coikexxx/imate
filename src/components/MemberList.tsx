import { MAX_AGENTS } from "../lib/wizard";
import type { Agent, AgentStatus, CliInfo } from "../types";
import { Button } from "./ui";
import { shirtColor } from "../office/layout";

interface Props {
  agents: Agent[];
  clis: CliInfo[];
  onAdd: () => void;
  statuses?: Record<string, AgentStatus>;
  onMention?: (agent: Agent) => void;
  onEdit: (agent: Agent) => void;
}

export function MemberList({ agents, clis, onAdd, onEdit, statuses = {}, onMention }: Props) {
  const labelOf = (cliId: string) => clis.find((c) => c.id === cliId)?.label ?? cliId;

  return (
    <aside className="flex h-full flex-col border-r border-slate-800">
      <h2 className="flex items-center justify-between px-4 py-4 text-sm font-medium">
        成员 <span className="font-mono text-xs tabular-nums text-slate-500">{agents.length}/{MAX_AGENTS}</span>
      </h2>
      <ul className="flex-1 space-y-2 overflow-y-auto px-2">
        {agents.map((agent) => (
          <li key={agent.id} className="rounded-lg border border-transparent hover:border-slate-800 hover:bg-slate-900">
            <button
              type="button"
              onClick={() => onEdit(agent)}
              className="w-full rounded-lg px-3 py-3 text-left"
            >
              <div className="flex items-center gap-2 text-sm">
                <span aria-hidden className="h-3 w-1 shrink-0 rounded-sm" style={{ backgroundColor: shirtColor(agent.id) }} />
                <span className="truncate font-medium">{agent.name}</span>
                {agent.isLeader && (
                  <span className="ml-auto shrink-0 text-xs text-slate-400">组长</span>
                )}
              </div>
              <div className="mt-1 truncate text-xs text-slate-400">
                {agent.isLeader && agent.role === "组长" ? labelOf(agent.cli) : `${agent.role} · ${labelOf(agent.cli)}`}
              </div>
              <div className={`mt-1 text-xs ${statuses[agent.id] === "error" ? "text-red-300" : !statuses[agent.id] || statuses[agent.id] === "idle" ? "text-slate-500" : "text-sky-200"}`}>{({ idle: "空闲", thinking: "思考中", talking: "发言中", working: "工具运行中", error: "执行出错" })[statuses[agent.id] ?? "idle"]}</div>
            </button>
            {onMention && <button type="button" className="mb-2 ml-3 rounded px-2 py-1 text-xs text-slate-400 hover:bg-slate-800 hover:text-sky-200" onClick={() => onMention(agent)}>@ 点名</button>}
          </li>
        ))}
      </ul>
      <div className="border-t border-slate-800 p-3">
        <Button className="w-full" disabled={agents.length >= MAX_AGENTS} onClick={onAdd}>
          ＋ 添加成员
        </Button>
      </div>
    </aside>
  );
}
