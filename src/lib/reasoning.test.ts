import { describe, expect, it } from "vitest";
import { keepEffortForCli, reasoningLevels, reasoningOptions, supportsReasoningEffort } from "./reasoning";

describe("推理强度档位", () => {
  it("每个 CLI 只给自己支持的档位，Dsh 不在这里处理", () => {
    expect(reasoningLevels("claude")).toEqual(["low", "medium", "high", "xhigh", "max"]);
    expect(reasoningLevels("codex")).toEqual(["low", "medium", "high", "xhigh", "max", "ultra"]);
    expect(reasoningLevels("grok")).not.toContain("max");
    expect(reasoningLevels("grok")).not.toContain("ultra");
    expect(supportsReasoningEffort("dsh")).toBe(false);
    expect(reasoningLevels("dsh")).toEqual([]);
  });

  it("第一项是默认；已保存但当前 CLI 不支持的值保留并标为不可用", () => {
    const options = reasoningOptions("grok", "");
    expect(options[0]).toMatchObject({ value: "", unavailable: false });
    expect(options.map((option) => option.value)).toEqual(["", "low", "medium", "high", "xhigh"]);
    const stale = reasoningOptions("grok", "max");
    expect(stale[stale.length - 1]).toMatchObject({ value: "max", unavailable: true });
  });

  it("换 CLI 时保留共有的档位，清掉没有的", () => {
    expect(keepEffortForCli("codex", "high")).toBe("high");
    expect(keepEffortForCli("codex", "ultra")).toBe("ultra");
    expect(keepEffortForCli("grok", "ultra")).toBe("");
    expect(keepEffortForCli("dsh", "high")).toBe("");
    expect(keepEffortForCli("claude", "")).toBe("");
  });
});
