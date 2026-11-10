import { describe, expect, it } from "vitest";
import type { AgentInput, RuleDoc } from "../types";
import {
  isValidRuleFilename,
  uniqueRuleFilename,
  validateAgent,
  validateBasics,
  validateRules,
  validateTeam,
} from "./wizard";

const PERSONA =
  "## 定位\n\n测试成员\n\n## 职责\n\n干活\n\n## 边界\n\n不越界\n\n## 工作风格\n\n务实\n";

function agent(name: string, isLeader: boolean): AgentInput {
  return { name, role: "开发", persona: PERSONA, cli: "claude", model: null, isLeader };
}

function rule(filename: string, content = "# 规章\n"): RuleDoc {
  return { filename, content };
}

describe("isValidRuleFilename", () => {
  it("接受普通的 .md 文件名", () => {
    expect(isValidRuleFilename("协作规范.md")).toBe(true);
    expect(isValidRuleFilename("API design v2.md")).toBe(true);
  });

  it("拒绝不合法的文件名", () => {
    for (const bad of [
      "协作规范.txt",
      ".md",
      "  .md",
      ".hidden.md",
      "a/b.md",
      "a\\b.md",
      "../逃逸.md",
      "C:x.md",
      "问?.md",
    ]) {
      expect(isValidRuleFilename(bad), bad).toBe(false);
    }
  });
});

describe("uniqueRuleFilename", () => {
  it("没有冲突时原样返回，有冲突时加序号", () => {
    expect(uniqueRuleFilename("新规章.md", [])).toBe("新规章.md");
    expect(uniqueRuleFilename("新规章.md", ["新规章.md"])).toBe("新规章2.md");
    expect(uniqueRuleFilename("新规章.md", ["新规章.md", "新规章2.md"])).toBe("新规章3.md");
  });
});

describe("validateBasics", () => {
  it("名称和文件夹都必填", () => {
    expect(validateBasics("官网", "D:/site")).toBeNull();
    expect(validateBasics("  ", "D:/site")).toBe("请填写项目名称");
    expect(validateBasics("官网", "")).toBe("请选择项目文件夹");
  });
});

describe("validateRules", () => {
  it("至少一份，文件名合法且不重复，内容非空", () => {
    expect(validateRules([rule("协作规范.md")])).toBeNull();
    expect(validateRules([])).toBe("至少保留一份规章");
    expect(validateRules([rule("a/b.md")])).toBe("规章文件名不合法：a/b.md");
    expect(validateRules([rule("协作规范.md"), rule("协作规范.md")])).toBe(
      "规章文件名重复：协作规范.md",
    );
    expect(validateRules([rule("协作规范.md", "  \n")])).toBe("规章内容为空：协作规范.md");
  });
});

describe("validateAgent", () => {
  it("通过合法输入", () => {
    expect(validateAgent(agent("老王", true), [])).toBeNull();
  });

  it("逐项报出问题", () => {
    expect(validateAgent(agent("  ", false), [])).toBe("请填写成员名字");
    expect(validateAgent(agent("老王", false), ["老王"])).toBe("成员名「老王」已存在");
    expect(validateAgent({ ...agent("老王", false), role: " " }, [])).toBe("请填写角色");
    expect(validateAgent({ ...agent("老王", false), cli: "" }, [])).toBe("请选择 CLI");
    expect(validateAgent({ ...agent("老王", false), persona: "## 定位\n\n只有一段" }, [])).toBe(
      "人设的四段都要填写",
    );
  });
});

describe("validateTeam", () => {
  it("需要 1 到 8 人且恰好一个组长", () => {
    expect(validateTeam([agent("老王", true), agent("小李", false)])).toBeNull();
    expect(validateTeam([])).toBe("至少添加一个成员");
    expect(validateTeam([agent("老王", false)])).toBe("需要且只能有一个组长");
    expect(validateTeam([agent("老王", true), agent("小李", true)])).toBe("需要且只能有一个组长");
    const nine = Array.from({ length: 9 }, (_, i) => agent(`成员${i}`, i === 0));
    expect(validateTeam(nine)).toBe("每个项目最多 8 个成员");
  });
});
