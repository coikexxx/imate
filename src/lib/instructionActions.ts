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

/** 任务行只展示标题、成员名和状态。找不到成员时不留空。 */
export function taskRowLabel(
  task: { title: string; assigneeAgentId: string | null; status: string },
  agents: { id: string; name: string }[],
): { title: string; member: string; status: string } {
  const member = agents.find((agent) => agent.id === task.assigneeAgentId)?.name ?? "未指定";
  return { title: task.title, member, status: TASK_STATUS_LABELS[task.status] ?? task.status };
}
