// 与后端 DTO 一一对应，字段为 camelCase。

export interface ProxySettings { mode: "system" | "manual" | "direct"; url: string }
export interface ProxyState { settings: ProxySettings; detectedProxy: string; detectionError: string | null }
export type DshProfile = "web" | "headless";
export interface DshSettings { authentication: "headless" | "account"; profile: DshProfile }
export interface DshCatalog {
  profile: DshProfile;
  groups: { id: string; name: string; models: { id: string; name: string }[] }[];
  failures: { provider: string; message: string }[];
}

export interface CliInfo {
  id: string;
  label: string;
  installed: boolean;
  supported: boolean;
  path: string | null;
  version: string | null;
}

export interface CliModel { id: string; label: string; custom: boolean }

/** 应用管理里一个 CLI 的完整视图：检测结果加已保存的配置 */
export interface CliConfig extends CliInfo {
  enabled: boolean;
  customPath: string | null;
  models: CliModel[];
  defaultModel: string | null;
  modelsSource: "auto" | "builtin" | "none";
}

export interface CliConfigInput {
  enabled: boolean;
  customPath: string | null;
  models: CliModel[];
  defaultModel: string | null;
}

export interface CliModelCatalog { models: CliModel[]; error: string | null; providerHost: string | null }

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
  sessionId: string | null;
  lastSeenMessageId: number | null;
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

export interface ModelInfo { requestedModel: string | null; reportedModel: string | null; runStatus: string | null }
export interface Message {
  id: number;
  projectId: string;
  senderType: string;
  senderAgentId: string | null;
  kind: string;
  content: string;
  round: number | null;
  createdAt: number;
  modelInfo: ModelInfo | null;
}
export type AgentStatus = "idle" | "thinking" | "talking" | "working" | "error";
export interface MessageDelta { projectId: string; runId: string; seq: number; agentId: string; text: string }
export interface MessageNew { projectId: string; runId: string | null; message: Message }
export interface AgentStatusEvent { projectId: string; runId: string | null; agentId: string; status: AgentStatus }
export interface RunFinished {
  projectId: string; runId: string; agentId: string; seq: number;
  status: "ok" | "failed" | "timeout" | "cancelled"; error: string | null;
  replacement?: { runId: string; messageId: number };
}
export interface RuntimeSnapshot { projectId: string; runId: string; agentId: string; status: AgentStatus; seq: number; partialText: string }
export interface CleanupNotice { runId: string; projectId: string | null; folderPath: string }
export interface ProjectRuntime { active: RuntimeSnapshot | null; cleanupRequired: CleanupNotice[] }
export interface SendMessageResponse { runId: string; userMessage: Message; agentMessageId: number }

export interface FileEntry { path: string; name: string; isDir: boolean }
export interface FileListing { entries: FileEntry[]; truncated: boolean }
/** paths 是这一批改动；changed 是当前高亮的全集，新指令开始时为空。 */
export interface FilesChanged { projectId: string; paths: string[]; changed: string[] }
export interface RulesChanged { projectId: string; version: string }

export interface Instruction {
  id: string;
  projectId: string;
  sourceMessageId: number;
  kind: string;
  targetAgentId: string | null;
  phase: string;
  status: string;
  discussRounds: number;
  turnCount: number;
  currentBatch: string | null;
  resumePhase: string | null;
  contextSnapshotJson: string | null;
  waitReason: string | null;
  createdAt: number;
  updatedAt: number;
  finishedAt: number | null;
  elevatedAllowed: boolean;
}

export interface TaskItem {
  id: string;
  projectId: string;
  instructionMessageId: number | null;
  assigneeAgentId: string | null;
  title: string;
  detail: string;
  orderIndex: number;
  status: string;
  reworkCount: number;
  reportMessageId: number | null;
  createdAt: number;
  finishedAt: number | null;
  instructionId: string | null;
  batchId: string | null;
  supersededByTaskId: string | null;
}
export interface ProjectEventMap {
  "message:delta": MessageDelta;
  "message:new": MessageNew;
  "agent:status": AgentStatusEvent;
  "run:finished": RunFinished;
  "files:changed": FilesChanged;
  "rules:changed": RulesChanged;
}

export interface DraftMember { name: string; role: string; persona: string }
export interface DraftInput {
  draftId: string;
  folderPath: string;
  projectName: string;
  description: string;
  rules: RuleDoc[];
  members: DraftMember[];
  memberName: string | null;
  role: string | null;
  currentText: string | null;
  targetFilename: string | null;
  feedback: string | null;
}
export interface DraftResponse {
  draftId: string;
  runId: string;
  draft: RuleDoc[] | string | null;
  rawText: string;
  sessionId: string | null;
  error: string | null;
}
export interface AssistantDelta { draftId: string; runId: string; seq: number; text: string }
