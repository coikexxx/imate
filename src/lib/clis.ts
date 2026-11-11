import type { CliInfo } from "../types";

/** 内置助理可用：本机至少有一个已安装且已接入的 CLI。后端按 claude、codex、grok、dsh 的顺序选用。 */
export function assistantAvailable(clis: CliInfo[]): boolean {
  return clis.some((cli) => cli.installed && cli.supported);
}

/** dsh 的模型写在它自己的 profile 里，命令行上不能指定。 */
export function modelIgnored(cliId: string): boolean {
  return cliId === "dsh";
}
