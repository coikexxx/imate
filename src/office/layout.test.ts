import { describe, expect, it } from "vitest";
import { idleAct, shirtColor } from "./layout";

describe("成员配色与摸鱼动作", () => {
  it("同一个成员的配色固定，八种配色都会出现", () => {
    expect(shirtColor("agent-1")).toBe(shirtColor("agent-1"));
    const seen = new Set(Array.from({ length: 200 }, (_, index) => shirtColor(`agent-${index}`)));
    expect(seen.size).toBe(8);
    for (const color of seen) expect(color).toMatch(/^#[0-9a-f]{6}$/);
  });
  it("摸鱼动作在一个时间段内稳定，三种都会出现", () => {
    expect(idleAct("agent-1", 1_000)).toBe(idleAct("agent-1", 11_999));
    const seen = new Set(Array.from({ length: 200 }, (_, index) => idleAct("agent-1", index * 12_000)));
    expect([...seen].sort()).toEqual(["coffee", "nap", "phone"]);
  });
});