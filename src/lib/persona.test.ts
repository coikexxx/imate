import { describe, expect, it } from "vitest";
import { composePersona, emptyPersona, isPersonaComplete, parsePersona } from "./persona";

const FULL =
  "## 定位\n\n后端开发\n\n## 职责\n\n- 写接口\n- 写测试\n\n## 边界\n\n不做界面\n\n## 工作风格\n\n务实\n";

describe("parsePersona", () => {
  it("拆出四段正文", () => {
    expect(parsePersona(FULL)).toEqual({
      定位: "后端开发",
      职责: "- 写接口\n- 写测试",
      边界: "不做界面",
      工作风格: "务实",
    });
  });

  it("缺失的段落为空字符串，第一个标题之前的文字被忽略", () => {
    expect(parsePersona("前言\n## 定位\n\n测试")).toEqual({ ...emptyPersona(), 定位: "测试" });
  });

  it("兼容 Windows 换行", () => {
    expect(parsePersona("## 定位\r\n\r\n测试\r\n").定位).toBe("测试");
  });
});

describe("composePersona", () => {
  it("与 parsePersona 互逆", () => {
    expect(composePersona(parsePersona(FULL))).toBe(FULL);
  });

  it("去掉每段首尾的空白", () => {
    const text = composePersona({ 定位: " a \n", 职责: "b", 边界: "c", 工作风格: "d" });
    expect(text).toBe("## 定位\n\na\n\n## 职责\n\nb\n\n## 边界\n\nc\n\n## 工作风格\n\nd\n");
  });
});

describe("isPersonaComplete", () => {
  it("四段都有内容才算完整", () => {
    expect(isPersonaComplete(parsePersona(FULL))).toBe(true);
    expect(isPersonaComplete({ ...parsePersona(FULL), 边界: "  " })).toBe(false);
    expect(isPersonaComplete(emptyPersona())).toBe(false);
  });
});
