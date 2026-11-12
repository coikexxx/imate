import { confirm } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { Link } from "react-router";
import { api } from "../api";
import { ProjectCard } from "../components/ProjectCard";
import type { ProjectSummary } from "../types";
import { AppShell } from "../components/AppShell";
import { Icon } from "../components/Icon";

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

  const agentTotal = (projects ?? []).reduce((sum, p) => sum + p.agentCount, 0);

  return (
    <AppShell><main className="studio-page">
      <header className="studio-page-head">
        <div>
          <p className="studio-kicker"><Icon name="grid" />工作空间 / 项目</p>
          <h1 className="studio-page-title">项目总览</h1>
          <p className="studio-subtitle mt-2">把目标交给团队，让每一项工作都有着落</p>
        </div>
        <Link to="/new" className="studio-primary-link"><Icon name="plus" />新建项目</Link>
      </header>
      <section className="studio-summary" aria-label="工作空间总览">
        <div><strong>{projects?.length ?? "—"}</strong><span>全部项目</span></div>
        <div><strong>{projects ? agentTotal : "—"}</strong><span>团队成员</span></div>
        <div><strong>{projects ? projects.filter((p) => p.status === "discussing" || p.status === "executing").length : "—"}</strong><span>正在协作的项目</span></div>
      </section>
      <div className="mb-5 flex items-center justify-between"><h2 className="text-base font-medium">我的项目</h2><span className="text-xs text-slate-500">本地项目 · 文件保存在你的电脑上</span></div>

      {error && <p className="mb-4 text-sm text-red-400">{error}</p>}

      <div className="studio-project-grid">
        {(projects ?? []).map((project) => (
          <ProjectCard key={project.id} project={project} onDelete={() => remove(project)} />
        ))}
        <Link
          to="/new"
          className="studio-project-new"
        >
          <span className="grid size-10 place-items-center rounded-full border border-slate-700"><Icon name="plus" /></span>
          <span className="text-sm font-medium">开始一个新项目</span>
          <span className="text-xs text-slate-500">选择文件夹，组建你的团队</span>
        </Link>
      </div>
    </main></AppShell>
  );
}
