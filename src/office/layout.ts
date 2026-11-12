export const SEAT_COLUMNS = 4;
export const SEAT_ROWS = 2;
export const MAX_SEATS = SEAT_COLUMNS * SEAT_ROWS;
/** 一个工位的逻辑宽高（像素）。 */
export const CELL_W = 64;
export const CELL_H = 48;
/** 后墙的最小高度，以及第一排工位压进墙面的高度（那一段只画气泡）。 */
const WALL_MIN = 32;
const WALL_MAX = 64;
const WALL_OVERLAP = 8;
/** 能完整放下八个工位和后墙的最小逻辑场景。 */
export const MIN_W = CELL_W * SEAT_COLUMNS + 8;
export const MIN_H = WALL_MIN - WALL_OVERLAP + CELL_H * SEAT_ROWS + 4;

export interface Point { x: number; y: number }
/** 一帧场景的排布：逻辑尺寸随容器变化，乘以 scale 后铺满容器。 */
export interface Scene {
  scale: number;
  width: number;
  height: number;
  /** 后墙高度，墙以下是地板。 */
  wallH: number;
  /** 按工位号排列的工位左上角。 */
  seats: Point[];
}

const clamp = (value: number, low: number, high: number) => Math.min(high, Math.max(low, value));

/** 能放下最小场景的最大整数倍数，保证像素不被拉糊。 */
export function integerScale(width: number, height: number): number {
  return Math.max(1, Math.floor(Math.min(width / MIN_W, height / MIN_H)) || 1);
}

/** 按容器尺寸排布场景：整数倍放大，逻辑尺寸取整到刚好盖住容器，工位区居中。 */
export function sceneLayout(containerW: number, containerH: number): Scene {
  const scale = integerScale(containerW, containerH);
  const width = Math.max(MIN_W, Math.ceil(containerW / scale));
  const height = Math.max(MIN_H, Math.ceil(containerH / scale));
  const wallH = clamp(Math.round(height * 0.26), WALL_MIN, WALL_MAX);

  const gapX = clamp(Math.floor((width - CELL_W * SEAT_COLUMNS) / 8), 0, 24);
  const blockW = CELL_W * SEAT_COLUMNS + gapX * (SEAT_COLUMNS - 1);
  const left = Math.floor((width - blockW) / 2);

  const rowsTop = wallH - WALL_OVERLAP;
  const spareY = height - rowsTop - CELL_H * SEAT_ROWS;
  const gapY = clamp(Math.floor(spareY / 3), 0, 18);
  const top = rowsTop + Math.floor((spareY - gapY) / 2);

  const seats = Array.from({ length: MAX_SEATS }, (_, seat) => ({
    x: left + (seat % SEAT_COLUMNS) * (CELL_W + gapX),
    y: top + Math.floor(seat / SEAT_COLUMNS) * (CELL_H + gapY),
  }));
  return { scale, width, height, wallH, seats };
}

/** 32 位 FNV-1a，用来从 ID 得到稳定的配色和动作。 */
export function hashOf(text: string): number {
  let hash = 0x811c9dc5;
  for (let index = 0; index < text.length; index += 1) {
    hash ^= text.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}

export const SHIRT_COLORS = [
  "#38bdf8", "#f472b6", "#a3e635", "#fbbf24", "#c084fc", "#fb7185", "#34d399", "#f97316",
] as const;

/** 每个成员按 ID 取一个固定配色。 */
export function shirtColor(agentId: string): string {
  return SHIRT_COLORS[hashOf(agentId) % SHIRT_COLORS.length];
}

export const DESK_ITEMS = ["plant", "books", "lamp", "frame"] as const;
export type DeskItem = (typeof DESK_ITEMS)[number];
/** 每个成员按 ID 在桌上固定摆一件小物。 */
export function deskItem(agentId: string): DeskItem {
  return DESK_ITEMS[hashOf(`${agentId}:desk`) % DESK_ITEMS.length];
}

export type IdleAct = "coffee" | "nap" | "phone";
const IDLE_ACTS: readonly IdleAct[] = ["coffee", "nap", "phone"];
/** 摸鱼动作每 12 秒换一次。同一时间段内结果稳定，便于测试。 */
export function idleAct(agentId: string, nowMs: number): IdleAct {
  const bucket = Math.floor(nowMs / 12_000);
  return IDLE_ACTS[hashOf(`${agentId}:${bucket}`) % IDLE_ACTS.length];
}
