import { describe, expect, it } from "vitest";
import { DOORWAYS, FURNITURE, GBA_ROOM, MEETING_SEATS, OFFICE_REGIONS, REST_SEATS, WORK_SEATS } from "./gbaOfficeMap";

const walkable = (col: number, row: number) => GBA_ROOM.tiles[row * GBA_ROOM.cols + col] === 1;

describe("GBA 分区办公室地图", () => {
  it("五区域互不重叠，全部落在 32×32 的地图内", () => {
    expect([GBA_ROOM.cols, GBA_ROOM.rows, GBA_ROOM.cellSize]).toEqual([32, 32, 16]);
    expect(OFFICE_REGIONS.map((region) => region.name)).toEqual(["办公区", "摸鱼区", "阳台", "会议室", "前台"]);
    for (const region of OFFICE_REGIONS) {
      expect(region.col).toBeGreaterThan(0);
      expect(region.row).toBeGreaterThan(0);
      expect(region.col + region.cols).toBeLessThanOrEqual(32);
      expect(region.row + region.rows).toBeLessThanOrEqual(32);
      for (const other of OFFICE_REGIONS) {
        if (region.id === other.id) continue;
        const overlap = region.col < other.col + other.cols && region.col + region.cols > other.col
          && region.row < other.row + other.rows && region.row + region.rows > other.row;
        expect(overlap).toBe(false);
      }
    }
  });

  it("每名成员都有独立工位、会议座位与休息点，且座位可站立", () => {
    const inside = (cell: { col: number; row: number }, ids: string[]) => OFFICE_REGIONS.some((region) =>
      ids.includes(region.id) && cell.col > region.col && cell.col < region.col + region.cols - 1
      && cell.row > region.row && cell.row < region.row + region.rows - 1);
    for (const [seats, ids] of [[WORK_SEATS, ["office"]], [MEETING_SEATS, ["meeting"]], [REST_SEATS, ["lounge", "balcony"]]] as const) {
      expect(seats).toHaveLength(8);
      expect(new Set(seats.map((seat) => `${seat.col},${seat.row}`)).size).toBe(8);
      for (const seat of seats) { expect(walkable(seat.col, seat.row)).toBe(true); expect(inside(seat, [...ids])).toBe(true); }
    }
  });

  it("墙体与家具占地阻挡行走，门洞保持通行", () => {
    for (const furniture of FURNITURE) {
      for (let row = furniture.row; row < furniture.row + furniture.rows; row += 1) {
        for (let col = furniture.col; col < furniture.col + furniture.cols; col += 1) expect(walkable(col, row)).toBe(false);
      }
    }
    expect(walkable(1, 1)).toBe(false);
    expect(walkable(0, 0)).toBe(false);
    for (const door of DOORWAYS) expect(walkable(door.col, door.row)).toBe(true);
  });

  it("从前台入口可达每个工位、会议座位、休息点与区域", () => {
    const reached = new Set<string>();
    const queue = [{ col: 9, row: 29 }];
    for (let i = 0; i < queue.length; i += 1) {
      const cell = queue[i];
      const key = `${cell.col},${cell.row}`;
      if (reached.has(key) || !walkable(cell.col, cell.row)) continue;
      reached.add(key);
      for (const [dx, dy] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
        const col = cell.col + dx; const row = cell.row + dy;
        if (col >= 0 && row >= 0 && col < 32 && row < 32) queue.push({ col, row });
      }
    }
    for (const seat of [...WORK_SEATS, ...MEETING_SEATS, ...REST_SEATS]) expect(reached.has(`${seat.col},${seat.row}`)).toBe(true);
    for (const region of OFFICE_REGIONS) expect([...reached].some((key) => {
      const [col, row] = key.split(",").map(Number);
      return col > region.col && col < region.col + region.cols - 1 && row > region.row && row < region.row + region.rows - 1;
    })).toBe(true);
  });
});
