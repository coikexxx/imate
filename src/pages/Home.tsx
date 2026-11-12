import { confirm } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { Link } from "react-router";
import { api } from "../api";
import { ProjectCard } from "../components/ProjectCard";
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

  const agentTotal = (projects ?? []).reduce((sum, p) => sum + p.agentCount, 0);

  return (
    <main className="mx-auto min-h-full max-w-6xl px-6 py-8 lg:px-8">
      <header className="mb-8 flex flex-wrap items-center justify-between gap-4 border-b border-slate-800 pb-6">
        <div>
          <h1 className="text-2xl font-medium">imates</h1>
          <p className="mt-2 text-sm text-slate-400">项目与团队</p>
        </div>
        <div className="flex items-center gap-4">
          <p className="text-sm text-slate-400">
            {projects ? `${projects.length} 个项目 · ${agentTotal} 个 Agent` : "加载中…"}
          </p>
          <Link to="/settings" className="inline-flex min-h-9 shrink-0 items-center rounded-lg border border-slate-700 bg-slate-900 px-3 py-2 text-sm hover:bg-slate-800">系统设置</Link>
        </div>
      </header>

      {error && <p className="mb-4 text-sm text-red-400">{error}</p>}

      <div className="grid grid-cols-[repeat(auto-fill,minmax(min(100%,280px),1fr))] gap-4">
        {(projects ?? []).map((project) => (
          <ProjectCard key={project.id} project={project} onDelete={() => remove(project)} />
        ))}
        <Link
          to="/new"
          className="flex min-h-44 items-center justify-center gap-2 rounded-xl border border-dashed border-slate-700 text-sm text-slate-400 hover:border-slate-500 hover:bg-slate-900 hover:text-slate-100"
        >
          ＋ 新建项目
        </Link>
      </div>
    </main>
  );
}
