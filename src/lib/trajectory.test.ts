import { describe, expect, it } from "vitest";
import { filterTrajectory, parseTaskRef, sortTrajectory, trajectorySummary } from "./trajectory";

const task = (id: string, status: string, orderIndex: number, createdAt = 0, title = id) => ({ id, title, status, orderIndex, createdAt });

describe("任务编号抽取", () => {
  it("认出冒号编号，半角全角都行", () => {
    expect(parseTaskRef("G0-PRD: 产品需求与需求追溯表")).toEqual({ ref: "G0-PRD", text: "产品需求与需求追溯表" });
    expect(parseTaskRef("G1-14：接入真实读通道")).toEqual({ ref: "G1-14", text: "接入真实读通道" });
  });

  it("认出含数字的空格编号", () => {
    expect(parseTaskRef("G1-01 平台后端骨架")).toEqual({ ref: "G1-01", text: "平台后端骨架" });
  });

  it("正文里的冒号和普通词不当编号", () => {
    expect(parseTaskRef("12:30 开会")).toEqual({ ref: null, text: "12:30 开会" });
    expect(parseTaskRef("修复 v2 版本：兼容")).toEqual({ ref: null, text: "修复 v2 版本：兼容" });
    expect(parseTaskRef("TODO: 补测试")).toEqual({ ref: null, text: "TODO: 补测试" });
    expect(parseTaskRef("见 G1-01: 说明")).toEqual({ ref: null, text: "见 G1-01: 说明" });
  });

  it("编号后没有正文时保留原标题", () => {
    expect(parseTaskRef("G1-01: ")).toEqual({ ref: null, text: "G1-01: " });
    expect(parseTaskRef("")).toEqual({ ref: null, text: "" });
  });
});

describe("轨迹概览", () => {
  it("按状态计数，未结束含执行中、未开始与受阻", () => {
    const summary = trajectorySummary([
      task("1", "done", 0), task("2", "done", 1), task("3", "running", 2),
      task("4", "pending", 3), task("5", "blocked", 4), task("6", "failed", 5), task("7", "cancelled", 6),
    ]);
    expect(summary).toMatchObject({
      total: 7, done: 2, running: 1, pending: 1, blocked: 1, failed: 1, cancelled: 1, open: 3,
    });
    expect(summary.donePercent).toBe(29);
  });

  it("没有任务时比例为零，不出现 NaN", () => {
    expect(trajectorySummary([])).toMatchObject({ total: 0, done: 0, open: 0, donePercent: 0 });
  });
});

describe("轨迹排序与筛选", () => {
  it("按分工顺序排，同序再按创建时间与 id，且不改入参", () => {
    const input = [task("c", "done", 2), task("b", "done", 1, 9), task("a", "done", 1, 9), task("d", "done", 1, 3)];
    const sorted = sortTrajectory(input);
    expect(sorted.map((item) => item.id)).toEqual(["d", "a", "b", "c"]);
    expect(input.map((item) => item.id)).toEqual(["c", "b", "a", "d"]);
  });

  it("只看未结束时保留执行中、未开始与受阻", () => {
    const tasks = ["done", "running", "pending", "blocked", "failed", "cancelled"].map((status, index) => task(String(index), status, index));
    expect(filterTrajectory(tasks, false)).toHaveLength(6);
    expect(filterTrajectory(tasks, true).map((item) => item.status)).toEqual(["running", "pending", "blocked"]);
  });
});
