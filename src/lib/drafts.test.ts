import { expect, it } from "vitest";
import { acceptDraftResponse, canApplyRules, canSendFeedback } from "./drafts";

it("只允许完整且未关闭的草稿应用，迟到响应不能复活旧对话", () => {
  const rules = [{ filename: "协作规范.md", content: "已经编辑的正文" }];
  expect(canApplyRules(rules, false, false)).toBe(true);
  expect(canApplyRules(rules, true, false)).toBe(false);
  expect(canApplyRules(rules, false, true)).toBe(false);
  expect(canApplyRules([], false, false)).toBe(false);
  expect(canSendFeedback("  ", false, false)).toBe(false);
  expect(canSendFeedback("继续精简", true, false)).toBe(false);
  expect(canSendFeedback("继续精简", false, true)).toBe(false);
  expect(canSendFeedback("继续精简", false, false)).toBe(true);
  expect(acceptDraftResponse("新草稿", "旧草稿", rules)).toBeNull();
  expect(acceptDraftResponse(null, "旧草稿", rules)).toBeNull();
  expect(acceptDraftResponse("当前草稿", "当前草稿", rules)).toEqual(rules);
});
