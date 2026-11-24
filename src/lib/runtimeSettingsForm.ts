import type { CliConfig, CliInfo, RuntimeLimits, RuntimeSettings } from "../types";

/** 窗口里的护栏输入：整数字段按回合或次数，时长按分钟；全部是输入框原文。 */
export interface LimitsForm {
  discussRounds: string;
  reworksPerTask: string;
  turnsPerInstruction: string;
  modelIdleMinutes: string;
  discussToolMinutes: string;
  executeToolMinutes: string;
  discussTotalMinutes: string;
  executeTotalMinutes: string;
}

/** 空字符串表示助理按安装顺序自动选择。 */
export interface SettingsForm { assistantCli: string; limits: LimitsForm }

export interface LimitField {
  key: keyof LimitsForm;
  label: string;
  unit: "回合" | "次" | "分钟";
  /** 取值范围与生效时机，提交前就展示。 */
  hint: string;
  min: number;
  max: number;
  integer: boolean;
}

/** 与后端 `RuntimeLimits::default()` 一致的手册默认值。 */
export const DEFAULT_LIMITS: RuntimeLimits = {
  discussRounds: 3, reworksPerTask: 2, turnsPerInstruction: 30, modelIdleSeconds: 300,
  discussToolSeconds: 600, executeToolSeconds: 1800, discussTotalSeconds: 600, executeTotalSeconds: 1800,
};

const MAX_MINUTES = 24 * 60;
const FROZEN = "新指令开始时冻结，进行中的指令不受影响";

export const LIMIT_FIELDS: LimitField[] = [
  { key: "discussRounds", label: "讨论轮数上限", unit: "回合", hint: `按整条指令累计，1 到 10；${FROZEN}`, min: 1, max: 10, integer: true },
  { key: "reworksPerTask", label: "单任务返工上限", unit: "次", hint: `0 表示组长不能打回任务，最多 10；${FROZEN}`, min: 0, max: 10, integer: true },
  { key: "turnsPerInstruction", label: "单条指令回合总数上限", unit: "回合", hint: `组长、成员与会话重开都计入，前 3 次协议纠错不占额度，1 到 300；任务多的长指令可以上调；${FROZEN}`, min: 1, max: 300, integer: true },
  { key: "modelIdleMinutes", label: "模型空闲阈值", unit: "分钟", hint: `没有任何输出超过这个时长就停止；团队回合按新指令冻结，点名与起草按下一次启动生效`, min: 1 / 60, max: MAX_MINUTES, integer: false },
  { key: "discussToolMinutes", label: "讨论工具运行阈值", unit: "分钟", hint: `单个工具从开始到结束的上限；团队回合按新指令冻结，点名与起草按下一次启动生效`, min: 1 / 60, max: MAX_MINUTES, integer: false },
  { key: "executeToolMinutes", label: "执行工具运行阈值", unit: "分钟", hint: `单个工具从开始到结束的上限；团队回合按新指令冻结，点名与起草按下一次启动生效`, min: 1 / 60, max: MAX_MINUTES, integer: false },
  { key: "discussTotalMinutes", label: "讨论回合总时长", unit: "分钟", hint: `有输出也不能超过；团队回合按新指令冻结，点名与起草按下一次启动生效`, min: 1 / 60, max: MAX_MINUTES, integer: false },
  { key: "executeTotalMinutes", label: "执行回合总时长", unit: "分钟", hint: `有输出也不能超过；团队回合按新指令冻结，点名与起草按下一次启动生效`, min: 1 / 60, max: MAX_MINUTES, integer: false },
];

const SECONDS_KEYS: Record<keyof LimitsForm, keyof RuntimeLimits> = {
  discussRounds: "discussRounds", reworksPerTask: "reworksPerTask", turnsPerInstruction: "turnsPerInstruction",
  modelIdleMinutes: "modelIdleSeconds", discussToolMinutes: "discussToolSeconds", executeToolMinutes: "executeToolSeconds",
  discussTotalMinutes: "discussTotalSeconds", executeTotalMinutes: "executeTotalSeconds",
};

function minutesText(seconds: number): string {
  const minutes = seconds / 60;
  return Number.isInteger(minutes) ? String(minutes) : String(Math.round(minutes * 100) / 100);
}

export function toForm(settings: RuntimeSettings): SettingsForm {
  const limits = {} as LimitsForm;
  for (const field of LIMIT_FIELDS) {
    const value = settings.limits[SECONDS_KEYS[field.key]];
    limits[field.key] = field.integer ? String(value) : minutesText(value);
  }
  return { assistantCli: settings.assistantCli ?? "", limits };
}

/** 解析一个字段；返回换算后的后端值（整回合或整秒），或一句指出字段的错误。 */
function parseField(field: LimitField, raw: string): { ok: true; value: number } | { ok: false; error: string } {
  const text = raw.trim();
  if (field.integer) {
    if (!/^\d+$/.test(text)) return { ok: false, error: `${field.label}须是 ${field.min} 到 ${field.max} 之间的整数` };
    const value = Number(text);
    if (value < field.min || value > field.max) return { ok: false, error: `${field.label}须在 ${field.min} 到 ${field.max} 之间` };
    return { ok: true, value };
  }
  const minutes = Number(text);
  if (text === "" || !Number.isFinite(minutes)) return { ok: false, error: `${field.label}须填写分钟数` };
  const seconds = Math.round(minutes * 60);
  if (seconds < 1 || minutes > field.max) return { ok: false, error: `${field.label}须在 1 秒到 ${field.max} 分钟之间（按分钟填写）` };
  return { ok: true, value: seconds };
}

export function fromForm(form: SettingsForm): { ok: true; settings: RuntimeSettings } | { ok: false; error: string } {
  const limits = { ...DEFAULT_LIMITS };
  for (const field of LIMIT_FIELDS) {
    const parsed = parseField(field, form.limits[field.key]);
    if (!parsed.ok) return parsed;
    limits[SECONDS_KEYS[field.key]] = parsed.value;
  }
  const assistant = form.assistantCli.trim();
  return { ok: true, settings: { assistantCli: assistant || null, limits } };
}

/** 草稿是否与已保存值不同。无法解析的草稿当作已修改。 */
export function isDirty(saved: RuntimeSettings, form: SettingsForm): boolean {
  const parsed = fromForm(form);
  if (!parsed.ok) return true;
  return JSON.stringify(parsed.settings) !== JSON.stringify({ assistantCli: saved.assistantCli ?? null, limits: saved.limits });
}

/** 后端自动选择的顺序。 */
const AUTO_ORDER = ["claude", "codex", "grok", "dsh"];

function status(cli: CliInfo, configs: CliConfig[] | null): { installed: boolean; enabled: boolean | null; text: string } {
  const config = configs?.find((item) => item.id === cli.id);
  const enabled = config ? config.enabled : null;
  const parts = [cli.installed ? "已安装" : "未安装"];
  if (enabled !== null) parts.push(enabled ? "已启用" : "已关闭");
  return { installed: cli.installed, enabled, text: parts.join(" · ") };
}

function supportedClis(clis: CliInfo[]): CliInfo[] {
  return AUTO_ORDER.map((id) => clis.find((cli) => cli.id === id && cli.supported)).filter((cli): cli is CliInfo => Boolean(cli));
}

function autoPick(clis: CliInfo[], configs: CliConfig[] | null): CliInfo | null {
  return supportedClis(clis).find((cli) => { const state = status(cli, configs); return state.installed && state.enabled !== false; }) ?? null;
}

export function assistantOptions(clis: CliInfo[], configs: CliConfig[] | null, current: string): { value: string; label: string }[] {
  const options = [{ value: "", label: "自动选择（按 Claude Code、Codex、Grok、DeepSeek Harness 的顺序取第一个已安装且已启用的）" }];
  for (const cli of supportedClis(clis)) options.push({ value: cli.id, label: `${cli.label} · ${status(cli, configs).text}` });
  if (current && !options.some((option) => option.value === current)) options.push({ value: current, label: `${current}（未知，保存时会被拒绝）` });
  return options;
}

export function assistantNotice(clis: CliInfo[], configs: CliConfig[] | null, current: string): { tone: "info" | "warning"; text: string } {
  if (!current) {
    const picked = autoPick(clis, configs);
    return picked
      ? { tone: "info", text: `下一次起草将使用 ${picked.label}。` }
      : { tone: "warning", text: "当前没有已安装且已启用的 CLI，内置助理不可用；请先安装并在应用管理中启用。" };
  }
  const cli = clis.find((item) => item.id === current);
  if (!cli || !cli.supported) return { tone: "warning", text: `「${current}」不是已接入的 CLI，起草时会报错。` };
  const state = status(cli, configs);
  if (!state.installed) return { tone: "warning", text: `${cli.label} 未安装，起草时会报错；可在应用管理里指定路径，或改选其他 CLI。` };
  if (state.enabled === false) return { tone: "warning", text: `${cli.label} 已关闭，起草时会报错；请到应用管理里启用，或改选其他 CLI。` };
  return { tone: "info", text: `下一次起草将使用 ${cli.label}。` };
}

export type PanelState =
  | { phase: "loading" }
  | { phase: "load_failed"; error: string }
  | { phase: "ready"; saved: RuntimeSettings; form: SettingsForm; error: string | null; saving: boolean; savedTick: number };

export type PanelAction =
  | { type: "loaded"; settings: RuntimeSettings }
  | { type: "loadFailed"; error: string }
  | { type: "edit"; form: SettingsForm }
  | { type: "saveStarted" }
  | { type: "saveFailed"; error: string }
  | { type: "saved"; settings: RuntimeSettings };

export const initialPanel: PanelState = { phase: "loading" };

/** 保存失败时草稿和原设置都不动；保存成功后以后端返回值为准。 */
export function panelReducer(state: PanelState, action: PanelAction): PanelState {
  switch (action.type) {
    case "loaded": return { phase: "ready", saved: action.settings, form: toForm(action.settings), error: null, saving: false, savedTick: 0 };
    case "loadFailed": return { phase: "load_failed", error: action.error };
    case "edit": return state.phase === "ready" ? { ...state, form: action.form } : state;
    case "saveStarted": return state.phase === "ready" ? { ...state, saving: true, error: null } : state;
    case "saveFailed": return state.phase === "ready" ? { ...state, saving: false, error: action.error } : state;
    case "saved": return state.phase === "ready"
      ? { ...state, saving: false, error: null, saved: action.settings, form: toForm(action.settings), savedTick: state.savedTick + 1 }
      : state;
  }
}
