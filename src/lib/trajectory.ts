/** 任务轨迹的纯函数：编号抽取、概览统计、排序与筛选。 */

type TaskLike = { id: string; title: string; status: string; orderIndex: number; createdAt: number };

/** 与后端状态一致；顺序用于概览计数与「未结束」判断。 */
export const OPEN_TASK_STATUSES = ["running", "pending", "blocked"] as const;

const COLON_REF = /^([A-Za-z][A-Za-z0-9._-]{0,23})\s*[:：]\s*/;
const SPACE_REF = /^([A-Za-z]{1,3}\d{1,3}-\d{1,3})\s+/;

/**
 * 从任务标题里抽出行首编号（组长常把编号写进标题）。
 * 只认带冒号的短编号，或 `G1-01 标题` 这类含数字的空格编号；
 * 正文里的冒号（如「12:30 开会」「修复 v2 版本：兼容」）不当作编号。
 */
export function parseTaskRef(title: string): { ref: string | null; text: string } {
  const colon = title.match(COLON_REF);
  const candidate = colon?.[1] ?? title.match(SPACE_REF)?.[1] ?? null;
  if (!candidate || !/[-\d]/.test(candidate)) return { ref: null, text: title };
  const text = title.slice((colon?.[0] ?? title.match(SPACE_REF)?.[0] ?? "").length);
  if (!text.trim()) return { ref: null, text: title };
  return { ref: candidate, text };
}

export interface TrajectorySummary {
  total: number;
  done: number;
  running: number;
  pending: number;
  blocked: number;
  failed: number;
  cancelled: number;
  /** 未结束：执行中 + 待开始 + 受阻。 */
  open: number;
  /** 已完成占比，四舍五入到整数；没有任务时为 0。 */
  donePercent: number;
}

export function trajectorySummary(tasks: Pick<TaskLike, "status">[]): TrajectorySummary {
  const count = (status: string) => tasks.filter((task) => task.status === status).length;
  const total = tasks.length;
  const done = count("done");
  const running = count("running");
  const pending = count("pending");
  const blocked = count("blocked");
  return {
    total,
    done,
    running,
    pending,
    blocked,
    failed: count("failed"),
    cancelled: count("cancelled"),
    open: running + pending + blocked,
    donePercent: total > 0 ? Math.round((done / total) * 100) : 0,
  };
}

/** 按分工顺序排；同序再按创建时间与 id，保证渲染稳定。不改入参。 */
export function sortTrajectory<T extends Pick<TaskLike, "orderIndex" | "createdAt" | "id">>(tasks: T[]): T[] {
  return [...tasks].sort((left, right) => left.orderIndex - right.orderIndex
    || left.createdAt - right.createdAt
    || left.id.localeCompare(right.id));
}

export function filterTrajectory<T extends Pick<TaskLike, "status">>(tasks: T[], onlyOpen: boolean): T[] {
  if (!onlyOpen) return tasks;
  return tasks.filter((task) => (OPEN_TASK_STATUSES as readonly string[]).includes(task.status));
}
