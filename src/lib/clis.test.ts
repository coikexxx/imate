import { describe, expect, it } from "vitest";
import type { CliInfo } from "../types";
import { assistantAvailable } from "./clis";

const cli = (id: string, installed: boolean, supported: boolean): CliInfo =>
  ({ id, label: id, installed, supported, path: installed ? `/bin/${id}` : null, version: null });

describe("CLI 可用性", () => {
  it("任何已安装且已接入的 CLI 都能当助理", () => {
    expect(assistantAvailable([cli("claude", false, true), cli("grok", true, true)])).toBe(true);
    expect(assistantAvailable([cli("dsh", true, true)])).toBe(true);
  });
  it("未安装或未接入的不算", () => {
    expect(assistantAvailable([cli("claude", false, true), cli("pi", true, false)])).toBe(false);
    expect(assistantAvailable([])).toBe(false);
  });
});
