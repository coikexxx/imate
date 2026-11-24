import { MAX_AGENTS } from "../../lib/wizard";
import { shirtColor } from "../../office/layout";
import type { Agent, AgentStatus, CliInfo } from "../../types";
import { Icon } from "../ui/Icon";
import { Button } from "../ui/ui";

interface Props {
  agents: Agent[];
  clis: CliInfo[];
  onAdd: () => void;
  statuses?: Record<string, AgentStatus>;
  onMention?: (agent: Agent) => void;
  onEdit: (agent: Agent) => void;
}

const STATUS_META: Record<AgentStatus, { text: string; dot: string; color: string }> = {
  idle: { text: "空闲", dot: "bg-[#a5a7a0]", color: "text-ink-muted" },
  thinking: { text: "思考中", dot: "bg-warn", color: "text-warn font-medium" },
  talking: { text: "发言中", dot: "bg-warn", color: "text-warn font-medium" },
  working: { text: "执行中", dot: "bg-accent", color: "text-accent font-medium" },
  error: { text: "出错", dot: "bg-danger", color: "text-danger font-medium" },
};

export function MemberList({ agents, clis, onAdd, onEdit, statuses = {}, onMention }: Props) {
  const labelOf = (cliId: string) => clis.find((c) => c.id === cliId)?.label ?? cliId;

  return (
    <div className="studio-member-list">
      <p className="studio-member-hint">点击成员编辑人设与权限 · 点右侧按钮点名</p>
      <ul className="min-h-0 flex-1 space-y-1.5 overflow-y-auto px-3 py-1">
        {agents.map((agent) => {
          const status = statuses[agent.id] ?? "idle";
          const meta = STATUS_META[status];
          return (
            <li
              key={agent.id}
              className="flex items-center gap-1 rounded-xl border border-transparent bg-surface/70 p-1 hover:border-line hover:bg-surface"
            >
              <button
                type="button"
                onClick={() => onEdit(agent)}
                className="flex min-w-0 flex-1 items-center gap-2.5 rounded-lg px-2 py-1.5 text-left"
              >
                <span
                  className="studio-message-avatar shrink-0"
                  style={{ borderBottomColor: shirtColor(agent.id), background: "white" }}
                >
                  {agent.name.slice(0, 1)}
                </span>
                <span className="min-w-0 flex-1">
                  <span className="flex items-center gap-1.5 text-[13px] font-medium text-ink">
                    <span className="truncate">{agent.name}</span>
                    {agent.isLeader && (
                      <span className="shrink-0 rounded bg-accent-soft px-1.5 py-0.2 text-[10px] font-medium text-accent">
                        组长
                      </span>
                    )}
                    <span className={`ml-auto inline-flex shrink-0 items-center gap-1 text-[11px] font-normal ${meta.color}`}>
                      <span className={`size-1.5 rounded-full ${meta.dot}`} aria-hidden="true" />
                      {meta.text}
                    </span>
                  </span>
                  <span className="mt-0.5 block truncate text-[11px] text-ink-muted">
                    {agent.isLeader && agent.role === "组长" ? labelOf(agent.cli) : `${agent.role} · ${labelOf(agent.cli)}`}
                  </span>
                </span>
              </button>
              {onMention && (
                <button
                  type="button"
                  aria-label={`点名 ${agent.name}`}
                  title={`点名 ${agent.name}`}
                  className="grid size-7 shrink-0 place-items-center rounded-lg text-ink-muted hover:bg-surface-subtle hover:text-accent"
                  onClick={() => onMention(agent)}
                >
                  <Icon name="at" width="14" height="14" />
                </button>
              )}
            </li>
          );
        })}
      </ul>
      <div className="p-3">
        <Button className="w-full" disabled={agents.length >= MAX_AGENTS} onClick={onAdd}>
          <Icon name="plus" width="15" height="15" />添加成员
        </Button>
      </div>
    </div>
  );
}
