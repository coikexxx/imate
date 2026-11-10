import type { AgentInput, RuleDoc } from "../types";
import { isPersonaComplete, parsePersona } from "./persona";

/** 每个项目的成员上限。必须与后端 agents.rs 的 MAX_AGENTS 一致。 */
export const MAX_AGENTS = 8;

const FORBIDDEN = /[\\/:*?"<>|]/;

/** 与后端 rules.rs 的 validate_filename 规则一致。 */
export function isValidRuleFilename(filename: string): boolean {
  if (!filename.endsWith(".md")) return false;
  const stem = filename.slice(0, -3);
  return stem.trim() !== "" && !stem.startsWith(".") && !FORBIDDEN.test(filename);
}

/** 给新规章取一个不与现有文件冲突的名字：新规章.md、新规章2.md、新规章3.md…… */
export function uniqueRuleFilename(base: string, taken: string[]): string {
  if (!taken.includes(base)) return base;
  const stem = base.slice(0, -3);
  let n = 2;
  while (taken.includes(`${stem}${n}.md`)) n += 1;
  return `${stem}${n}.md`;
}

export function validateBasics(name: string, folderPath: string): string | null {
  if (name.trim() === "") return "请填写项目名称";
  if (folderPath.trim() === "") return "请选择项目文件夹";
  return null;
}

export function validateRules(rules: RuleDoc[]): string | null {
  if (rules.length === 0) return "至少保留一份规章";
  const seen = new Set<string>();
  for (const rule of rules) {
    if (!isValidRuleFilename(rule.filename)) return `规章文件名不合法：${rule.filename}`;
    if (seen.has(rule.filename)) return `规章文件名重复：${rule.filename}`;
    seen.add(rule.filename);
    if (rule.content.trim() === "") return `规章内容为空：${rule.filename}`;
  }
  return null;
}

/** takenNames：同项目里其他成员的名字。 */
export function validateAgent(agent: AgentInput, takenNames: string[]): string | null {
  const name = agent.name.trim();
  if (name === "") return "请填写成员名字";
  if (takenNames.includes(name)) return `成员名「${name}」已存在`;
  if (agent.role.trim() === "") return "请填写角色";
  if (agent.cli === "") return "请选择 CLI";
  if (!isPersonaComplete(parsePersona(agent.persona))) return "人设的四段都要填写";
  return null;
}

export function validateTeam(agents: AgentInput[]): string | null {
  if (agents.length === 0) return "至少添加一个成员";
  if (agents.length > MAX_AGENTS) return `每个项目最多 ${MAX_AGENTS} 个成员`;
  if (agents.filter((agent) => agent.isLeader).length !== 1) return "需要且只能有一个组长";
  return null;
}
