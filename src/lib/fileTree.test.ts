import { describe, expect, it } from "vitest";
import { ancestors, visibleRows } from "./fileTree";

const entries = [
  { path: "docs", name: "docs", isDir: true },
  { path: "docs/深层", name: "深层", isDir: true },
  { path: "docs/深层/说明.md", name: "说明.md", isDir: false },
  { path: "src", name: "src", isDir: true },
  { path: "src/main.rs", name: "main.rs", isDir: false },
  { path: "README.md", name: "README.md", isDir: false },
];

describe("文件树行", () => {
  it("列出上级目录", () => {
    expect(ancestors("docs/深层/说明.md")).toEqual(["docs", "docs/深层"]);
    expect(ancestors("README.md")).toEqual([]);
  });
  it("按层级缩进，折叠的目录隐藏全部后代", () => {
    const rows = visibleRows(entries, new Set(["docs"]), []);
    expect(rows.map((row) => row.path)).toEqual(["docs", "src", "src/main.rs", "README.md"]);
    expect(rows[0]).toMatchObject({ depth: 0, isDir: true, collapsed: true });
    expect(rows[2]).toMatchObject({ depth: 1, isDir: false, collapsed: false });
  });
  it("改动的文件和它的上级目录都高亮，折叠后仍能在目录上看到", () => {
    const changed = ["docs/深层/说明.md"];
    const open = visibleRows(entries, new Set(), changed);
    expect(open.filter((row) => row.changed).map((row) => row.path)).toEqual(["docs", "docs/深层", "docs/深层/说明.md"]);
    const closed = visibleRows(entries, new Set(["docs"]), changed);
    expect(closed.find((row) => row.path === "docs")?.changed).toBe(true);
    expect(closed.find((row) => row.path === "src")?.changed).toBe(false);
  });
});
