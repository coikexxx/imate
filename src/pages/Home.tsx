import { confirm } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { Link } from "react-router";
import { api } from "../api";
import { AppShell } from "../components/layout/AppShell";
import { ProjectCard } from "../components/project/ProjectCard";
import { Icon } from "../components/ui/Icon";
import type { ProjectSummary } from "../types";

export default function Home() {
  const [projects, setProjects] = useState<ProjectSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  function reload() {
    api
      .listProjects()
      .then(setProjects)
      .catch((e) => setError(String(e)));
  }

  useEffect(reload, []);

  async function remove(project: ProjectSummary) {
    const confirmed = await confirm(
      `删除项目「${project.name}」？只删除应用内的记录，项目文件夹里的文件不会动。`,
      { title: "删除项目", kind: "warning" },
    );
    if (!confirmed) return;
    try {
      await api.deleteProject(project.id);
      reload();
    } catch (e) {
      setError(String(e));
    }
  }

  const list = projects ?? [];
  const agentTotal = list.reduce((sum, p) => sum + p.agentCount, 0);
  const activeCount = list.filter((p) => p.status === "discussing" || p.status === "executing").length;

  return (
    <AppShell>
      <main className="studio-page">
        <header className="studio-page-head">
          <div>
            <p className="studio-kicker"><Icon name="grid" width="14" height="14" />工作空间</p>
            <h1 className="studio-page-title">项目总览</h1>
            <p className="studio-subtitle mt-1.5">把目标交给团队，按组长负责制讨论、分工与验收交付</p>
          </div>
          <Link to="/new" className="studio-primary-link">
            <Icon name="plus" width="15" height="15" />新建项目
          </Link>
        </header>

        <section className="studio-summary" aria-label="工作空间总览">
          <div>
            <strong>{projects ? list.length : "—"}</strong>
            <span>全部项目</span>
          </div>
          <div>
            <strong>{projects ? agentTotal : "—"}</strong>
            <span>团队成员</span>
          </div>
          <div>
            <strong>{projects ? activeCount : "—"}</strong>
            <span>正在协作的项目</span>
          </div>
        </section>

        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-[15px] font-medium text-ink">我的项目</h2>
          <span className="text-xs text-ink-muted">文件保存在所选本地目录，删除项目不会改动原文件</span>
        </div>

        {error && (
          <p role="alert" className="mb-4 rounded-lg border border-danger-border bg-danger-soft px-3.5 py-2.5 text-sm text-danger">
            {error}
          </p>
        )}

        <div className="studio-project-grid">
          {list.map((project) => (
            <ProjectCard key={project.id} project={project} onDelete={() => remove(project)} />
          ))}
          <Link to="/new" className="studio-project-new">
            <span className="grid size-9 place-items-center rounded-lg border border-line-strong bg-surface text-ink-secondary">
              <Icon name="plus" width="16" height="16" />
            </span>
            <span className="mt-1 text-sm font-medium text-ink">开始一个新项目</span>
            <span className="text-xs text-ink-muted">选择本地文件夹，制定规章并组建团队</span>
          </Link>
        </div>
      </main>
    </AppShell>
  );
}
