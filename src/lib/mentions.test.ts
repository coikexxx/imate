import { describe, expect, it } from "vitest";
import { parseMention, mentionCandidates, shouldSendOnEnter } from "./mentions";
import type { Agent } from "../types";

function agent(name: string): Agent {
  return { id: name, projectId: "项目甲", name, role: "开发", persona: "", personaDirty: false,
    cli: "claude", model: null, isLeader: false, sessionId: null, lastSeenMessageId: 0, seatIndex: 0, createdAt: 0 };
}

describe("成员点名", () => {
  it("按最长完整名称匹配并保留消息空格", () => {
    const members = [agent("Dev"), agent("Dev Ops")];
    expect(parseMention("  @dev ops  修复构建  ", members)).toEqual({ agent: members[1], message: "修复构建" });
    expect(parseMention("@Dev 项目", members)).toEqual({ agent: members[0], message: "项目" });
  });
  it("拒绝空消息、未知成员和名称前缀歧义", () => {
    const members = [agent("王"), agent("王五")];
    expect(parseMention("@未知 做事", members)).toMatchObject({ error: expect.any(String) });
    expect(parseMention("@王五项目 做事", members)).toMatchObject({ error: expect.any(String) });
    expect(parseMention("@王", members)).toMatchObject({ error: expect.any(String) });
    expect(parseMention("普通消息", members)).toMatchObject({ error: expect.any(String) });
  });
  it("普通消息路由到组长，Enter 不打断换行或中文输入法", () => {
    const leader = { ...agent("组长"), isLeader: true };
    expect(parseMention("普通任务", [leader])).toEqual({ agent: leader, message: "普通任务" });
    expect(shouldSendOnEnter({ key: "Enter", shiftKey: false, isComposing: false })).toBe(true);
    expect(shouldSendOnEnter({ key: "Enter", shiftKey: true, isComposing: false })).toBe(false);
    expect(shouldSendOnEnter({ key: "Enter", shiftKey: false, isComposing: true })).toBe(false);
    expect(shouldSendOnEnter({ key: "Enter", shiftKey: false, isComposing: false, keyCode: 229 })).toBe(false);
  });
  it("候选项按名称匹配且不返回当前项目之外成员", () => {
    const members = [agent("测试"), { ...agent("设计"), projectId: "项目乙" }];
    expect(mentionCandidates("@测", members, "项目甲").map((a) => a.name)).toEqual(["测试"]);
  });
});
