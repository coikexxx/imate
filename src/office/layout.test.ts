import { describe, expect, it } from "vitest";
import {
  CELL_H, CELL_W, DESK_ITEMS, MIN_H, MIN_W, SHIRT_COLORS, deskItem, idleAct, integerScale, sceneLayout, shirtColor,
} from "./layout";

describe("场景排布", () => {
  it("场景铺满容器：逻辑尺寸乘以倍数不小于容器，且不多出一整格", () => {
    for (const [width, height] of [[676, 280], [1316, 390], [900, 640], [264, 124]] as const) {
      const scene = sceneLayout(width, height);
      expect(scene.width * scene.scale).toBeGreaterThanOrEqual(width);
      expect(scene.height * scene.scale).toBeGreaterThanOrEqual(height);
      expect((scene.width - 1) * scene.scale).toBeLessThan(width);
      expect((scene.height - 1) * scene.scale).toBeLessThan(height);
    }
  });
  it("常见窗口下放大 2 倍，逻辑尺寸随容器变化", () => {
    const scene = sceneLayout(676, 280);
    expect(scene.scale).toBe(2);
    expect(scene.width).toBe(338);
    expect(scene.height).toBe(140);
  });
  it("八个工位排成四列两行，互不重叠且都在场景内", () => {
    for (const [width, height] of [[676, 280], [1316, 390], [900, 640], [264, 124], [100, 50]] as const) {
      const scene = sceneLayout(width, height);
      expect(scene.seats).toHaveLength(8);
      for (let column = 0; column < 4; column += 1) expect(scene.seats[column].x).toBe(scene.seats[column + 4].x);
      for (let seat = 1; seat < 4; seat += 1) {
        expect(scene.seats[seat].y).toBe(scene.seats[0].y);
        expect(scene.seats[seat].x).toBeGreaterThanOrEqual(scene.seats[seat - 1].x + CELL_W);
      }
      expect(scene.seats[4].y).toBeGreaterThanOrEqual(scene.seats[0].y + CELL_H);
      for (const seat of scene.seats) {
        expect(Number.isInteger(seat.x) && Number.isInteger(seat.y)).toBe(true);
        expect(seat.x).toBeGreaterThanOrEqual(0);
        expect(seat.y).toBeGreaterThanOrEqual(0);
        expect(seat.x + CELL_W).toBeLessThanOrEqual(scene.width);
        expect(seat.y + CELL_H).toBeLessThanOrEqual(scene.height);
      }
      expect(scene.wallH).toBeGreaterThan(0);
      expect(scene.wallH).toBeLessThan(scene.height);
    }
  });
  it("工位区左右居中", () => {
    const scene = sceneLayout(1316, 390);
    const left = scene.seats[0].x;
    const right = scene.width - (scene.seats[3].x + CELL_W);
    expect(Math.abs(left - right)).toBeLessThanOrEqual(1);
  });
  it("缩放取能放下最小场景的最大整数倍，最小为 1", () => {
    expect(integerScale(MIN_W, MIN_H)).toBe(1);
    expect(integerScale(676, 280)).toBe(2);
    expect(integerScale(MIN_W * 5, MIN_H * 5)).toBe(5);
    expect(integerScale(100, 50)).toBe(1);
    expect(integerScale(0, 0)).toBe(1);
  });
  it("同一个成员的配色和桌面小物固定", () => {
    expect(shirtColor("agent-1")).toBe(shirtColor("agent-1"));
    expect(SHIRT_COLORS).toContain(shirtColor("agent-2"));
    expect(new Set(SHIRT_COLORS).size).toBe(8);
    expect(deskItem("agent-1")).toBe(deskItem("agent-1"));
    const seen = new Set(Array.from({ length: 200 }, (_, index) => deskItem(`agent-${index}`)));
    expect([...seen].sort()).toEqual([...DESK_ITEMS].sort());
  });
  it("摸鱼动作在一个时间段内稳定，三种都会出现", () => {
    expect(idleAct("agent-1", 1_000)).toBe(idleAct("agent-1", 11_999));
    const seen = new Set(Array.from({ length: 200 }, (_, index) => idleAct("agent-1", index * 12_000)));
    expect([...seen].sort()).toEqual(["coffee", "nap", "phone"]);
  });
});
