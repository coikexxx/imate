import { describe, expect, it } from "vitest";
import { instructionBarState, taskRowLabel } from "./instructionActions";

describe("指令等待操作", () => {
  it("等待用户时三个操作可用，补充有内容才会提交", () => {
    const state = instructionBarState({
      projectStatus: "waiting_user",
      instructionStatus: "waiting_user",
      supplement: "改用别的方案",
    });
    expect(state.showActions).toBe(true);
    expect(state.submitSupplement).toBe(true);
  });

  it("运行中不显示重试、补充和放弃", () => {
    for (const projectStatus of ["discussing", "executing"]) {
      const state = instructionBarState({
        projectStatus,
        instructionStatus: "running",
        supplement: "仍在跑",
      });
      expect(state.showActions).toBe(false);
      expect(state.submitSupplement).toBe(false);
    }
    const waitingProject = instructionBarState({
      projectStatus: "waiting_user",
      instructionStatus: "running",
      supplement: "指令还在跑",
    });
    expect(waitingProject.showActions).toBe(false);
  });

  it("补充内容为空时不调用", () => {
    for (const supplement of ["", "   ", "\n"]) {
      const state = instructionBarState({
        projectStatus: "waiting_user",
        instructionStatus: "waiting_user",
        supplement,
      });
      expect(state.showActions).toBe(true);
      expect(state.submitSupplement).toBe(false);
    }
  });

  it("任务行显示标题、成员和状态", () => {
    const row = taskRowLabel(
      { title: "改按钮", assigneeAgentId: "front", status: "failed" },
      [{ id: "front", name: "前端" }],
    );
    expect(row).toEqual({ title: "改按钮", member: "前端", status: "失败" });
    expect(taskRowLabel({ title: "无人认领", assigneeAgentId: null, status: "pending" }, []).member).toBe("未指定");
    expect(taskRowLabel({ title: "已完成", assigneeAgentId: "front", status: "done" }, [{ id: "front", name: "前端" }]).status).toBe("已完成");
  });
});
