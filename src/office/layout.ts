export const SEAT_COLUMNS = 4;
export const SEAT_ROWS = 2;
export const MAX_SEATS = SEAT_COLUMNS * SEAT_ROWS;
/** 一个工位的逻辑边长（像素）。 */
export const CELL = 64;
export const LOGICAL_W = CELL * SEAT_COLUMNS;
export const LOGICAL_H = CELL * SEAT_ROWS;

/** 工位左上角的逻辑坐标。工位号不合法时返回 null。 */
export function seatOrigin(seatIndex: number): { x: number; y: number } | null {
  if (!Number.isInteger(seatIndex) || seatIndex < 0 || seatIndex >= MAX_SEATS) return null;
  return { x: (seatIndex % SEAT_COLUMNS) * CELL, y: Math.floor(seatIndex / SEAT_COLUMNS) * CELL };
}

/** 能放进容器的最大整数倍数，保证像素不被拉糊。 */
export function integerScale(width: number, height: number): number {
  return Math.max(1, Math.floor(Math.min(width / LOGICAL_W, height / LOGICAL_H)) || 1);
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

export type IdleAct = "coffee" | "nap" | "phone";
const IDLE_ACTS: readonly IdleAct[] = ["coffee", "nap", "phone"];
/** 摸鱼动作每 12 秒换一次。同一时间段内结果稳定，便于测试。 */
export function idleAct(agentId: string, nowMs: number): IdleAct {
  const bucket = Math.floor(nowMs / 12_000);
  return IDLE_ACTS[hashOf(`${agentId}:${bucket}`) % IDLE_ACTS.length];
}
