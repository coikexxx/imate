import type { ModelInfo } from "../types";

/** Dsh 成员的模型值是 JSON（provider / model 选择），显示时转成「provider / model」；其他值原样返回。 */
function displayModel(value: string): string {
  try {
    const parsed: unknown = JSON.parse(value);
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      const { provider, model } = parsed as { provider?: unknown; model?: unknown };
      if (typeof provider === "string" && provider.trim() && typeof model === "string" && model.trim()) {
        return `${provider.trim()} / ${model.trim()}`;
      }
    }
  } catch {
    // 不是 JSON，按普通模型名处理。
  }
  return value;
}

/** 群聊消息旁显示的模型说明；没有链接运行的旧消息返回 null（不显示）。 */
export function modelNote(info: ModelInfo | null): string | null {
  if (!info) return null;
  const requested = info.requestedModel?.trim();
  if (info.runStatus !== "ok") {
    const reason = ({
      running: "运行中", failed: "运行失败", timeout: "运行超时", cancelled: "已取消", interrupted: "已中断",
    } as Record<string, string>)[info.runStatus ?? ""] ?? "运行状态未知";
    return requested
      ? displayModel(requested) + "（请求值，" + reason + "，未确认）"
      : "CLI 默认模型（" + reason + "，未确认）";
  }
  const reported = info.reportedModel?.trim();
  if (reported) return displayModel(reported);
  if (requested) return `${displayModel(requested)}（请求值，CLI 未回报）`;
  return "CLI 默认模型（未回报）";
}
