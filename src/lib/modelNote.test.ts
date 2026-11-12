import { describe, expect, it } from "vitest";
import { modelNote } from "./modelNote";

describe("modelNote", () => {
  it("没有链接运行的旧消息不显示", () => {
    expect(modelNote(null)).toBeNull();
  });
  it("CLI 回报了模型就直接显示回报值", () => {
    expect(modelNote({ requestedModel: null, reportedModel: "claude-opus-4-1" })).toBe("claude-opus-4-1");
  });
  it("只有请求值时标注 CLI 未回报", () => {
    expect(modelNote({ requestedModel: "gpt-5", reportedModel: null })).toBe("gpt-5（请求值，CLI 未回报）");
  });
  it("两者都没有时说明使用 CLI 默认模型", () => {
    expect(modelNote({ requestedModel: null, reportedModel: null })).toBe("CLI 默认模型（未回报）");
  });
  it("两者都有时优先显示回报值", () => {
    expect(modelNote({ requestedModel: "sonnet", reportedModel: "claude-sonnet-4-5-20250929" })).toBe("claude-sonnet-4-5-20250929");
  });
  it("Dsh 的 JSON 模型选择显示为 provider / model", () => {
    const dsh = JSON.stringify({ profile: "web", provider: "liberation", model: "deepseek-v4-1-flash-261017" });
    expect(modelNote({ requestedModel: dsh, reportedModel: null })).toBe("liberation / deepseek-v4-1-flash-261017（请求值，CLI 未回报）");
    expect(modelNote({ requestedModel: null, reportedModel: dsh })).toBe("liberation / deepseek-v4-1-flash-261017");
  });
  it("缺少 provider 或 model、非对象 JSON、坏 JSON、普通模型名原样显示", () => {
    for (const raw of [
      '{"provider":"gpt"}', '{"model":"gpt-6"}', '{"provider":"","model":"gpt-6"}', '{"provider":"gpt","model":5}',
      '"a"', '["gpt","gpt-6"]', "null", "{bad json", "gpt-5",
    ]) {
      expect(modelNote({ requestedModel: raw, reportedModel: null })).toBe(`${raw}（请求值，CLI 未回报）`);
      expect(modelNote({ requestedModel: null, reportedModel: raw })).toBe(raw);
    }
  });
  it("空字符串和全空白按没有处理", () => {
    expect(modelNote({ requestedModel: "  ", reportedModel: "" })).toBe("CLI 默认模型（未回报）");
    expect(modelNote({ requestedModel: "gpt-5", reportedModel: "   " })).toBe("gpt-5（请求值，CLI 未回报）");
  });
});
