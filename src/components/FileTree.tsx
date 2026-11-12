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
    <div className="grid h-full min-h-0 grid-cols-[minmax(0,1fr)_minmax(180px,34%)] text-sm">
      {/* 左：目录树 */}
      <div className="min-h-0 overflow-y-auto p-2">
        {error && <p role="alert" className="px-1 pb-2 text-xs text-red-400">{error}</p>}
        {rows.length === 0 && <p className="px-1 py-4 text-center text-xs text-slate-500">项目文件夹里还没有可展示的文件。</p>}
        <ul>
          {rows.map((row) => (
            <li key={row.path}>
              <button type="button" title={row.path} style={{ paddingLeft: `${row.depth * 14 + 4}px` }}
                className={`flex w-full items-center gap-1.5 rounded py-0.5 pr-2 text-left hover:bg-slate-800 ${row.changed ? "text-amber-300" : row.isDir ? "text-slate-200" : "text-slate-400"}`}
                onClick={() => (row.isDir ? toggle(row.path) : openFile(row.path))}>
                <span className="w-3 shrink-0 text-xs text-slate-500">{row.isDir ? (row.collapsed ? "▸" : "▾") : "·"}</span>
                <span className="truncate">{row.name}{row.isDir ? "/" : ""}</span>
                {row.changed && <span className="ml-auto shrink-0 text-xs" aria-label="已变化">●</span>}
              </button>
            </li>
          ))}
        </ul>
        {listing.truncated && <p className="px-1 pt-2 text-xs text-slate-500">文件较多，只显示前 2000 项。</p>}
      </div>
      {/* 右：最近一条指令期间变化的文件 */}
      <div className="flex min-h-0 flex-col border-l border-slate-800">
        <p className="flex items-center justify-between border-b border-slate-800 px-3 py-1.5 text-xs text-slate-400">
          <span>本轮变化</span>
          <span className={changed.length > 0 ? "text-amber-300" : "text-slate-600"}>{changed.length} 个文件</span>
        </p>
        <div className="min-h-0 flex-1 overflow-y-auto p-2">
          {changed.length === 0 ? (
            <p className="px-1 py-3 text-center text-xs text-slate-500">最近一条指令期间没有文件变化。</p>
          ) : (
            <>
              <ul>
                {changed.map((path) => {
                  const cut = path.lastIndexOf("/");
                  return (
                    <li key={path}>
                      <button type="button" title={path} onClick={() => openFile(path)}
                        className="flex w-full items-baseline gap-2 rounded px-1 py-0.5 text-left hover:bg-slate-800">
                        <span className="shrink-0 text-amber-300">{path.slice(cut + 1)}</span>
                        {cut > 0 && <span className="truncate text-xs text-slate-500">{path.slice(0, cut)}</span>}
                      </button>
                    </li>
                  );
                })}
              </ul>
              <p className="px-1 pt-2 text-xs text-slate-600">不区分是谁改的</p>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
