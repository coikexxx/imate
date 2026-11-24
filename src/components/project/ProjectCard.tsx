import { Link } from "react-router";
import { STATUS_LABELS, type ProjectStatus, type ProjectSummary } from "../../types";
import { Icon } from "../ui/Icon";

interface Props {
  project: ProjectSummary;
  onDelete: () => void;
}

const STATUS_STYLES: Record<ProjectStatus, string> = {
  idle: "bg-surface-subtle text-ink-muted",
  discussing: "bg-accent-soft text-accent",
  executing: "bg-accent-soft text-accent",
  waiting_user: "bg-warn-soft text-warn",
};

function formatTime(timestamp: number): string {
  return new Date(timestamp).toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function ProjectCard({ project, onDelete }: Props) {
  return (
    <div className={`studio-project-card group ${project.folderExists ? "" : "is-missing"}`}>
      <Link to={`/project/${project.id}`} className="flex flex-1 flex-col rounded-lg text-left no-underline">
        <div className="flex items-center justify-between gap-2 pr-8">
          <span className="studio-project-symbol">
            <Icon name="projects" width="16" height="16" />
          </span>
          <span className={`inline-flex items-center gap-1.5 rounded-md px-2 py-0.5 text-xs font-medium ${STATUS_STYLES[project.status]}`}>
            <span className="size-1.5 rounded-full bg-current" aria-hidden="true" />
            {STATUS_LABELS[project.status]}
          </span>
        </div>

        <h2 className="mt-4 truncate text-[16px] font-medium text-ink">{project.name}</h2>
        <p className="mt-1.5 line-clamp-2 min-h-[38px] text-[12.5px] leading-[19px] text-ink-secondary">
          {project.description || "团队已就位，下发目标即可开始协作"}
        </p>

        {!project.folderExists && (
          <p className="mt-2 inline-flex w-fit items-center rounded-md border border-danger-border bg-danger-soft px-2 py-0.5 text-[11px] font-medium text-danger">
            项目文件夹不存在
          </p>
        )}

        <p className="mb-3.5 mt-3 truncate font-mono text-[11px] text-ink-muted" title={project.folderPath}>
          {project.folderPath}
        </p>

        <div className="studio-project-bottom">
          <div className="flex min-w-0 items-center gap-3 text-xs text-ink-secondary">
            <span className="inline-flex shrink-0 items-center gap-1.5 font-medium tabular-nums">
              <Icon name="team" width="14" height="14" className="text-ink-muted" />
              {project.agentCount} 位成员
            </span>
            <span className="truncate text-[11px] text-ink-muted tabular-nums">
              更新于 {formatTime(project.updatedAt)}
            </span>
          </div>
          <Icon name="arrow" width="15" height="15" className="shrink-0 text-ink-muted transition-transform group-hover:translate-x-0.5 group-hover:text-accent" />
        </div>
      </Link>

      <button
        type="button"
        aria-label={`删除项目 ${project.name}`}
        onClick={onDelete}
        className="absolute right-3 top-3 grid size-7 place-items-center rounded-md text-ink-muted opacity-60 hover:bg-danger-soft hover:text-danger hover:opacity-100 focus-visible:opacity-100 group-hover:opacity-100"
      >
        <Icon name="close" width="14" height="14" />
      </button>
    </div>
  );
}
