import { errorHint } from "../../lib/errorHint";
import { instructionBarState } from "../../lib/instructionActions";
import { describeWaitReason } from "../../lib/runErrors";
import type { Instruction, TaskItem } from "../../types";
import { Button, buttonClass, inputClass } from "../ui/ui";

interface Props {
  projectStatus: string;
  instruction: Instruction | null;
  tasks: TaskItem[];
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
  projectStatus,
  instruction,
  tasks,
  supplement,
  busy,
  cleanupRequired,
  onSupplementChange,
  onRetry,
  onSupplement,
  onAbandon,
  onAuthorizeElevated,
}: Props) {
  const actions = instructionBarState({
    projectStatus,
    instructionStatus: instruction?.status ?? null,
    supplement,
  });
  const blocked = tasks.filter((task) => task.status === "blocked");
  const canAuthorize = blocked.length > 0 && instruction?.status === "waiting_user";
  const wait = describeWaitReason(instruction?.waitReason);

  if (!canAuthorize && !actions.showActions) return null;

  if (wait?.kind === "turn_limit") {
    return (
      <div className="border-t border-warn-border bg-warn-soft/70 px-4 py-3">
        <div className="space-y-2.5">
          <div className="text-xs leading-5 text-warn">
            <p>
              <strong>{wait.title}</strong>
              <span className="ml-2 break-words">{wait.detail}</span>
            </p>
            <p className="mt-0.5 opacity-90">{wait.next}</p>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <a className={buttonClass("ghost")} href="#/settings?tab=runtime">调整上限</a>
            <Button variant="danger" disabled={busy} onClick={onAbandon}>放弃</Button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="border-t border-warn-border bg-warn-soft/50 px-4 py-3">
      {canAuthorize && (
        <div className="mb-2.5 space-y-2 rounded-xl border border-warn-border bg-surface p-3 text-xs text-warn">
          {blocked.map((task) => (
            <div key={task.id} className="flex flex-wrap items-center justify-between gap-2">
              <p className="min-w-0 flex-1 leading-5">
                任务「{task.title}」因权限受阻未完成。单次授权仅对本次重试有效，完成后恢复原档位。
              </p>
              <Button variant="primary" disabled={busy || cleanupRequired} onClick={() => onAuthorizeElevated(task.id)}>
                授权并继续
              </Button>
            </div>
          ))}
        </div>
      )}

      {actions.showActions && (
        <div className="space-y-2.5">
          {wait && (
            <div className="text-xs leading-5 text-warn">
              <p>
                <strong>{wait.title}</strong>
                {wait.detail && <span className="ml-2 break-words">{wait.detail}</span>}
              </p>
              {!canAuthorize && <p className="mt-0.5 opacity-90">{wait.next}</p>}
            </div>
          )}
          {errorHint(instruction?.waitReason) && (
            <p className="rounded-lg border border-warn-border bg-surface px-3 py-2 text-xs leading-5 text-warn">
              提示：{errorHint(instruction?.waitReason)}
            </p>
          )}
          <div className="flex flex-wrap items-center gap-2">
            <input
              className={`${inputClass} flex-1`}
              aria-label="补充内容"
              placeholder="补充后续要求，交给组长继续研判"
              value={supplement}
              disabled={busy}
              onChange={(event) => onSupplementChange(event.target.value)}
            />
            <Button variant="primary" disabled={busy || cleanupRequired || !actions.submitSupplement} onClick={onSupplement}>
              补充
            </Button>
            {!canAuthorize && (
              <Button disabled={busy || cleanupRequired} onClick={onRetry}>重试</Button>
            )}
            <Button variant="danger" disabled={busy} onClick={onAbandon}>放弃</Button>
          </div>
        </div>
      )}
    </div>
  );
}
