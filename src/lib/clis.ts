import type { CliInfo } from "../types";

/** 内置助理可用：本机至少有一个已安装且已接入的 CLI。后端按 claude、codex、grok、dsh 的顺序选用。 */
export function assistantAvailable(clis: CliInfo[]): boolean {
  return clis.some((cli) => cli.installed && cli.supported);
}
