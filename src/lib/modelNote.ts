import type { ModelInfo } from "../types";

/** 群聊消息旁显示的模型说明；没有链接运行的旧消息返回 null（不显示）。 */
export function modelNote(info: ModelInfo | null): string | null {
  if (!info) return null;
  const reported = info.reportedModel?.trim();
  if (reported) return reported;
  const requested = info.requestedModel?.trim();
  if (requested) return `${requested}（请求值，CLI 未回报）`;
  return "CLI 默认模型（未回报）";
}
