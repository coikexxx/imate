import type { ReactNode } from "react";
import { Link, useLocation } from "react-router";
import { Icon } from "../ui/Icon";

export function AppShell({ children, projectName }: { children: ReactNode; projectName?: string }) {
  const { pathname } = useLocation();
  const isOverview = pathname === "/";
  const isWizard = pathname === "/new";
  const isSettings = pathname.startsWith("/settings");

  return (
    <div className="studio-shell">
      <aside className="studio-nav">
        <Link to="/" className="studio-brand" aria-label="imates 首页">
          <span className="studio-mark" aria-hidden="true"><i /><i /><i /><i /></span>
          <span>
            <span className="studio-brand-title">imates</span>
            <span className="studio-brand-note">团队工作室</span>
          </span>
        </Link>

        <div className="studio-nav-section">
          <span className="studio-nav-label">工作空间</span>
          <nav aria-label="主导航">
            <Link className={`studio-nav-link ${isOverview ? "is-active" : ""}`} to="/">
              <Icon name="projects" />
              <span>项目总览</span>
            </Link>
            <Link className={`studio-nav-link ${isWizard ? "is-active" : ""}`} to="/new">
              <Icon name="plus" />
              <span>新建项目</span>
            </Link>
          </nav>
        </div>

        {projectName && (
          <div className="studio-current">
            <span className="studio-nav-label">当前项目</span>
            <div className="studio-current-pill" title={projectName}>
              <span className="size-1.5 shrink-0 rounded-full bg-[#7ab392]" aria-hidden="true" />
              <span className="truncate">{projectName}</span>
            </div>
          </div>
        )}

        <div className="studio-nav-bottom">
          <Link className={`studio-nav-link ${isSettings ? "is-active" : ""}`} to="/settings">
            <Icon name="settings" />
            <span>系统设置</span>
          </Link>
          <div className="studio-nav-footer">
            <span className="studio-local-dot" aria-hidden="true" />
            <span>本地运行 · 数据存于本机</span>
          </div>
        </div>
      </aside>
      <div className="studio-main">{children}</div>
    </div>
  );
}
