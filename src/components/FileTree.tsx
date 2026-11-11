import { useEffect, useState } from "react";
import { api } from "../api";
import { visibleRows } from "../lib/fileTree";
import type { FileListing } from "../types";

interface Props { projectId: string; changed: string[]; tick: number }

/** 只读文件树。点击文件用系统默认程序打开，点击目录折叠或展开。 */
export function FileTree({ projectId, changed, tick }: Props) {
  const [listing, setListing] = useState<FileListing | null>(null);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let disposed = false;
    api.listFiles(projectId)
      .then((next) => { if (!disposed) { setListing(next); setError(null); } })
      .catch((failure) => { if (!disposed) setError(String(failure)); });
    return () => { disposed = true; };
  }, [projectId, tick]);

  function toggle(path: string) {
    setCollapsed((current) => {
      const next = new Set(current);
      if (next.has(path)) next.delete(path); else next.add(path);
      return next;
    });
  }
  function openFile(path: string) {
    api.openFile(projectId, path).catch((failure) => setError(String(failure)));
  }

  if (error && !listing) return <p className="p-3 text-sm text-red-400">{error}</p>;
  if (!listing) return <p className="p-3 text-sm text-slate-500">加载中…</p>;
  const rows = visibleRows(listing.entries, collapsed, changed);
  return (
    <div className="p-2 text-sm">
      {error && <p role="alert" className="px-1 pb-2 text-xs text-red-400">{error}</p>}
      {changed.length > 0 && <p className="px-1 pb-2 text-xs text-amber-300">最近一条指令期间有 {changed.length} 个文件发生变化（不区分是谁改的）</p>}
      {rows.length === 0 && <p className="px-1 py-4 text-center text-xs text-slate-500">项目文件夹里还没有可展示的文件。</p>}
      <ul>
        {rows.map((row) => (
          <li key={row.path}>
            <button type="button" title={row.path} style={{ paddingLeft: `${row.depth * 12 + 4}px` }}
              className={`flex w-full items-center gap-1 rounded py-0.5 pr-1 text-left hover:bg-slate-800 ${row.changed ? "text-amber-300" : "text-slate-300"}`}
              onClick={() => (row.isDir ? toggle(row.path) : openFile(row.path))}>
              <span className="w-3 shrink-0 text-xs text-slate-500">{row.isDir ? (row.collapsed ? "▸" : "▾") : ""}</span>
              <span className="truncate">{row.name}{row.isDir ? "/" : ""}</span>
              {row.changed && <span className="ml-auto shrink-0 text-xs" aria-label="已变化">●</span>}
            </button>
          </li>
        ))}
      </ul>
      {listing.truncated && <p className="px-1 pt-2 text-xs text-slate-500">文件较多，只显示前 2000 项。</p>}
    </div>
  );
}
