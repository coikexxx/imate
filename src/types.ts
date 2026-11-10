// 与后端 DTO 一一对应，字段为 camelCase。

export interface CliInfo {
  id: string;
  label: string;
  installed: boolean;
  path: string | null;
  version: string | null;
}

export type ProjectStatus = "idle" | "discussing" | "executing" | "waiting_user";

export const STATUS_LABELS: Record<ProjectStatus, string> = {
  idle: "空闲",
  discussing: "讨论中",
  executing: "执行中",
  waiting_user: "等待用户",
};

export interface Project {
  id: string;
  name: string;
  description: string;
  folderPath: string;
  status: ProjectStatus;
  createdAt: number;
  updatedAt: number;
}

export interface ProjectSummary extends Project {
  agentCount: number;
  folderExists: boolean;
}

export interface Agent {
  id: string;
  projectId: string;
  name: string;
  role: string;
  persona: string;
  personaDirty: boolean;
  cli: string;
  model: string | null;
  isLeader: boolean;
  seatIndex: number;
  createdAt: number;
}

/** 新建和编辑成员共用的输入 */
export interface AgentInput {
  name: string;
  role: string;
  persona: string;
  cli: string;
  model: string | null;
  isLeader: boolean;
}

export interface RuleDoc {
  filename: string;
  content: string;
}

export interface PersonaTemplate {
  role: string;
  persona: string;
}

export interface ProjectDetail {
  project: Project;
  agents: Agent[];
  folderExists: boolean;
}

export interface NewProjectInput {
  name: string;
  description: string;
  folderPath: string;
  rules: RuleDoc[];
  agents: AgentInput[];
}
