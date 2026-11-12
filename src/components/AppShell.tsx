import type { ReactNode } from "react";
import { Link, useLocation } from "react-router";
import { Icon } from "./Icon";

export function AppShell({ children, projectName }: { children: ReactNode; projectName?: string }) {
  const { pathname } = useLocation();
  return <div className="studio-shell">
    <aside className="studio-nav">
      <Link to="/" className="studio-brand" aria-label="imates 首页"><span className="studio-mark"><i /><i /><i /><i /></span><span>imates<span className="studio-brand-note">团队工作室</span></span></Link>
      <div className="studio-nav-label">工作空间</div>
      <nav aria-label="主导航">
        <Link className={`studio-nav-link ${!pathname.startsWith("/settings") ? "is-active" : ""}`} to="/"><Icon name="projects" />项目总览<Icon name="arrow" className="ml-auto opacity-50" /></Link>
      </nav>
      {projectName && <div className="studio-current"><span className="studio-nav-label">当前项目</span><span className="mt-3 flex items-center gap-2 text-sm"><span className="size-1.5 shrink-0 rounded-full bg-[#85b89b]" /><span className="truncate">{projectName}</span></span></div>}
      <Link to="/new" className="studio-new"><Icon name="plus" />新建项目</Link>
      <div className="studio-nav-bottom">
        <Link className={`studio-nav-link ${pathname.startsWith("/settings") ? "is-active" : ""}`} to="/settings"><Icon name="settings" />系统设置</Link>
        <div className="studio-nav-footer"><span className="studio-local-dot" /><div>本地工作空间<span>你的团队，在这台电脑上</span></div></div>
      </div>
    </aside>
    <div className="studio-main">{children}</div>
  </div>;
}
