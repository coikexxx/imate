import { describe, expect, it } from "vitest";
import { CELL, LOGICAL_H, LOGICAL_W, SHIRT_COLORS, idleAct, integerScale, seatOrigin, shirtColor } from "./layout";

describe("工位排布", () => {
  it("八个工位排成四列两行，互不重叠且都在画布内", () => {
    const origins = Array.from({ length: 8 }, (_, seat) => seatOrigin(seat)!);
    expect(origins[0]).toEqual({ x: 0, y: 0 });
    expect(origins[3]).toEqual({ x: 192, y: 0 });
    expect(origins[4]).toEqual({ x: 0, y: 64 });
    expect(origins[7]).toEqual({ x: 192, y: 64 });
    expect(new Set(origins.map((o) => `${o.x},${o.y}`)).size).toBe(8);
    for (const origin of origins) {
      expect(origin.x + CELL).toBeLessThanOrEqual(LOGICAL_W);
      expect(origin.y + CELL).toBeLessThanOrEqual(LOGICAL_H);
    }
  });
  it("超出范围或不是整数的工位号没有位置", () => {
    for (const bad of [-1, 8, 1.5, Number.NaN]) expect(seatOrigin(bad)).toBeNull();
  });
  it("缩放取能放下的最大整数倍，最小为 1", () => {
    expect(integerScale(256, 128)).toBe(1);
    expect(integerScale(800, 300)).toBe(2);
    expect(integerScale(1024, 512)).toBe(4);
    expect(integerScale(100, 50)).toBe(1);
    expect(integerScale(0, 0)).toBe(1);
  });
  it("同一个成员的配色固定", () => {
    expect(shirtColor("agent-1")).toBe(shirtColor("agent-1"));
    expect(SHIRT_COLORS).toContain(shirtColor("agent-2"));
    expect(new Set(SHIRT_COLORS).size).toBe(8);
  });
  it("摸鱼动作在一个时间段内稳定，三种都会出现", () => {
    expect(idleAct("agent-1", 1_000)).toBe(idleAct("agent-1", 11_999));
    const seen = new Set(Array.from({ length: 200 }, (_, index) => idleAct("agent-1", index * 12_000)));
    expect([...seen].sort()).toEqual(["coffee", "nap", "phone"]);
  });
});
