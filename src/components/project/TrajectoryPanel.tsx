import { useEffect, useRef, useState } from "react";
import { taskRowLabel, taskStatusShortLabel } from "../../lib/instructionActions";
import {
  filterTrajectory,
  OPEN_TASK_STATUSES,
  parseTaskRef,
  sortTrajectory,
  trajectorySummary,
} from "../../lib/trajectory";
import type { Agent, TaskItem } from "../../types";
import { Icon } from "../ui/Icon";

/** 节点与状态色的唯一映射；未知状态按待开始处理。 */
const TONE: Record<string, string> = {
  done: "done",
  running: "running",
  pending: "pending",
  blocked: "blocked",
  failed: "failed",
  cancelled: "cancelled",
};
const SUMMARY_ORDER = ["running", "pending", "blocked", "failed", "cancelled"] as const;

interface Props {
  tasks: TaskItem[];
  agents: Agent[];
}

/** 团队轨迹：概览 + 时间轴。默认展示全部任务，可只看未结束。 */
export function TrajectoryPanel({ tasks, agents }: Props) {
  const [onlyOpen, setOnlyOpen] = useState(false);
  const scroll = useRef<HTMLDivElement>(null);
  const current = useRef<HTMLLIElement>(null);
  const summary = trajectorySummary(tasks);
  const ordered = sortTrajectory(tasks);
  const visible = filterTrajectory(ordered, onlyOpen);
  const firstOpen = ordered.findIndex((task) => (OPEN_TASK_STATUSES as readonly string[]).includes(task.status));
  const anchorId = firstOpen > 0 ? ordered[firstOpen - 1].id : ordered[firstOpen]?.id ?? null;

  useEffect(() => {
    const box = scroll.current;
    const node = current.current;
    if (onlyOpen || !anchorId || !box || !node) return;
    box.scrollTop = Math.max(0, node.getBoundingClientRect().top - box.getBoundingClientRect().top + box.scrollTop - 8);
  }, [anchorId, onlyOpen]);

  const counts = SUMMARY_ORDER.map((status) => ({
    status,
    label: taskStatusShortLabel(status),
    value: summary[status],
  })).filter((item) => item.value > 0);

  return (
    <div className="studio-trajectory">
      {summary.total > 0 && (
        <div className="studio-trajectory-summary">
          <p className="studio-trajectory-ratio">
            <span>
              <strong>{summary.done}</strong>/{summary.total} 已完成
              <span className="studio-trajectory-percent">· {summary.donePercent}%</span>
            </span>
            <button
              type="button"
              className="studio-trajectory-toggle"
              aria-pressed={onlyOpen}
              onClick={() => setOnlyOpen(!onlyOpen)}
            >
              {onlyOpen ? "显示全部" : "只看未结束"}
            </button>
          </p>
          <div
            className="studio-trajectory-progress"
            role="progressbar"
            aria-label="任务完成进度"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={summary.donePercent}
          >
            <i style={{ width: "100%", transform: `scaleX(${summary.donePercent / 100})` }} />
          </div>
          {counts.length > 0 && (
            <ul className="studio-trajectory-counts">
              {counts.map((item) => (
                <li key={item.status} className={`is-${item.status}`}>
                  <i aria-hidden="true" />
                  {item.label}
                  <span>{item.value}</span>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}

      <div className="studio-trajectory-scroll" ref={scroll}>
        {visible.length === 0 ? (
          <div className="studio-trajectory-empty">
            <Icon name="trajectory" width="20" height="20" />
            <p>{summary.total === 0 ? "还没有任务" : "没有未结束的任务"}</p>
            <span>{summary.total === 0 ? "组长分工后，任务进度会显示在这里" : "本轮任务均已结束"}</span>
          </div>
        ) : (
          <ol className="studio-trajectory-list">
            {visible.map((task) => {
              const row = taskRowLabel(task, agents);
              const { ref, text } = parseTaskRef(row.title);
              const tone = TONE[task.status] ?? "pending";
              return (
                <li key={task.id} ref={task.id === anchorId ? current : undefined} className={`studio-trajectory-item is-${tone}`}>
                  <span className="studio-trajectory-node" aria-hidden="true" />
                  <div className="studio-trajectory-body">
                    <p className="studio-trajectory-title">
                      {ref && <span className="studio-trajectory-ref">{ref}</span>}
                      <span className="studio-trajectory-text">{text}</span>
                    </p>
                    <p className="studio-trajectory-meta">
                      <span>{row.member}</span>
                      <span aria-hidden="true">·</span>
                      <span className="studio-trajectory-status">{row.status}</span>
                    </p>
                  </div>
                </li>
              );
            })}
          </ol>
        )}
      </div>
    </div>
  );
}
