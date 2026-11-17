import { describe, expect, it } from "vitest";
import { describeCleanup, describeRunFinished, describeWaitReason } from "./runErrors";

describe("回合终态文案", () => {
  it("成功不留提醒；停止、超时各有短标题和下一步", () => {
    expect(describeRunFinished("ok", null)).toBeNull();
    expect(describeRunFinished("ok", "自动补救：会话不可用，已清空并重开一次")).toBeNull();
    const stopped = describeRunFinished("cancelled", "回合已停止，文件修改不会自动撤销");
    expect(stopped).toMatchObject({ kind: "stopped", title: "已停止", detail: "回合已停止，文件修改不会自动撤销" });
    expect(stopped?.next).toMatch(/不会自动撤销/);
    const timeout = describeRunFinished("timeout", "工具运行超过时限，已停止");
    expect(timeout).toMatchObject({ kind: "timeout", title: "回合超时", detail: "工具运行超过时限，已停止" });
    expect(timeout?.next).toMatch(/运行设置/);
  });

  it("CLI 不可用指向应用管理，权限受阻与上游失败都保持失败", () => {
    for (const error of ["CLI 不可用：未找到已安装的 claude", "助理 CLI「codex」已在应用管理中关闭，请先启用或更换助理 CLI", "未找到已安装的助理 CLI：grok"]) {
      const notice = describeRunFinished("failed", error);
      expect(notice, error).toMatchObject({ kind: "cli_unavailable", title: "CLI 不可用", detail: error });
      expect(notice?.next).toMatch(/应用管理/);
    }
    const blocked = describeRunFinished("failed", "权限受阻：git commit 被拒绝");
    expect(blocked).toMatchObject({ kind: "blocked", title: "权限受阻", detail: "权限受阻：git commit 被拒绝" });
    const upstream = describeRunFinished("failed", 'dsh 回合失败（INVALID_REQUEST）：400 {"error":{"message":"bad field"}}');
    expect(upstream).toMatchObject({ kind: "failed", title: "回合失败" });
    // 诊断原文照常展示，不做解析或改写。
    expect(upstream?.detail).toBe('dsh 回合失败（INVALID_REQUEST）：400 {"error":{"message":"bad field"}}');
  });

  it("没有原因的失败也有可读文案；等待清理与等待原因各有标题", () => {
    expect(describeRunFinished("failed", null)).toMatchObject({ kind: "failed", title: "回合失败", detail: null });
    expect(describeRunFinished("failed", null)?.next).toMatch(/重试|补充/);
    expect(describeCleanup("D:/项目")).toMatchObject({ kind: "cleanup", title: "等待清理", detail: "D:/项目" });
    expect(describeWaitReason(null)).toBeNull();
    expect(describeWaitReason("单条指令已达到 30 回合上限")).toMatchObject({ kind: "turn_limit", title: "已达回合上限" });
    expect(describeWaitReason("已停止。部分文件可能已改动，不会自动撤销。")).toMatchObject({ kind: "stopped", title: "已停止" });
    expect(describeWaitReason("任务「甲」因权限受阻，等待授权后重试：x")).toMatchObject({ kind: "blocked", title: "权限受阻" });
    expect(describeWaitReason("CLI 不可用：成员不存在")).toMatchObject({ kind: "cli_unavailable" });
    expect(describeWaitReason("组长回合失败")).toMatchObject({ kind: "failed", title: "回合失败", detail: "组长回合失败" });
  });
});
