import claude from "../assets/cli-icons/claude.svg";
import codex from "../assets/cli-icons/codex.svg";
import copilot from "../assets/cli-icons/copilot.svg";
import cursor from "../assets/cli-icons/cursor.svg";
import dsh from "../assets/cli-icons/dsh.svg";
import gemini from "../assets/cli-icons/gemini.svg";
import grok from "../assets/cli-icons/grok.svg";
import hermes from "../assets/cli-icons/hermes.svg";
import kimi from "../assets/cli-icons/kimi.svg";
import openclaw from "../assets/cli-icons/openclaw.svg";
import opencode from "../assets/cli-icons/opencode.svg";
import qwen from "../assets/cli-icons/qwen.svg";

export interface CliIconInfo {
  src: string;
  /** 单色图标（SVG 里只有 currentColor，用 <img> 显示是黑色）：深色界面里要反相成白色 */
  mono: boolean;
}

// 来源与许可见 src/assets/cli-icons/README.md。kimi 混有固定蓝色，反相会变色，不标 mono。
const ICONS = new Map<string, CliIconInfo>(Object.entries({
  claude: { src: claude, mono: false },
  codex: { src: codex, mono: false },
  grok: { src: grok, mono: true },
  dsh: { src: dsh, mono: false },
  gemini: { src: gemini, mono: false },
  opencode: { src: opencode, mono: true },
  openclaw: { src: openclaw, mono: false },
  hermes: { src: hermes, mono: true },
  cursor: { src: cursor, mono: true },
  qwen: { src: qwen, mono: false },
  copilot: { src: copilot, mono: true },
  kimi: { src: kimi, mono: false },
}));

/** 按 CLI 的 id 取 logo。Pi 和未知 id 没有图标，返回 null（界面改用字母徽标） */
export function cliIconInfo(id: string): CliIconInfo | null {
  return ICONS.get(id) ?? null;
}
