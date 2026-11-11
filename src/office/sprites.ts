import type { AgentStatus } from "../types";
import type { IdleAct } from "./layout";

/** 一张像素图：每个字符一个像素。`.` 透明，`C` 是成员配色，其余查调色板。 */
export type Sprite = readonly string[];
/** 相对工位左上角的一层图。 */
export interface Layer { sprite: Sprite; dx: number; dy: number }

export const PALETTE: Record<string, string> = {
  H: "#2b2b3a", // 头发
  S: "#f1c9a5", // 皮肤
  D: "#1e293b", // 深色描边、眼睛、关着的屏幕
  W: "#f8fafc", // 气泡
  K: "#8b5a2b", // 桌面
  L: "#6b4423", // 桌腿与桌沿
  M: "#475569", // 显示器外框
  B: "#38bdf8", // 屏幕
  G: "#bae6fd", // 屏幕亮处
  R: "#ef4444", // 感叹号
  Y: "#facc15", // 杯子
  Z: "#94a3b8", // 热气与 Z
};

interface Pose { mouthOpen?: boolean; eyesClosed?: boolean; handsOut?: boolean }
function person({ mouthOpen = false, eyesClosed = false, handsOut = false }: Pose): Sprite {
  return [
    "...HHHHHH...",
    "..HHHHHHHH..",
    "..HSSSSSSH..",
    eyesClosed ? "..DDSSSSDD.." : "..SDSSSSDS..",
    "..SSSSSSSS..",
    mouthOpen ? "...SDWWDS..." : "...SSDDSS...",
    "....SSSS....",
    "..CCCCCCCC..",
    ".CCCCCCCCCC.",
    ".CCCCCCCCCC.",
    handsOut ? "S.CCCCCCCC.S" : ".SCCCCCCCCS.",
    "..CCCCCCCC..",
  ];
}
const PERSON = person({});
const PERSON_TALK = person({ mouthOpen: true });
const PERSON_NAP = person({ eyesClosed: true });
const PERSON_TYPE = person({ handsOut: true });

function monitor(screen: readonly [string, string, string, string]): Sprite {
  return ["MMMMMMMMMM", ...screen.map((row) => `M${row}M`), "MMMMMMMMMM", "....MM....", "...MMMM..."];
}
const MONITOR_OFF = monitor(["DDDDDDDD", "DDDDDDDD", "DDDDDDDD", "DDDDDDDD"]);
const MONITOR_A = monitor(["BBBBBBBB", "BGGGGBBB", "BBBBBBBB", "BGGBBBBB"]);
const MONITOR_B = monitor(["BBBBBBBB", "BGGGGGGB", "BGGGBBBB", "BBBBBBBB"]);

const DESK: Sprite = [
  "K".repeat(40),
  "K".repeat(40),
  "L".repeat(40),
  ...Array.from({ length: 5 }, () => `LL${".".repeat(36)}LL`),
];

function bubble(second: string, third: string): Sprite {
  return [".WWWWWWWW.", "WWWWWWWWWW", second, third, ".WWWWWWWW.", "..WW......", ".W........"];
}
const BUBBLE_THINKING: readonly Sprite[] = [
  bubble("WWDWWWWWWW", "WWWWWWWWWW"),
  bubble("WWDWWDWWWW", "WWWWWWWWWW"),
  bubble("WWDWWDWWDW", "WWWWWWWWWW"),
];
const BUBBLE_TALKING: readonly Sprite[] = [
  bubble("WDDDDDDDDW", "WDDDDWWWWW"),
  bubble("WDDDDDDWWW", "WDDDDDDDDW"),
];
const MARK_ERROR: readonly Sprite[] = [
  [".RR.", ".RR.", ".RR.", ".RR.", ".RR.", "....", ".RR.", ".RR."],
  ["RRRR", "RRRR", ".RR.", ".RR.", ".RR.", "....", "RRRR", "RRRR"],
];
const MUG: readonly Sprite[] = [
  [".Z..", "YYY.", "YYYY", "YYY."],
  ["..Z.", "YYY.", "YYYY", "YYY."],
];
const ZZZ: readonly Sprite[] = [
  ["..ZZZ.", "...Z..", "..ZZZ.", "......", "......"],
  ["......", "...ZZZ", "....Z.", "...ZZZ", "......"],
];
const PHONE: readonly Sprite[] = [
  ["DDD", "DBD", "DBD", "DBD", "DDD"],
  ["DDD", "DGD", "DGD", "DGD", "DDD"],
];

/** 供测试检查矩形与调色板。 */
export const ALL_SPRITES: readonly Sprite[] = [
  PERSON, PERSON_TALK, PERSON_NAP, PERSON_TYPE, MONITOR_OFF, MONITOR_A, MONITOR_B, DESK,
  ...BUBBLE_THINKING, ...BUBBLE_TALKING, ...MARK_ERROR, ...MUG, ...ZZZ, ...PHONE,
];

/** 名字的锚点：水平居中，画在桌子下方。 */
export const NAME_OFFSET = { x: 32, y: 54 } as const;

// 以下坐标都相对工位左上角，工位边长 64。
const AT = {
  person: { dx: 20, dy: 22 },
  monitor: { dx: 40, dy: 26 },
  desk: { dx: 12, dy: 34 },
  bubble: { dx: 22, dy: 12 },
  mark: { dx: 24, dy: 12 },
  zzz: { dx: 32, dy: 14 },
  mug: { dx: 14, dy: 30 },
  phone: { dx: 15, dy: 29 },
} as const;

function pick<T>(frames: readonly T[], tick: number): T {
  return frames[((tick % frames.length) + frames.length) % frames.length];
}
const at = (sprite: Sprite, where: { dx: number; dy: number }): Layer => ({ sprite, dx: where.dx, dy: where.dy });

/** 没有成员的工位：只有桌子和关着的电脑。 */
export function emptySeatLayers(): Layer[] {
  return [at(MONITOR_OFF, AT.monitor), at(DESK, AT.desk)];
}

/** 一个工位在某一帧要画的全部图层，按从后到前的顺序。约每秒 4 个 tick。 */
export function seatLayers(status: AgentStatus, act: IdleAct, tick: number): Layer[] {
  switch (status) {
    case "thinking":
      return [at(PERSON, AT.person), at(MONITOR_A, AT.monitor), at(DESK, AT.desk), at(pick(BUBBLE_THINKING, tick), AT.bubble)];
    case "talking":
      return [at(pick([PERSON, PERSON_TALK], tick), AT.person), at(MONITOR_A, AT.monitor), at(DESK, AT.desk), at(pick(BUBBLE_TALKING, tick), AT.bubble)];
    case "working":
      return [at(pick([PERSON, PERSON_TYPE], tick), AT.person), at(pick([MONITOR_A, MONITOR_B], tick), AT.monitor), at(DESK, AT.desk)];
    case "error":
      return [at(PERSON, AT.person), at(MONITOR_OFF, AT.monitor), at(DESK, AT.desk), at(pick(MARK_ERROR, tick), AT.mark)];
    default:
      if (act === "nap") return [at(PERSON_NAP, AT.person), at(MONITOR_OFF, AT.monitor), at(DESK, AT.desk), at(pick(ZZZ, tick), AT.zzz)];
      if (act === "phone") return [at(PERSON, AT.person), at(MONITOR_OFF, AT.monitor), at(DESK, AT.desk), at(pick(PHONE, tick), AT.phone)];
      return [at(PERSON, AT.person), at(MONITOR_OFF, AT.monitor), at(DESK, AT.desk), at(pick(MUG, tick), AT.mug)];
  }
}
