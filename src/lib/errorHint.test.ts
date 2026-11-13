import { describe, expect, it } from "vitest";
import { errorHint } from "./errorHint";

describe("运行失败提示", () => {
  it("要求开启思考的 400 指向推理强度与 Dsh 配置", () => {
    const hint = errorHint('dsh 回合失败（INVALID_REQUEST）：400 {"error":{"message":"claude-opus-5-5 requires adaptive thinking; omit thinking or use thinking.type=adaptive","type":"invalid_request_error"}}');
    expect(hint).toContain("推理强度");
    expect(hint).toContain("forceAdaptiveThinking");
  });

  it("推理强度不被支持时让用户改选", () => {
    expect(errorHint('dsh: UNSUPPORTED_REASONING_EFFORT: provider "claude" model "m" does not support reasoning effort "high"')).toContain("改成列表里有的值");
  });

  it("中转站没有该模型时提示换模型", () => {
    expect(errorHint('404 {"error":{"message":"Model \\"m\\" is not supported by any configured account in this group","type":"model_not_found"}}')).toContain("没有开通");
  });

  it("其他 400 参数错误给通用指引，认不出的错误不给提示", () => {
    expect(errorHint('dsh 回合失败（INVALID_REQUEST）：400 {"error":{"message":"bad field","type":"invalid_request_error"}}')).toContain("compat");
    expect(errorHint("回合超时")).toBeNull();
    expect(errorHint(null)).toBeNull();
    expect(errorHint("")).toBeNull();
  });
});
