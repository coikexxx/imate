/** 等待用户时才显示重试、补充、放弃。补充内容去掉空白后仍为空则不提交。 */
export function instructionBarState(input: {
  projectStatus: string;
  instructionStatus: string | null;
  supplement: string;
}): { showActions: boolean; submitSupplement: boolean } {
  const showActions = input.projectStatus === "waiting_user" && input.instructionStatus === "waiting_user";
  return { showActions, submitSupplement: showActions && input.supplement.trim().length > 0 };
}

const TASK_STATUS_LABELS: Record<string, string> = {
  pending: "未开始",
  running: "执行中",
  done: "已完成",
  failed: "失败",
  cancelled: "已取消",
  blocked: "受阻：权限不足",
};

/** 状态文案唯一来源；轨迹面板与指令栏共用。未知状态原样显示。 */
export function taskStatusLabel(status: string): string {
  return TASK_STATUS_LABELS[status] ?? status;
}

const TASK_STATUS_SHORT: Record<string, string> = { blocked: "受阻" };

/** 概览计数等窄处用的短文案；没有单独的短文案时与完整文案一致。 */
export function taskStatusShortLabel(status: string): string {
  return TASK_STATUS_SHORT[status] ?? taskStatusLabel(status);
}

/** 任务行只展示标题、成员名和状态。找不到成员时不留空。 */
export function taskRowLabel(
  task: { title: string; assigneeAgentId: string | null; status: string },
  agents: { id: string; name: string }[],
): { title: string; member: string; status: string } {
  const member = agents.find((agent) => agent.id === task.assigneeAgentId)?.name ?? "未指定";
  return { title: task.title, member, status: taskStatusLabel(task.status) };
}
