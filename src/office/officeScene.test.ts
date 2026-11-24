import { describe, expect, it } from "vitest";
import {
  ROOM,
  ROOM_HEIGHT,
  ROOM_WIDTH,
  WORK_SEATS,
  cellPoint,
  createActor,
  findRoomPath,
  getOfficeViewport,
  getRegionCamera,
  isWalkable,
  isoProject,
  officeFrame,
  stepActor,
  syncActors,
} from "./officeScene";
import { MEETING_SEATS, OFFICE_REGIONS, REST_SEATS } from "./studioMap";

describe("2.5D 等轴测工作室角色与投影", () => {
  it("等轴测投影落在画幅内，海拔升高向上偏移，前后景深度单调递增", () => {
    expect([ROOM.cols, ROOM.rows, ROOM.cellSize]).toEqual([32, 32, 16]);
    expect([ROOM_WIDTH, ROOM_HEIGHT]).toEqual([724, 468]);
    const north = isoProject(0, 0, 0);
    const east = isoProject(32, 0, 0);
    const west = isoProject(0, 32, 0);
    const south = isoProject(32, 32, 0);
    expect(west.x).toBeGreaterThanOrEqual(0);
    expect(east.x).toBeLessThanOrEqual(ROOM_WIDTH);
    expect(north.y).toBeGreaterThanOrEqual(40);
    expect(south.y).toBeLessThanOrEqual(ROOM_HEIGHT);
    expect(west.x).toBeLessThan(north.x);
    expect(north.x).toBeLessThan(east.x);
    expect(north.y).toBeLessThan(south.y);
    expect(isoProject(10, 10, 24).y).toBeLessThan(isoProject(10, 10, 0).y);
  });

  it("首座位于组长主控台，其余七座错落分布在三个协作岛，不呈排排坐矩阵", () => {
    expect(WORK_SEATS).toHaveLength(8);
    expect(WORK_SEATS.every(isWalkable)).toBe(true);
    const distinctRows = new Set(WORK_SEATS.map((seat) => seat.row));
    expect(distinctRows.size).toBeGreaterThanOrEqual(4);
  });

  it("成员列表重排不影响 seatIndex 对应的工位，删除成员清理角色", () => {
    const actors = new Map();
    syncActors(actors, [{ id: "a", seatIndex: 7 }, { id: "b", seatIndex: 0 }], {});
    expect(actors.get("a").home).toEqual(WORK_SEATS[7]);
    const first = actors.get("a");
    syncActors(actors, [{ id: "b", seatIndex: 0 }, { id: "a", seatIndex: 7 }], { a: "working" });
    expect(actors.get("a")).toBe(first);
    expect(actors.get("a").status).toBe("working");
    syncActors(actors, [{ id: "b", seatIndex: 0 }], {});
    expect(actors.has("a")).toBe(false);
    expect(() => createActor("invalid", 8)).toThrow();
    expect(() => createActor("invalid", -1)).toThrow();
  });

  it("动作帧按移动、工作与思考区分，左向支持镜像标记", () => {
    expect(officeFrame("working", 0, "up", false)).toMatchObject({ sx: 48, sy: 32, flip: false });
    expect(officeFrame("working", 1, "up", false).sx).toBe(64);
    expect(officeFrame("thinking", 0, "down", false).sx).toBe(80);
    expect(officeFrame("thinking", 1, "down", false).sx).toBe(96);
    expect(officeFrame("idle", 0, "left", true)).toMatchObject({ sx: 0, sy: 64, flip: true });
  });

  it("跨区域路径逐格连通、不穿隔断与家具，非法目标不可达", () => {
    const from = WORK_SEATS[0];
    const to = REST_SEATS[6];
    const path = findRoomPath(from, to);
    expect(path.length).toBeGreaterThan(10);
    expect(path[path.length - 1]).toEqual({ col: to.col, row: to.row });
    let previous = from;
    for (const cell of path) {
      expect(isWalkable(cell)).toBe(true);
      expect(Math.abs(cell.col - previous.col) + Math.abs(cell.row - previous.row)).toBe(1);
      previous = { ...from, ...cell };
    }
    expect(findRoomPath(from, { col: 0, row: 0 })).toEqual([]);
    expect(findRoomPath(from, { col: -1, row: 12 })).toEqual([]);
    expect(findRoomPath(from, from)).toEqual([]);
  });

  it("休息中的成员开始工作后沿路径返回自己的工位，不瞬移或越过目标", () => {
    const actor = createActor("a", 0);
    actor.cell = { col: REST_SEATS[0].col, row: REST_SEATS[0].row };
    Object.assign(actor, cellPoint(actor.cell));
    actor.destination = "rest";
    const actors = new Map([[actor.id, actor]]);
    const start = { x: actor.x, y: actor.y };
    syncActors(actors, [{ id: "a", seatIndex: 0 }], { a: "working" });
    expect(actor.destination).toBe("desk");
    expect(actor.path.length).toBeGreaterThan(0);
    stepActor(actor, 0.05, true);
    expect(Math.hypot(actor.x - start.x, actor.y - start.y)).toBeLessThanOrEqual(4.01);
    for (let frame = 0; frame < 2000; frame += 1) stepActor(actor, 0.05, true);
    expect([actor.x, actor.y]).toEqual([cellPoint(actor.home).x, cellPoint(actor.home).y]);
    expect(actor.path).toEqual([]);
    expect(actor.facing).toBe(actor.home.facing);
  });

  it("减少动态效果时停止自主走动，角色保持在固定工位", () => {
    const actor = createActor("a", 0);
    actor.cell = { col: REST_SEATS[0].col, row: REST_SEATS[0].row };
    Object.assign(actor, cellPoint(actor.cell));
    actor.path = findRoomPath(actor.cell, actor.home);
    stepActor(actor, 100, false);
    expect(actor.path).toEqual([]);
    expect([actor.x, actor.y]).toEqual([cellPoint(actor.home).x, cellPoint(actor.home).y]);
  });

  it("走到半格时切换状态，先沿原路退回网格中心再转向", () => {
    const actor = createActor("a", 0);
    actor.cell = { col: REST_SEATS[0].col, row: REST_SEATS[0].row };
    Object.assign(actor, cellPoint(actor.cell));
    actor.x += 4;
    actor.destination = "rest";
    syncActors(new Map([[actor.id, actor]]), [{ id: "a", seatIndex: 0 }], { a: "working" });
    expect(actor.path[0]).toEqual(actor.cell);
  });

  it("发言时前往研讨圆桌独立座位，发言结束回原工位，不改写真实状态", () => {
    const actor = createActor("speaker", 2);
    const actors = new Map([[actor.id, actor]]);
    const members = [{ id: actor.id, seatIndex: actor.seatIndex }];
    syncActors(actors, members, { speaker: "talking" });
    expect(actor.destination).toBe("meeting");
    for (let frame = 0; frame < 2000; frame += 1) stepActor(actor, 0.05, true);
    expect([actor.x, actor.y]).toEqual([cellPoint(MEETING_SEATS[2]).x, cellPoint(MEETING_SEATS[2]).y]);
    expect(actor.facing).toBe(MEETING_SEATS[2].facing);
    expect(actor.status).toBe("talking");
    syncActors(actors, members, {});
    expect(actor.destination).toBe("desk");
    expect(actor.status).toBe("idle");
    syncActors(actors, members, { speaker: "working" });
    for (let frame = 0; frame < 2000; frame += 1) stepActor(actor, 0.05, true);
    expect([actor.x, actor.y]).toEqual([cellPoint(actor.home).x, cellPoint(actor.home).y]);
  });

  it("八名空闲成员都有独立休息点，前四位去咖啡图书角、后四位去露台花园", () => {
    for (let seat = 0; seat < 8; seat += 1) {
      const actor = createActor(`idle-${seat}`, seat);
      for (let frame = 0; frame < 190 && actor.destination === "desk"; frame += 1) stepActor(actor, 0.1, true);
      expect(actor.destination).toBe("rest");
      expect(actor.path[actor.path.length - 1]).toEqual({ col: REST_SEATS[seat].col, row: REST_SEATS[seat].row });
    }
  });
});

describe("等轴测沙盘相机", () => {
  it("横竖窗口完整显示沙盘全景，零尺寸不会产生无效坐标", () => {
    for (const [width, height] of [[276, 246], [800, 700], [1200, 500], [0, 0]]) {
      const view = getOfficeViewport(width, height, 1);
      expect(Number.isFinite(view.scale)).toBe(true);
      expect(view.scale).toBeGreaterThan(0);
      expect(view.offsetX).toBeGreaterThanOrEqual(0);
      expect(view.offsetY).toBeGreaterThanOrEqual(0);
      expect(view.offsetX + ROOM_WIDTH * view.scale).toBeLessThanOrEqual(Math.max(1, width) + 1);
      expect(view.offsetY + ROOM_HEIGHT * view.scale).toBeLessThanOrEqual(Math.max(1, height) + 1);
    }
  });

  it("放大后拖动始终保留沙盘边界，复位恢复居中", () => {
    const view = getOfficeViewport(724, 468, 4, { x: -9999, y: 9999 });
    expect(view.offsetX).toBe(0);
    expect(view.offsetY + ROOM_HEIGHT * view.scale).toBe(468);
    const reset = getOfficeViewport(724, 468, 1, view.center);
    expect(reset.center).toEqual({ x: ROOM_WIDTH / 2, y: ROOM_HEIGHT / 2 });
  });

  it("区域定位在横竖窗口中都完整框住等轴测菱形区域（含立体高度）", () => {
    for (const [width, height] of [[960, 600], [400, 700], [260, 220]]) {
      for (const region of OFFICE_REGIONS) {
        const camera = getRegionCamera(width, height, region);
        const view = getOfficeViewport(width, height, camera.zoom, camera.center);
        const left = isoProject(region.col, region.row + region.rows, 0).x;
        const right = isoProject(region.col + region.cols, region.row, 0).x;
        const top = isoProject(region.col, region.row, 32).y;
        const bottom = isoProject(region.col + region.cols, region.row + region.rows, 0).y;
        expect(view.offsetX + left * view.scale).toBeGreaterThanOrEqual(-1);
        expect(view.offsetY + top * view.scale).toBeGreaterThanOrEqual(-1);
        expect(view.offsetX + right * view.scale).toBeLessThanOrEqual(width + 1);
        expect(view.offsetY + bottom * view.scale).toBeLessThanOrEqual(height + 1);
      }
    }
  });
});
