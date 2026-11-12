import { Link } from "react-router";
import { STATUS_LABELS, type ProjectSummary } from "../types";

interface Props {
  project: ProjectSummary;
  onDelete: () => void;
}

export function ProjectCard({ project, onDelete }: Props) {
  return (
    <div
      className={`relative min-h-44 rounded-xl border bg-slate-900/60 p-5 ${
        project.folderExists ? "border-slate-800 hover:border-slate-700" : "border-red-400/40"
      }`}
    >
      <Link to={`/project/${project.id}`} className="block space-y-3 rounded">
        <h2 className="truncate pr-6 text-base font-medium">{project.name}</h2>
        <p className="truncate text-xs text-slate-400" title={project.folderPath}>
          {project.folderPath}
        </p>
        {!project.folderExists && <p className="text-xs text-red-400">项目文件夹不存在</p>}
        <div className="flex items-center justify-between text-sm">
          <span>{project.agentCount} 个 Agent</span>
          <span className={`flex items-center gap-2 text-xs ${project.status === "waiting_user" ? "text-amber-200" : project.status === "idle" ? "text-slate-500" : "text-sky-200"}`}>
            <span aria-hidden className="size-1 rounded-full bg-current" />{STATUS_LABELS[project.status]}
          </span>
        </div>
        <p className="text-xs text-slate-500">
          最近活动：{new Date(project.updatedAt).toLocaleString("zh-CN")}
        </p>
      </Link>
      <button
        type="button"
        aria-label={`删除项目 ${project.name}`}
        onClick={onDelete}
        className="absolute right-2 top-2 grid size-8 place-items-center rounded-lg text-slate-500 hover:bg-slate-800 hover:text-red-300"
      >
        ✕
      </button>
    </div>
  );
}
