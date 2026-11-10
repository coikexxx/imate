import { Link } from "react-router";
import { STATUS_LABELS, type ProjectSummary } from "../types";

interface Props {
  project: ProjectSummary;
  onDelete: () => void;
}

export function ProjectCard({ project, onDelete }: Props) {
  return (
    <div
      className={`relative rounded-lg border bg-slate-900 p-4 ${
        project.folderExists ? "border-slate-700" : "border-red-700"
      }`}
    >
      <Link to={`/project/${project.id}`} className="block space-y-2">
        <h2 className="truncate pr-6 text-base font-semibold">{project.name}</h2>
        <p className="truncate text-xs text-slate-400" title={project.folderPath}>
          {project.folderPath}
        </p>
        {!project.folderExists && <p className="text-xs text-red-400">项目文件夹不存在</p>}
        <div className="flex items-center justify-between text-sm">
          <span>{project.agentCount} 个 Agent</span>
          <span className="rounded bg-slate-800 px-2 py-0.5 text-xs">
            {STATUS_LABELS[project.status]}
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
        className="absolute right-2 top-2 rounded px-1.5 text-slate-500 hover:bg-slate-800 hover:text-red-400"
      >
        ✕
      </button>
    </div>
  );
}
