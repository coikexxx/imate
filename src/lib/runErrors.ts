/**
 * 回合终态与等待原因的界面文案。只按后端已给出的结构化状态和错误前缀归类；
 * `detail` 原样展示后端已脱敏的诊断，不解析、不改写、不显示原始 stderr。
 */
export type RunNoticeKind = "stopped" | "timeout" | "cli_unavailable" | "blocked" | "failed" | "cleanup" | "turn_limit";

export interface RunNotice {
  kind: RunNoticeKind;
  /** 界面短提示，与 `runs.error` 区分。 */
  title: string;
  /** 后端给出的说明，可能为空。 */
  detail: string | null;
  /** 用户下一步能做什么。 */
  next: string;
}

const CLI_UNAVAILABLE = /^CLI 不可用|未找到已安装|未安装|已在应用管理中关闭|尚未接入|未知的助理 CLI|成员不存在/;
const BLOCKED = /权限受阻|权限不足/;

const NEXT = {
  stopped: "部分文件可能已改动，不会自动撤销。检查后可以继续发指令。",
  timeout: "可在系统设置 → 运行设置里放宽空闲、工具或总时长阈值，再重试或补充说明。",
  cli_unavailable: "到系统设置 → 应用管理检查该 CLI 的安装、开关与路径，或为成员改选其他 CLI。",
  blocked: "等待授权后重跑，或补充说明、放弃；未经授权不会按成功处理。",
  failed: "可以重试，或补充说明后继续；执行阶段已改的文件不会自动撤销。",
  cleanup: "请先在终端或任务管理器里确认该目录相关的残留 CLI 进程已停止，再点击解除阻塞。",
  turn_limit: "补充说明可以继续这条指令，或放弃后发新指令；回合数不会清零。",
};

function classifyFailure(error: string | null): RunNoticeKind {
  if (!error) return "failed";
  if (CLI_UNAVAILABLE.test(error)) return "cli_unavailable";
  if (BLOCKED.test(error)) return "blocked";
  return "failed";
}

const TITLES: Record<RunNoticeKind, string> = {
  stopped: "已停止", timeout: "回合超时", cli_unavailable: "CLI 不可用", blocked: "权限受阻",
  failed: "回合失败", cleanup: "等待清理", turn_limit: "已达回合上限",
};

function notice(kind: RunNoticeKind, detail: string | null): RunNotice {
  return { kind, title: TITLES[kind], detail, next: NEXT[kind] };
}

/** `run:finished` 的终态。成功不留提醒，哪怕它是自动重开后的成功。 */
export function describeRunFinished(status: "ok" | "failed" | "timeout" | "cancelled", error: string | null): RunNotice | null {
  switch (status) {
    case "ok": return null;
    case "cancelled": return notice("stopped", error);
    case "timeout": return notice("timeout", error);
    case "failed": return notice(classifyFailure(error), error);
  }
}

export function describeCleanup(folderPath: string): RunNotice {
  return notice("cleanup", folderPath);
}

/** 指令等待用户时的原因。 */
export function describeWaitReason(reason: string | null | undefined): RunNotice | null {
  if (!reason) return null;
  if (/回合上限/.test(reason)) return notice("turn_limit", reason);
  if (/^已停止/.test(reason)) return notice("stopped", reason);
  if (/超时|超过时限|超过总时长|长时间没有输出/.test(reason)) return notice("timeout", reason);
  return notice(classifyFailure(reason), reason);
}
