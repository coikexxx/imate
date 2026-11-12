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
  const reported = info.reportedModel?.trim();
  if (reported) return displayModel(reported);
  const requested = info.requestedModel?.trim();
  if (requested) return `${displayModel(requested)}（请求值，CLI 未回报）`;
  return "CLI 默认模型（未回报）";
}
