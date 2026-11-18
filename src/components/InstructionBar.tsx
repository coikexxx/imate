import type { Agent, Instruction, TaskItem } from "../types";
import { Button, inputClass } from "./ui";
import { instructionBarState, taskRowLabel } from "../lib/instructionActions";
import { errorHint } from "../lib/errorHint";
import { describeWaitReason } from "../lib/runErrors";

interface Props {
  projectStatus: string;
  instruction: Instruction | null;
  tasks: TaskItem[];
  agents: Agent[];
  supplement: string;
  busy: boolean;
  cleanupRequired: boolean;
  onSupplementChange: (value: string) => void;
  onRetry: () => void;
  onSupplement: () => void;
  onAbandon: () => void;
  onAuthorizeElevated: (taskId: string) => void;
}

export function InstructionBar({
  projectStatus, instruction, tasks, agents, supplement, busy, cleanupRequired,
  onSupplementChange, onRetry, onSupplement, onAbandon, onAuthorizeElevated,
}: Props) {
  const actions = instructionBarState({
    projectStatus,
    instructionStatus: instruction?.status ?? null,
    supplement,
  });
  if (!instruction && tasks.length === 0) return null;
  const blocked = tasks.filter((task) => task.status === "blocked");
  const canAuthorize = blocked.length > 0 && instruction?.status === "waiting_user";
  const wait = describeWaitReason(instruction?.waitReason);
  return <div className="border-t border-slate-800 px-3 py-2">
    {tasks.length > 0 && <ul aria-label="任务列表" className="mb-2 space-y-1 text-xs text-slate-300">
      {tasks.map((task) => {
        const row = taskRowLabel(task, agents);
        return <li key={task.id} className={task.status === "blocked" ? "text-[#6b4a12]" : ""}>{row.title} · {row.member} · {row.status}</li>;
      })}
    </ul>}
    {canAuthorize && <div className="mb-2 space-y-2 rounded-xl border border-[#d8b878] bg-[#fbf3df] p-3 text-xs text-[#6b4a12]">
      {blocked.map((task) => <div key={task.id}>
        <p>任务「{task.title}」因权限受阻，未完成。授权只对这一次重试有效，用完恢复原档位。</p>
        <Button className="mt-2" variant="primary" disabled={busy || cleanupRequired} onClick={() => onAuthorizeElevated(task.id)}>授权并继续</Button>
      </div>)}
    </div>}
    {actions.showActions && <div className="space-y-2">
      {wait && <div className="text-xs text-[#6b4a12]">
        <p><strong>{wait.title}</strong>{wait.detail && <span className="ml-2 break-words">{wait.detail}</span>}</p>
        {!canAuthorize && <p className="mt-1 opacity-80">{wait.next}</p>}
      </div>}
      {errorHint(instruction?.waitReason) && <p className="rounded-lg border border-[#d8b878] bg-[#fbf3df] px-3 py-2 text-xs leading-5 text-[#6b4a12]">提示：{errorHint(instruction?.waitReason)}</p>}
      <div className="flex flex-wrap items-center gap-2">
        {!canAuthorize && <Button disabled={busy || cleanupRequired} onClick={onRetry}>重试</Button>}
        <Button variant="danger" disabled={busy} onClick={onAbandon}>放弃</Button>
      </div>
      <div className="flex items-center gap-2">
        <input className={inputClass} aria-label="补充内容" placeholder="补充后续要求" value={supplement}
          disabled={busy} onChange={(event) => onSupplementChange(event.target.value)} />
        <Button variant="primary" disabled={busy || cleanupRequired || !actions.submitSupplement} onClick={onSupplement}>补充</Button>
      </div>
    </div>}
  </div>;
}
