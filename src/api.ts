import { invoke } from "@tauri-apps/api/core";
import type {
  Agent,
  AgentInput,
  CliInfo,
  NewProjectInput,
  PersonaTemplate,
  Project,
  ProjectDetail,
  ProjectSummary,
  RuleDoc,
} from "./types";

// 后端失败时 invoke 会 reject，值是一句中文说明；调用方用 String(e) 展示即可。
export const api = {
  listClis: () => invoke<CliInfo[]>("list_clis"),

  listProjects: () => invoke<ProjectSummary[]>("list_projects"),
  createProject: (input: NewProjectInput) => invoke<Project>("create_project", { input }),
  getProject: (id: string) => invoke<ProjectDetail>("get_project", { id }),
  updateProject: (id: string, name: string, description: string, folderPath: string) =>
    invoke<Project>("update_project", { id, name, description, folderPath }),
  deleteProject: (id: string) => invoke<void>("delete_project", { id }),

  createAgent: (projectId: string, input: AgentInput) =>
    invoke<Agent>("create_agent", { projectId, input }),
  updateAgent: (id: string, input: AgentInput) => invoke<Agent>("update_agent", { id, input }),
  deleteAgent: (id: string) => invoke<void>("delete_agent", { id }),

  listRules: (projectId: string) => invoke<RuleDoc[]>("list_rules", { projectId }),
  saveRule: (projectId: string, filename: string, content: string) =>
    invoke<void>("save_rule", { projectId, filename, content }),
  deleteRule: (projectId: string, filename: string) =>
    invoke<void>("delete_rule", { projectId, filename }),

  listRuleTemplates: () => invoke<RuleDoc[]>("list_rule_templates"),
  listPersonaTemplates: () => invoke<PersonaTemplate[]>("list_persona_templates"),
};
