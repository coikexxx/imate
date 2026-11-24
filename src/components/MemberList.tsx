import { MAX_AGENTS } from "../lib/wizard";
import type { Agent, AgentStatus, CliInfo } from "../types";
import { Button } from "./ui";
import { shirtColor } from "../office/layout";
import { Icon } from "./Icon";

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
  return <div className="studio-member-list">
    <p className="studio-member-hint">点击成员编辑 · 行尾 ＠ 点名</p>
    <ul className="min-h-0 flex-1 space-y-1 overflow-y-auto px-3">
      {agents.map((agent) => {
        const status = statuses[agent.id] ?? "idle";
        return <li key={agent.id} className="flex items-center gap-1 rounded-lg border border-transparent p-1 hover:border-slate-700 hover:bg-white/70">
          <button type="button" onClick={() => onEdit(agent)} className="flex min-w-0 flex-1 items-center gap-3 rounded-lg p-2 text-left">
            <span className="studio-message-avatar shrink-0" style={{ borderBottomColor: shirtColor(agent.id), background: "white" }}>{agent.name.slice(0, 1)}</span>
            <span className="min-w-0 flex-1"><span className="flex items-center gap-2 text-sm font-medium"><span className="truncate">{agent.name}</span>{agent.isLeader && <span className="text-[10px] font-normal text-slate-500">组长</span>}</span><span className="mt-0.5 block truncate text-[11px] text-slate-500">{agent.isLeader && agent.role === "组长" ? labelOf(agent.cli) : `${agent.role} · ${labelOf(agent.cli)}`}</span><span className={`block text-[11px] ${status === "error" ? "text-red-400" : status === "idle" ? "text-slate-500" : "text-sky-300"}`}>{({idle:"空闲",thinking:"思考中",talking:"发言中",working:"工具运行中",error:"执行出错"})[status]}</span></span>
          </button>
          {onMention && <button type="button" aria-label={`点名 ${agent.name}`} title={`点名 ${agent.name}`} className="grid size-8 shrink-0 place-items-center rounded-lg text-slate-500 hover:bg-white hover:text-sky-300" onClick={() => onMention(agent)}>＠</button>}
        </li>;
      })}
    </ul>
    <div className="p-3"><Button className="w-full" disabled={agents.length >= MAX_AGENTS} onClick={onAdd}><Icon name="plus" width="16" height="16" />添加成员</Button></div>
  </div>;
}
