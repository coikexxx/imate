import { describe, expect, it } from "vitest";
import type { AgentStatus } from "../types";
import { CELL } from "./layout";
import { ALL_SPRITES, PALETTE, emptySeatLayers, seatLayers } from "./sprites";

const STATUSES: AgentStatus[] = ["idle", "thinking", "talking", "working", "error"];
const ACTS = ["coffee", "nap", "phone"] as const;

describe("像素图", () => {
  it("每张图都是矩形，只用调色板里的字符", () => {
    expect(ALL_SPRITES.length).toBeGreaterThan(10);
    for (const sprite of ALL_SPRITES) {
      expect(sprite.length).toBeGreaterThan(0);
      for (const row of sprite) {
        expect(row.length).toBe(sprite[0].length);
        for (const char of row) expect(char === "." || char === "C" || char in PALETTE).toBe(true);
      }
    }
  });
  it("每种状态的动画是 2 到 4 帧，图层不超出工位", () => {
    for (const status of STATUSES) {
      for (const act of ACTS) {
        const frames = new Set<string>();
        for (let tick = 0; tick < 12; tick += 1) {
          const layers = seatLayers(status, act, tick);
          frames.add(JSON.stringify(layers));
          for (const layer of layers) {
            expect(layer.dx).toBeGreaterThanOrEqual(0);
            expect(layer.dy).toBeGreaterThanOrEqual(0);
            expect(layer.dx + layer.sprite[0].length).toBeLessThanOrEqual(CELL);
            expect(layer.dy + layer.sprite.length).toBeLessThanOrEqual(CELL);
          }
        }
        expect(frames.size, `${status}/${act}`).toBeGreaterThanOrEqual(2);
        expect(frames.size, `${status}/${act}`).toBeLessThanOrEqual(4);
      }
    }
  });
  it("动画按 tick 循环，负数和很大的 tick 也有图", () => {
    expect(seatLayers("thinking", "coffee", 0)).toEqual(seatLayers("thinking", "coffee", 3));
    expect(seatLayers("working", "coffee", -1).length).toBeGreaterThan(0);
    expect(seatLayers("talking", "coffee", 1_000_001).length).toBeGreaterThan(0);
  });
  it("三种摸鱼动作画面不同，空工位只有桌子和关着的电脑", () => {
    const pictures = ACTS.map((act) => JSON.stringify(seatLayers("idle", act, 0)));
    expect(new Set(pictures).size).toBe(3);
    expect(emptySeatLayers()).toHaveLength(2);
  });
});
