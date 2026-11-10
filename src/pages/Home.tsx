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
    <main className="mx-auto max-w-6xl p-8">
      <header className="mb-6 flex items-baseline justify-between">
        <h1 className="text-2xl font-bold">imates</h1>
        <p className="text-sm text-slate-400">
          {projects ? `${projects.length} 个项目 · ${agentTotal} 个 Agent` : "加载中…"}
        </p>
      </header>

      {error && <p className="mb-4 text-sm text-red-400">{error}</p>}

      <div className="grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-4">
        {(projects ?? []).map((project) => (
          <ProjectCard key={project.id} project={project} onDelete={() => remove(project)} />
        ))}
        <Link
          to="/new"
          className="flex min-h-36 items-center justify-center rounded-lg border border-dashed border-slate-600 text-slate-400 hover:border-sky-500 hover:text-sky-400"
        >
          ＋ 新建项目
        </Link>
      </div>
    </main>
  );
}
