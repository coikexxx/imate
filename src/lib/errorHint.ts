/** 运行失败文字对应的处理提示。认不出的错误返回 null，不乱猜。 */
export function errorHint(message: string | null | undefined): string | null {
  if (!message) return null;
  if (/UNSUPPORTED_REASONING_EFFORT|does not support reasoning effort/i.test(message)) {
    return "所选推理强度不被这个模型支持。到成员设置里把「推理强度」改成列表里有的值，或选「默认」。";
  }
  if (/model_not_found|not supported by any configured account/i.test(message)) {
    return "供应商或中转站没有开通这个模型。换一个模型，或检查中转站的分组和账号。";
  }
  if (/INVALID_REQUEST|invalid_request_error/i.test(message) && /thinking|adaptive/i.test(message)) {
    return "这个模型要求开启思考。到成员设置里为该成员选一个「推理强度」；"
      + "也可以在 Dsh 配置文件 cordis.patch.yml 里，给该模型加 compat.forceAdaptiveThinking: true，并给供应商设置默认的 reasoning。";
  }
  if (/INVALID_REQUEST|invalid_request_error/i.test(message)) {
    return "供应商拒绝了请求参数（400）。常见原因是中转站对这个模型有特殊要求，"
      + "请对照报错原文，在 Dsh 配置文件 cordis.patch.yml 里调整该模型的 compat 或 reasoningEfforts，保存后点重试。";
  }
  return null;
}
