import { describe, expect, it } from "vitest";
import {
  ROOM, ROOM_WIDTH, ROOM_HEIGHT, WORK_SEATS, cellPoint, createActor, findRoomPath, getOfficeViewport, getRegionCamera,
  isWalkable, officeFrame, stepActor, syncActors,
} from "./officeScene";
import { MEETING_SEATS, OFFICE_REGIONS, REST_SEATS } from "./gbaOfficeMap";

describe("GBA 办公室角色", () => {
  it("八个固定工位集中在办公区，按 seatIndex 对应四列两行", () => {
    expect([ROOM.cols, ROOM.rows, ROOM.cellSize]).toEqual([32, 32, 16]);
    expect(WORK_SEATS.map(({ col, row }) => [col, row])).toEqual([
      [4, 7], [8, 7], [12, 7], [16, 7], [4, 13], [8, 13], [12, 13], [16, 13],
    ]);
    expect(WORK_SEATS.every(isWalkable)).toBe(true);
    expect(WORK_SEATS.every((seat) => seat.facing === "up")).toBe(true);
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

  it("帧图使用上游的行列定义，工作与思考动作区分，左向复用右向翻转", () => {
    expect(officeFrame("working", 0, "up", false)).toMatchObject({ sx: 48, sy: 32, flip: false });
    expect(officeFrame("working", 1, "up", false).sx).toBe(64);
    expect(officeFrame("thinking", 0, "down", false).sx).toBe(80);
    expect(officeFrame("thinking", 1, "down", false).sx).toBe(96);
    expect(officeFrame("idle", 0, "left", true)).toMatchObject({ sx: 0, sy: 64, flip: true });
    for (const status of ["idle", "thinking", "talking", "working", "error"] as const) {
      const frame = officeFrame(status, 123, "down", false);
      expect(frame.sx + frame.width).toBeLessThanOrEqual(112);
      expect(frame.sy + frame.height).toBeLessThanOrEqual(96);
    }
  });

  it("跨房间路径逐格连通、不穿墙，非法目标不可达", () => {
    const from = WORK_SEATS[0];
    const to = WORK_SEATS[7];
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

  it("发言时去会议室独立座位，发言结束回办公区，不改写真实状态", () => {
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

  it("八名空闲成员都有对应休息点，前四位去摸鱼区、后四位去阳台", () => {
    for (let seat = 0; seat < 8; seat += 1) {
      const actor = createActor(`idle-${seat}`, seat);
      for (let frame = 0; frame < 190 && actor.destination === "desk"; frame += 1) stepActor(actor, 0.1, true);
      expect(actor.destination).toBe("rest");
      expect(actor.path[actor.path.length - 1]).toEqual({ col: REST_SEATS[seat].col, row: REST_SEATS[seat].row });
    }
  });
});

describe("办公室相机", () => {
  it("横竖窗口完整显示房间，零尺寸不会产生无效坐标", () => {
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

  it("放大后拖动始终保留房间边界，复位恢复居中", () => {
    const view = getOfficeViewport(512, 512, 4, { x: -9999, y: 9999 });
    expect(view.offsetX).toBe(0);
    expect(view.offsetY + ROOM_HEIGHT * view.scale).toBe(512);
    const reset = getOfficeViewport(512, 512, 1, view.center);
    expect(reset.center).toEqual({ x: ROOM_WIDTH / 2, y: ROOM_HEIGHT / 2 });
  });

  it("区域定位在横竖窗口中都完整保留所选区域，且可复位全景", () => {
    for (const [width, height] of [[960, 600], [400, 700], [237, 217]]) {
      for (const region of OFFICE_REGIONS) {
        const camera = getRegionCamera(width, height, region);
        const view = getOfficeViewport(width, height, camera.zoom, camera.center);
        expect(view.offsetX + region.col * 16 * view.scale).toBeGreaterThanOrEqual(-1);
        expect(view.offsetY + region.row * 16 * view.scale).toBeGreaterThanOrEqual(-1);
        expect(view.offsetX + (region.col + region.cols) * 16 * view.scale).toBeLessThanOrEqual(width + 1);
        expect(view.offsetY + (region.row + region.rows) * 16 * view.scale).toBeLessThanOrEqual(height + 1);
      }
    }
  });
});
