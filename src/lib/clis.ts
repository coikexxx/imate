import type { CliInfo } from "../types";

/**
 * 内置助理可用：本机至少有一个已安装且已接入的 CLI。后端按 claude、codex、grok、dsh 的顺序选用。
 * `closedIds` 是应用管理里已关闭的 CLI，不会被选用；不传则不排除。
 */
export function assistantAvailable(clis: CliInfo[], closedIds?: ReadonlySet<string>): boolean {
  return clis.some((cli) => cli.installed && cli.supported && !closedIds?.has(cli.id));
}
