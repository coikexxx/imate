import type { FileEntry } from "../types";

export interface TreeRow {
  path: string;
  name: string;
  isDir: boolean;
  depth: number;
  changed: boolean;
  collapsed: boolean;
}

/** 一个相对路径的全部上级目录，由浅到深。 */
export function ancestors(path: string): string[] {
  const parts = path.split("/");
  return parts.slice(0, -1).map((_, index) => parts.slice(0, index + 1).join("/"));
}

/** 把后端给的先序平铺列表变成当前可见的行。改动的文件连同上级目录一起标记。 */
export function visibleRows(entries: FileEntry[], collapsed: ReadonlySet<string>, changed: readonly string[]): TreeRow[] {
  const marked = new Set(changed);
  for (const path of changed) for (const dir of ancestors(path)) marked.add(dir);
  return entries
    .filter((entry) => !ancestors(entry.path).some((dir) => collapsed.has(dir)))
    .map((entry) => ({
      path: entry.path,
      name: entry.name,
      isDir: entry.isDir,
      depth: entry.path.split("/").length - 1,
      changed: marked.has(entry.path),
      collapsed: entry.isDir && collapsed.has(entry.path),
    }));
}
