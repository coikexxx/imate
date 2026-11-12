import type { AgentStatus } from "../types";
import type { DeskItem, IdleAct } from "./layout";

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
  T: "#b07a4a", // 桌面高光
  P: "#74502f", // 桌子前挡板
  A: "#64748b", // 椅背描边
  V: "#3b4a63", // 椅背
  F: "#34425e", // 隔板
  E: "#5b6e94", // 隔板包边
  N: "#f472b6", // 便签、书
  Q: "#cbd5e1", // 键帽、鼠标
  O: "#4ade80", // 电源灯、叶子
  U: "#16a34a", // 叶子暗部
  X: "#c2410c", // 花盆
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

function monitor(screen: readonly [string, string, string, string, string], led: string): Sprite {
  return ["MMMMMMMMMMMM", ...screen.map((row) => `M${row}M`), `MMMMMMMMM${led}MM`, "....MMMM....", "...MMMMMM..."];
}
const MONITOR_OFF = monitor(["DDDDDDDDDD", "DDDDDDDDDD", "DDDDDDDDDD", "DDDDDDDDDD", "DDDDDDDDDD"], "D");
const MONITOR_A = monitor(["BBBBBBBBBB", "BGGGGGBBBB", "BBBBBBBBBB", "BGGGBBBBBB", "BBBBBBBBBB"], "O");
const MONITOR_B = monitor(["BBBBBBBBBB", "BGGGGGGGBB", "BGGGGBBBBB", "BBBBBBBBBB", "BGGBBBBBBB"], "O");
const MONITOR_ERROR = monitor(["DDDDDDDDDD", "DDDRDDRDDD", "DDDDRRDDDD", "DDDRDDRDDD", "DDDDDDDDDD"], "R");

/** 把一小块不透明的图贴到底图上。 */
function stamp(base: Sprite, patch: Sprite, x: number, y: number): Sprite {
  return base.map((row, index) => {
    const line = patch[index - y];
    return line ? row.slice(0, x) + line + row.slice(x + line.length) : row;
  });
}

/** 桌子：高光、桌面、桌沿、前挡板、桌脚。前挡板中间留给名牌。 */
const DESK: Sprite = [
  "T".repeat(48),
  ...Array.from({ length: 3 }, () => "K".repeat(48)),
  "L".repeat(48),
  ...Array.from({ length: 11 }, () => `L${"P".repeat(46)}L`),
  ...Array.from({ length: 2 }, () => `LL${".".repeat(44)}LL`),
];

/** 工位背后的隔板，左上角贴着两张便签。 */
const PARTITION: Sprite = stamp(
  stamp(["E".repeat(56), ...Array.from({ length: 11 }, () => "F".repeat(56))], ["YYY", "YYY", "YYY"], 6, 3),
  ["NNN", "NNN", "NNN"], 11, 4,
);

const CHAIR: Sprite = ["..AAAAAAAAAAAA..", ...Array.from({ length: 9 }, () => ".AVVVVVVVVVVVVA.")];

/** 键盘和鼠标，平放在桌面上。 */
const KEYBOARD: Sprite = ["QQQQQQQQQQ.QQ", "MMMMMMMMMM.MM"];

const ITEM_SPRITES: Record<DeskItem, Sprite> = {
  plant: ["..OO..", ".OUOO.", "OOOUOO", ".OUOO.", "..UU..", ".XXXX.", ".XXXX.", "..XX.."],
  books: ["..NNNNN.", "..NNNNN.", ".BBBBBB.", ".BBBBBB.", "YYYYYYY.", "YYYYYYY."],
  lamp: ["..YYYY.", ".YYYYYY", ".YYYYYY", "....M..", "...M...", "...M...", "...M...", "..MMM..", ".MMMMM."],
  frame: ["LLLLLL", "LGGGGL", "LGOGGL", "LOOOGL", "LLLLLL", "..LL..", ".LLLL."],
};

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
  PERSON, PERSON_TALK, PERSON_NAP, PERSON_TYPE, MONITOR_OFF, MONITOR_A, MONITOR_B, MONITOR_ERROR,
  DESK, PARTITION, CHAIR, KEYBOARD, ...Object.values(ITEM_SPRITES),
  ...BUBBLE_THINKING, ...BUBBLE_TALKING, ...MARK_ERROR, ...MUG, ...ZZZ, ...PHONE,
];

/** 名牌的锚点：水平居中，贴在桌子前挡板上。 */
export const NAME_OFFSET = { x: 32, y: 29 } as const;
/** 名牌高度，以及桌子投影相对工位左上角的位置。 */
export const NAME_PLATE_H = 8;
export const DESK_SHADOW = { dx: 8, dy: 40, w: 48, h: 2 } as const;

// 以下坐标都相对工位左上角，工位宽 64、高 48。桌面上的东西底边都落在 y=24 附近。
const AT = {
  partition: { dx: 4, dy: 10 },
  chair: { dx: 24, dy: 12 },
  person: { dx: 26, dy: 10 },
  desk: { dx: 8, dy: 22 },
  monitor: { dx: 44, dy: 16 },
  keyboard: { dx: 27, dy: 23 },
  bubble: { dx: 36, dy: 1 },
  mark: { dx: 30, dy: 1 },
  zzz: { dx: 38, dy: 4 },
  mug: { dx: 19, dy: 20 },
  phone: { dx: 22, dy: 17 },
} as const;
const ITEM_AT: Record<DeskItem, { dx: number; dy: number }> = {
  plant: { dx: 11, dy: 16 },
  books: { dx: 10, dy: 18 },
  lamp: { dx: 10, dy: 15 },
  frame: { dx: 11, dy: 17 },
};

function pick<T>(frames: readonly T[], tick: number): T {
  return frames[((tick % frames.length) + frames.length) % frames.length];
}
const at = (sprite: Sprite, where: { dx: number; dy: number }): Layer => ({ sprite, dx: where.dx, dy: where.dy });

/** 没有成员的工位：隔板、推进去的椅子、空桌子、关着的电脑和键盘。 */
export function emptySeatLayers(): Layer[] {
  return [at(PARTITION, AT.partition), at(CHAIR, AT.chair), at(DESK, AT.desk), at(MONITOR_OFF, AT.monitor), at(KEYBOARD, AT.keyboard)];
}

/** 一个工位在某一帧要画的全部图层，按从后到前的顺序。约每秒 4 个 tick。 */
export function seatLayers(status: AgentStatus, act: IdleAct, tick: number, item: DeskItem = "plant"): Layer[] {
  const seat = (person: Sprite, screen: Sprite, ...extras: Layer[]): Layer[] => [
    at(PARTITION, AT.partition), at(CHAIR, AT.chair), at(person, AT.person), at(DESK, AT.desk),
    at(screen, AT.monitor), at(KEYBOARD, AT.keyboard), at(ITEM_SPRITES[item], ITEM_AT[item]), ...extras,
  ];
  switch (status) {
    case "thinking":
      return seat(PERSON, MONITOR_A, at(pick(BUBBLE_THINKING, tick), AT.bubble));
    case "talking":
      return seat(pick([PERSON, PERSON_TALK], tick), MONITOR_A, at(pick(BUBBLE_TALKING, tick), AT.bubble));
    case "working":
      return seat(pick([PERSON, PERSON_TYPE], tick), pick([MONITOR_A, MONITOR_B], tick));
    case "error":
      return seat(PERSON, MONITOR_ERROR, at(pick(MARK_ERROR, tick), AT.mark));
    default:
      if (act === "nap") return seat(PERSON_NAP, MONITOR_OFF, at(pick(ZZZ, tick), AT.zzz));
      if (act === "phone") return seat(PERSON, MONITOR_OFF, at(pick(PHONE, tick), AT.phone));
      return seat(PERSON, MONITOR_OFF, at(pick(MUG, tick), AT.mug));
  }
}
