/** 非 Dsh CLI 的推理强度档位，必须与后端 `reasoning.rs` 保持一致。Dsh 的强度跟随模型，见 `dsh.ts`。 */
const LEVELS: Record<string, string[]> = {
  claude: ["low", "medium", "high", "xhigh", "max"],
  codex: ["low", "medium", "high", "xhigh", "max", "ultra"],
  grok: ["low", "medium", "high", "xhigh"],
};

export function supportsReasoningEffort(cli: string): boolean { return cli in LEVELS; }

export function reasoningLevels(cli: string): string[] { return LEVELS[cli] ?? []; }

export interface EffortOption { value: string; label: string; unavailable: boolean }

/** 下拉选项：第一项「默认」表示沿用 CLI 自己的配置；已保存但当前 CLI 不支持的值保留并标出。 */
export function reasoningOptions(cli: string, current: string): EffortOption[] {
  const options: EffortOption[] = [{ value: "", label: "默认（沿用 CLI 配置）", unavailable: false },
    ...reasoningLevels(cli).map((level) => ({ value: level, label: level, unavailable: false }))];
  if (current && !options.some((option) => option.value === current)) {
    options.push({ value: current, label: `${current}（该 CLI 不支持）`, unavailable: true });
  }
  return options;
}

/** 换 CLI 时：新 CLI 也有这一档就保留，否则清掉。 */
export function keepEffortForCli(cli: string, current: string): string {
  return reasoningLevels(cli).includes(current) ? current : "";
}
