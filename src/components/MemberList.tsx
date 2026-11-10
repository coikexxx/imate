import { MAX_AGENTS } from "../lib/wizard";
import type { Agent, AgentStatus, CliInfo } from "../types";
import { Button } from "./ui";

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
      <h2 className="px-4 py-3 text-sm font-semibold">
        成员（{agents.length}/{MAX_AGENTS}）
      </h2>
      <ul className="flex-1 space-y-1 overflow-y-auto px-2">
        {agents.map((agent) => (
          <li key={agent.id}>
            <button
              type="button"
              onClick={() => onEdit(agent)}
              className="w-full rounded px-2 py-2 text-left hover:bg-slate-800"
            >
              <div className="flex items-center gap-2 text-sm">
                <span className="font-medium">{agent.name}</span>
                {agent.isLeader && (
                  <span className="rounded bg-amber-900/60 px-1.5 text-xs text-amber-200">组长</span>
                )}
              </div>
              <div className="text-xs text-slate-400">
                {agent.role} · {labelOf(agent.cli)}
              </div>
              <div className="text-xs text-sky-400">{({ idle: "空闲", thinking: "思考中", talking: "发言中", working: "工具运行中", error: "执行出错" })[statuses[agent.id] ?? "idle"]}</div>
            </button>
            {onMention && <button type="button" className="px-2 text-xs text-slate-400 hover:text-sky-300" onClick={() => onMention(agent)}>@ 点名</button>}
          </li>
        ))}
      </ul>
      <div className="p-2">
        <Button className="w-full" disabled={agents.length >= MAX_AGENTS} onClick={onAdd}>
          ＋ 添加成员
        </Button>
      </div>
    </aside>
  );
}
