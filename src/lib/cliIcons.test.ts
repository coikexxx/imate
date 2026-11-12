import { describe, expect, it } from "vitest";
import { cliIconInfo } from "./cliIcons";

const ALL_IDS = ["claude", "codex", "grok", "dsh", "gemini", "opencode", "openclaw", "hermes", "cursor", "qwen", "copilot", "kimi"];
// 单色图标（SVG 里只有 currentColor），深色界面需要反相成白色
const MONO_IDS = ["copilot", "cursor", "grok", "hermes", "opencode"];

describe("CLI 图标映射", () => {
  it("12 个有图标的 CLI 都能取到非空的图片地址", () => {
    for (const id of ALL_IDS) {
      const info = cliIconInfo(id);
      expect(info, id).not.toBeNull();
      expect(typeof info!.src, id).toBe("string");
      expect(info!.src.length, id).toBeGreaterThan(0);
    }
  });

  it("Pi、未知 id 与空串没有图标，交给字母徽标", () => {
    expect(cliIconInfo("pi")).toBeNull();
    expect(cliIconInfo("not-a-cli")).toBeNull();
    expect(cliIconInfo("")).toBeNull();
  });

  it("mono 标记与单色图标清单一致", () => {
    for (const id of ALL_IDS) {
      expect(cliIconInfo(id)!.mono, id).toBe(MONO_IDS.includes(id));
    }
  });

  it("kimi 混有固定蓝色，不能整体反相", () => {
    expect(cliIconInfo("kimi")!.mono).toBe(false);
  });
});
