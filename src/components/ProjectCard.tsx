import { Link } from "react-router";
import { STATUS_LABELS, type ProjectSummary } from "../types";
import { Icon } from "./Icon";

interface Props { project: ProjectSummary; onDelete: () => void }

export function ProjectCard({ project, onDelete }: Props) {
  return <div className={`studio-project-card ${project.folderExists ? "" : "!border-red-400/40"}`}>
    <Link to={`/project/${project.id}`} className="flex flex-1 flex-col rounded">
      <div className="flex items-center justify-between pr-5"><span className="studio-project-symbol"><Icon name="projects" /></span><span className={`flex items-center gap-2 text-xs ${project.status === "waiting_user" ? "text-amber-200" : "text-slate-400"}`}><span className="size-1.5 rounded-full bg-current" />{STATUS_LABELS[project.status]}</span></div>
      <h2 className="truncate">{project.name}</h2>
      <p className="mt-2 line-clamp-2 min-h-10">{project.description || "团队已就位，随时可以开始协作"}</p>
      {!project.folderExists && <p className="mt-2 !text-red-400">项目文件夹不存在</p>}
      <p className="mb-4 mt-3 truncate font-mono !text-[11px]" title={project.folderPath}>{project.folderPath}</p>
      <div className="studio-project-bottom"><span className="flex items-center gap-2 text-xs text-slate-400"><Icon name="team" />{project.agentCount} 位成员</span><Icon name="arrow" className="text-sky-300" /></div>
      <p className="mt-3 !text-[11px]">最近活动 {new Date(project.updatedAt).toLocaleString("zh-CN")}</p>
    </Link>
    <button type="button" aria-label={`删除项目 ${project.name}`} onClick={onDelete} className="absolute right-3 top-3 grid size-7 place-items-center rounded text-slate-500 hover:bg-red-950 hover:text-red-400"><Icon name="close" width="14" height="14" /></button>
  </div>;
}
