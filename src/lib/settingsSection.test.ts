import { describe, expect, it } from "vitest";
import { settingsSectionFromParam } from "./settingsSection";

describe("设置页深链", () => {
  it("识别三类设置", () => {
    expect(settingsSectionFromParam("runtime")).toBe("runtime");
    expect(settingsSectionFromParam("proxy")).toBe("proxy");
    expect(settingsSectionFromParam("apps")).toBe("apps");
  });

  it("没有参数或参数不认识时回到应用管理", () => {
    expect(settingsSectionFromParam(null)).toBe("apps");
    expect(settingsSectionFromParam(undefined)).toBe("apps");
    expect(settingsSectionFromParam("")).toBe("apps");
    expect(settingsSectionFromParam("runtimex")).toBe("apps");
  });
});
