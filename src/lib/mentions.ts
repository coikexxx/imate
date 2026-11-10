import type { Agent } from "../types";

export function normalizeName(name: string): string { return name.trim().toLowerCase(); }
export function parseMention(text: string, agents: Agent[]): { agent: Agent; message: string } | { error: string } {
  const content = text.trim();
  if (!content) return { error: "消息不能为空" };
  if (!content.startsWith("@")) {
    const leader = agents.find((a) => a.isLeader);
    return leader ? { agent: leader, message: content } : { error: "项目没有组长，请先配置成员" };
  }
  const mention = content.slice(1);
  const ends = [...mention.matchAll(/\s/gu)].map((match) => match.index).concat(mention.length);
  const matches: { agent: Agent; message: string; length: number }[] = [];
  for (const end of ends) {
    const key = normalizeName(mention.slice(0, end));
    const found = agents.filter((agent) => normalizeName(agent.name) === key);
    if (found.length > 1) return { error: "成员名归一后有冲突，请先修改名称" };
    if (found.length === 1) matches.push({ agent: found[0], message: mention.slice(end).trim(), length: end });
  }
  const match = matches.sort((a, b) => b.length - a.length)[0];
  if (!match) return { error: "未找到被点名的成员，请从成员列表选择" };
  return match.message ? { agent: match.agent, message: match.message } : { error: "点名后请填写消息内容" };
}
export function mentionCandidates(text: string, agents: Agent[], projectId: string): Agent[] {
  const content = text.trimStart();
  if (!content.startsWith("@")) return [];
  const key = normalizeName(content.slice(1));
  return agents.filter((a) => a.projectId === projectId && normalizeName(a.name).startsWith(key));
}
export function shouldSendOnEnter(event: { key: string; shiftKey: boolean; isComposing: boolean; keyCode?: number }): boolean {
  return event.key === "Enter" && !event.shiftKey && !event.isComposing && event.keyCode !== 229;
}
