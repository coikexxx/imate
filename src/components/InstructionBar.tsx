import type { Agent, Instruction, TaskItem } from "../types";
import { Button, inputClass } from "./ui";
import { instructionBarState, taskRowLabel } from "../lib/instructionActions";

interface Props {
  projectStatus: string;
  instruction: Instruction | null;
  tasks: TaskItem[];
  agents: Agent[];
  supplement: string;
  busy: boolean;
  onSupplementChange: (value: string) => void;
  onRetry: () => void;
  onSupplement: () => void;
  onAbandon: () => void;
}

export function InstructionBar({
  projectStatus, instruction, tasks, agents, supplement, busy,
  onSupplementChange, onRetry, onSupplement, onAbandon,
}: Props) {
  const actions = instructionBarState({
    projectStatus,
    instructionStatus: instruction?.status ?? null,
    supplement,
  });
  if (!instruction && tasks.length === 0) return null;
  return <div className="border-t border-slate-800 px-3 py-2">
    {tasks.length > 0 && <ul aria-label="任务列表" className="mb-2 space-y-1 text-xs text-slate-300">
      {tasks.map((task) => {
        const row = taskRowLabel(task, agents);
        return <li key={task.id}>{row.title} · {row.member} · {row.status}</li>;
      })}
    </ul>}
    {actions.showActions && <div className="space-y-2">
      {instruction?.waitReason && <p className="text-xs text-amber-200">{instruction.waitReason}</p>}
      <div className="flex flex-wrap items-center gap-2">
        <Button disabled={busy} onClick={onRetry}>重试</Button>
        <Button variant="danger" disabled={busy} onClick={onAbandon}>放弃</Button>
      </div>
      <div className="flex items-center gap-2">
        <input className={inputClass} aria-label="补充内容" placeholder="补充后续要求" value={supplement}
          disabled={busy} onChange={(event) => onSupplementChange(event.target.value)} />
        <Button variant="primary" disabled={busy || !actions.submitSupplement} onClick={onSupplement}>补充</Button>
      </div>
    </div>}
  </div>;
}
