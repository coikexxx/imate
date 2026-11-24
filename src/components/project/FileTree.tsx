import { useEffect, useState } from "react";
import { api } from "../../api";
import { visibleRows } from "../../lib/fileTree";
import type { FileListing } from "../../types";
import { Icon } from "../ui/Icon";

interface Props {
  projectId: string;
  changed: string[];
  tick: number;
}

/** 只读文件树。点击文件用系统默认程序打开，点击目录折叠或展开。 */
export function FileTree({ projectId, changed, tick }: Props) {
  const [listing, setListing] = useState<FileListing | null>(null);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let disposed = false;
    api
      .listFiles(projectId)
      .then((next) => {
        if (!disposed) {
          setListing(next);
          setError(null);
        }
      })
      .catch((failure) => {
        if (!disposed) setError(String(failure));
      });
    return () => {
      disposed = true;
    };
  }, [projectId, tick]);

  function toggle(path: string) {
    setCollapsed((current) => {
      const next = new Set(current);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }

  function openFile(path: string) {
    api.openFile(projectId, path).catch((failure) => setError(String(failure)));
  }

  if (error && !listing) return <p className="p-4 text-sm text-danger">{error}</p>;
  if (!listing) return <p className="p-4 text-sm text-ink-secondary">正在读取目录结构…</p>;
  const rows = visibleRows(listing.entries, collapsed, changed);

  return (
    <div className="grid h-full min-h-0 grid-cols-[minmax(0,1fr)_minmax(210px,34%)] text-[13px]">
      <div className="min-h-0 overflow-y-auto p-3">
        {error && (
          <p role="alert" className="mb-2 rounded-lg border border-danger-border bg-danger-soft px-3 py-1.5 text-xs text-danger">
            {error}
          </p>
        )}
        {rows.length === 0 && (
          <p className="py-8 text-center text-xs text-ink-muted">项目文件夹里还没有可展示的文件</p>
        )}
        <ul className="space-y-0.5">
          {rows.map((row) => (
            <li key={row.path}>
              <button
                type="button"
                title={row.path}
                style={{ paddingLeft: `${row.depth * 16 + 8}px` }}
                className={`flex min-h-[30px] w-full items-center gap-2 rounded-md pr-2.5 text-left hover:bg-surface-subtle ${
                  row.changed
                    ? "font-medium text-warn"
                    : row.isDir
                      ? "font-medium text-ink"
                      : "text-ink-secondary"
                }`}
                onClick={() => (row.isDir ? toggle(row.path) : openFile(row.path))}
              >
                {row.isDir ? (
                  <>
                    <Icon
                      name={row.collapsed ? "chevronRight" : "chevronDown"}
                      width="13"
                      height="13"
                      className="shrink-0 text-ink-muted"
                    />
                    <Icon
                      name={row.collapsed ? "folder" : "folderOpen"}
                      width="15"
                      height="15"
                      className={row.changed ? "shrink-0 text-warn" : "shrink-0 text-ink-muted"}
                    />
                  </>
                ) : (
                  <>
                    <span className="w-[13px] shrink-0" aria-hidden="true" />
                    <Icon
                      name="file"
                      width="15"
                      height="15"
                      className={row.changed ? "shrink-0 text-warn" : "shrink-0 text-ink-muted"}
                    />
                  </>
                )}
                <span className="truncate font-mono text-xs">{row.name}</span>
                {row.changed && (
                  <span
                    className="ml-auto size-1.5 shrink-0 rounded-full bg-warn"
                    aria-label="已变化"
                  />
                )}
              </button>
            </li>
          ))}
        </ul>
        {listing.truncated && (
          <p className="px-2 pt-3 text-xs text-ink-muted">文件较多，仅显示前 2,000 项</p>
        )}
      </div>

      <div className="flex min-h-0 flex-col border-l border-line bg-paper/50">
        <div className="flex items-center justify-between border-b border-line px-3.5 py-2 text-xs">
          <span className="font-medium text-ink-secondary">本轮变化</span>
          <span
            className={`rounded px-1.5 py-0.5 font-mono text-[11px] tabular-nums ${
              changed.length > 0 ? "bg-warn-soft font-medium text-warn" : "bg-surface-subtle text-ink-muted"
            }`}
          >
            {changed.length} 个文件
          </span>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto p-2.5">
          {changed.length === 0 ? (
            <p className="py-8 text-center text-xs text-ink-muted">最近一条指令期间没有文件变化</p>
          ) : (
            <ul className="space-y-1">
              {changed.map((path) => {
                const cut = path.lastIndexOf("/");
                return (
                  <li key={path}>
                    <button
                      type="button"
                      title={path}
                      onClick={() => openFile(path)}
                      className="flex w-full items-start gap-2 rounded-lg border border-transparent px-2.5 py-1.5 text-left hover:border-line hover:bg-surface"
                    >
                      <Icon name="file" width="14" height="14" className="mt-0.5 shrink-0 text-warn" />
                      <span className="min-w-0 flex-1">
                        <span className="block truncate font-mono text-xs font-medium text-ink">
                          {path.slice(cut + 1)}
                        </span>
                        {cut > 0 && (
                          <span className="block truncate font-mono text-[11px] text-ink-muted">
                            {path.slice(0, cut)}
                          </span>
                        )}
                      </span>
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      </div>
    </div>
  );
}
