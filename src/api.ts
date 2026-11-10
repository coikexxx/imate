import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import type {
  Agent,
  Message,
  ProjectRuntime,
  SendMessageResponse,
  ProjectEventMap,
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
  listMessages: (projectId: string, beforeId: number | null = null, limit = 50) =>
    invoke<Message[]>("list_messages", { projectId, beforeId, limit }),
  sendMessage: (projectId: string, content: string) => invoke<SendMessageResponse>("send_message", { projectId, content }),
  stopProject: (projectId: string) => invoke<void>("stop_project", { projectId }),
  getProjectRuntime: (projectId: string) => invoke<ProjectRuntime>("get_project_runtime", { projectId }),
  confirmRuntimeCleanup: (runId: string) => invoke<void>("confirm_runtime_cleanup", { runId }),
  listenProjectEvent: <K extends keyof ProjectEventMap>(name: K, receive: (payload: ProjectEventMap[K]) => void) =>
    listen<ProjectEventMap[K]>(name, (event) => receive(event.payload)),

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
